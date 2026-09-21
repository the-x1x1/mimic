import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { contractFixture, importFixture } from "@mimic/test-fixtures";
import {
  ADJUSTMENT_LABELS,
  Adjustment,
  CHANNEL_LABELS,
  Channel,
  DeletionReport,
  Draft,
  DraftOutcomes,
  GenerationContext,
  IDENTIFIER_LABELS,
  IdentifierKind,
  JOB_KINDS,
  JOB_LABELS,
  LatestJson,
  MIN_SAMPLE,
  Settings,
  VoiceOverview,
  canFinishOnboarding,
  composeReadiness,
  describeDeletion,
  describeImport,
  describeMetric,
  formatDuration,
  formatRate,
  importStepState,
  nextCheckDelayMs,
  nextOnboardingStep,
} from "../src";

const read = (path: string) => JSON.parse(readFileSync(path, "utf8"));

describe("fixtures written by the Rust pipeline parse against the zod schemas", () => {
  it("parses the voice overview", () => {
    const overview = VoiceOverview.parse(read(contractFixture("voice_overview.json")));
    expect(overview.ownMessages).toBeGreaterThan(0);
    expect(overview.profiles.length).toBeGreaterThan(0);
    // The fixture deliberately contains both a measurable and an unmeasurable
    // layer, because rendering the second one is where the UI goes wrong.
    expect(overview.profiles.some((p) => p.measurable)).toBe(true);
    const thin = overview.profiles.find((p) => !p.measurable);
    expect(thin).toBeDefined();
    expect(thin!.metrics.emojiRate).toBeNull();
    expect(thin!.metrics.sampleSize).toBeGreaterThan(0);
  });

  it("parses the generation context, including its human-readable evidence", () => {
    const ctx = GenerationContext.parse(read(contractFixture("generation_context.json")));
    expect(ctx.evidence.length).toBeGreaterThan(0);
    expect(ctx.evidence.every((e) => typeof e === "string" && e.length > 0)).toBe(true);
    expect(ctx.examples.length).toBeGreaterThan(0);
    expect(ctx.examples[0]!.reason).toBeTruthy();
    expect(ctx.voice.layers.length).toBeGreaterThan(0);
  });

  it("parses a draft and a deletion report", () => {
    const draft = Draft.parse(read(contractFixture("draft.json")));
    expect(draft.promptHash).toBeTruthy();
    expect(draft.outcome).toBeNull();
    const report = DeletionReport.parse(read(contractFixture("deletion_report.json")));
    expect(report.messages).toBeGreaterThan(0);
  });

  it("rejects a payload that is missing a field the UI reads", () => {
    const overview = read(contractFixture("voice_overview.json"));
    delete overview.messagesUntilMeasurable;
    expect(() => VoiceOverview.parse(overview)).toThrow();
  });

  it("rejects an unmeasured rate sent as 0 instead of null", () => {
    const overview = read(contractFixture("voice_overview.json"));
    overview.ownMessages = "lots";
    expect(() => VoiceOverview.parse(overview)).toThrow();
  });
});

describe("the import fixture is the format the docs describe", () => {
  it("has conversations of messages with identifiable authors", () => {
    const exported = read(importFixture("sample_export.json"));
    expect(Channel.parse(exported.channel)).toBe("chat");
    expect(exported.conversations.length).toBeGreaterThan(1);
    const messages = exported.conversations.flatMap((c: { messages: unknown[] }) => c.messages);
    expect(messages.length).toBe(45);
    const identifiable = messages.filter(
      (m: { from: Record<string, unknown> }) =>
        m.from.email || m.from.phone || m.from.handle || m.from.accountId,
    );
    expect(identifiable.length).toBe(44);
  });
});

describe("rates are rendered honestly", () => {
  it("distinguishes a measured zero from an unmeasured value", () => {
    expect(formatRate(0)).toBe("0%");
    expect(formatRate(null)).toBe("not measured yet");
    expect(formatRate(0.336)).toBe("34%");
  });

  it("describes only what was measured", () => {
    const unmeasured = {
      sampleSize: 3,
      measurable: false,
      avgWordsPerMessage: null,
      medianWordsPerMessage: null,
      p90WordsPerMessage: null,
      avgSentencesPerMessage: null,
      multiParagraphRate: null,
      terminalPeriodRate: null,
      questionRate: null,
      exclamationRate: null,
      ellipsisRate: null,
      emojiRate: null,
      lowercaseStartRate: null,
      allLowercaseRate: null,
      contractionsPer100Words: null,
      greetingRate: null,
      signOffRate: null,
      topGreetings: [],
      topSignOffs: [],
      topPhrases: [],
      medianResponseSeconds: null,
    };
    expect(describeMetric("emojiRate", unmeasured)).toBeNull();
    const measured = { ...unmeasured, measurable: true, emojiRate: 0, medianWordsPerMessage: 7 };
    expect(describeMetric("emojiRate", measured)).toContain("0%");
    expect(describeMetric("medianWordsPerMessage", measured)).toBe("7 words in a typical message");
  });

  it("formats durations the way a person would say them", () => {
    expect(formatDuration(null)).toBe("not measured yet");
    expect(formatDuration(45)).toBe("45 seconds");
    expect(formatDuration(600)).toBe("10 minutes");
    expect(formatDuration(7200)).toBe("2 hours");
    expect(formatDuration(259200)).toBe("3 days");
  });
});

describe("deletion is described in consequences, not counts", () => {
  const base = {
    participants: 1,
    identifiers: 2,
    conversations: 1,
    messages: 40,
    ownMessages: 20,
    embeddings: 0,
    representativeExamples: 6,
    voiceProfiles: 1,
    voicePreferences: 1,
    drafts: 2,
    profilesInvalidated: 3,
    conversationsKept: 1,
  };

  it("warns that the user's own messages go too", () => {
    const lines = describeDeletion(base);
    expect(lines.some((l) => l.includes("20 of those are messages you wrote"))).toBe(true);
    expect(lines.some((l) => l.includes("group conversations will be kept"))).toBe(true);
    expect(lines[lines.length - 1]).toBe("This cannot be undone.");
  });

  it("says nothing it cannot substantiate", () => {
    const empty = {
      ...base,
      messages: 0,
      ownMessages: 0,
      drafts: 0,
      conversationsKept: 0,
      profilesInvalidated: 0,
    };
    const lines = describeDeletion(empty);
    expect(lines.some((l) => l.includes("messages you wrote"))).toBe(false);
    expect(lines.some((l) => l.includes("drafts"))).toBe(false);
  });
});

describe("import summaries name what was skipped", () => {
  it("mentions duplicates and unattributed messages", () => {
    const line = describeImport({
      conversations: 3,
      inserted: 44,
      duplicates: 2,
      empty: 1,
      fromSelf: 22,
      unattributed: 1,
      participantsCreated: 2,
    });
    expect(line).toContain("44 messages from 3 conversations");
    expect(line).toContain("22 written by you");
    expect(line).toContain("2 already imported");
    expect(line).toContain("1 with no identifiable author");
  });

  it("stays quiet about the things that did not happen", () => {
    const line = describeImport({
      conversations: 1,
      inserted: 10,
      duplicates: 0,
      empty: 0,
      fromSelf: 5,
      unattributed: 0,
      participantsCreated: 1,
    });
    expect(line).not.toContain("already imported");
    expect(line).not.toContain("no identifiable author");
  });
});

describe("onboarding advances on facts", () => {
  const none = {
    completed: false,
    hasIdentity: false,
    hasSource: false,
    hasOwnMessages: false,
    hasVoiceProfile: false,
  };

  it("returns the first unfinished step", () => {
    expect(nextOnboardingStep(none)).toBe("identity");
    expect(nextOnboardingStep({ ...none, hasIdentity: true })).toBe("source");
    expect(nextOnboardingStep({ ...none, hasIdentity: true, hasSource: true })).toBe("import");
    expect(
      nextOnboardingStep({ ...none, hasIdentity: true, hasSource: true, hasOwnMessages: true }),
    ).toBe("analyze");
    expect(
      nextOnboardingStep({
        completed: true,
        hasIdentity: true,
        hasSource: true,
        hasOwnMessages: true,
        hasVoiceProfile: true,
      }),
    ).toBeNull();
  });
});

describe("onboarding cannot dead-end", () => {
  const base = {
    completed: false,
    hasIdentity: true,
    hasSource: true,
    hasOwnMessages: true,
    hasVoiceProfile: false,
  };

  it("lets someone with messages of their own leave before a profile exists", () => {
    // Under twenty own messages there will never be a measurable profile, and
    // refusing to let them in would be permanent.
    expect(canFinishOnboarding(base)).toBe(true);
    expect(nextOnboardingStep(base)).toBe("analyze");
    expect(canFinishOnboarding({ ...base, hasOwnMessages: false })).toBe(false);
    expect(canFinishOnboarding({ ...base, hasSource: false })).toBe(false);
  });

  it("names the state where an import read a file and found nothing of yours", () => {
    expect(importStepState([])).toBe("no-source");
    expect(importStepState([{ status: "new", messageCount: 0 }])).toBe("not-started");
    expect(importStepState([{ status: "importing", messageCount: 0 }])).toBe("running");
    expect(importStepState([{ status: "failed", messageCount: 0 }])).toBe("failed");
    // The file was read; those messages are simply all from other people.
    expect(importStepState([{ status: "imported", messageCount: 4000 }])).toBe("none-of-yours");
  });
});

describe("compose readiness explains itself", () => {
  const layer = (stale: boolean) => ({
    layer: "global",
    scopeKey: "",
    label: "Everything you write",
    sampleSize: 100,
    measurable: true,
    stale,
    metrics: VoiceOverview.parse(read(contractFixture("voice_overview.json"))).profiles[0]!.metrics,
  });

  const context = (measurable: boolean, stale: boolean) =>
    ({
      participant: null,
      channel: "email",
      voice: { layers: [layer(stale)], overrides: [], examples: [] },
      effective: { ...layer(stale).metrics, measurable },
      examples: [],
      transcript: [],
      evidence: [],
    }) as never;

  it("says when there is not enough writing yet, but still allows a draft", () => {
    const r = composeReadiness(context(false, false));
    expect(r.ready).toBe(true);
    expect(r.reason).toContain("not seen enough of your writing");
  });

  it("says when the profile is out of date", () => {
    expect(composeReadiness(context(true, true)).reason).toContain("Re-analyze");
  });

  it("says nothing when there is nothing to say", () => {
    expect(composeReadiness(context(true, false)).reason).toBeNull();
  });

  it("refuses the draft outright when the model is not answering", () => {
    const unreachable = composeReadiness(context(true, false), {
      displayName: "Local model",
      local: true,
      reachable: false,
      error: "connection refused",
    });
    expect(unreachable.ready).toBe(false);
    expect(unreachable.reason).toContain("not answering");
    expect(unreachable.reason).toContain("connection refused");
  });

  it("does not assume a model answers before it has been checked", () => {
    const unknown = composeReadiness(context(true, false), {
      displayName: "Local model",
      local: true,
      reachable: null,
    });
    expect(unknown.ready).toBe(true);
    expect(unknown.reason).toBeNull();
  });
});

describe("enumerations stay in step with their labels", () => {
  it("labels every channel, identifier kind and adjustment", () => {
    for (const c of Channel.options) expect(CHANNEL_LABELS[c]).toBeTruthy();
    for (const k of IdentifierKind.options) expect(IDENTIFIER_LABELS[k]).toBeTruthy();
    for (const a of Adjustment.options) expect(ADJUSTMENT_LABELS[a]).toBeTruthy();
  });

  it("labels every job kind the native side can enqueue", () => {
    for (const kind of Object.values(JOB_KINDS)) expect(JOB_LABELS[kind]).toBeTruthy();
  });

  it("keeps the sample threshold in one place", () => {
    expect(MIN_SAMPLE).toBe(20);
  });
});

describe("settings and updater contracts", () => {
  it("accepts the default settings map and rejects an unknown channel", () => {
    const defaults = {
      "general.theme": "dark",
      "performance.workerConcurrency": 2,
      "privacy.networkFeatures": false,
      "updates.channel": "stable",
      "updates.automatic": true,
      "diagnostics.includePaths": false,
      "generation.provider": "local",
      "generation.localUrl": "http://127.0.0.1:11434/v1",
      "generation.localModel": "llama3.1:8b",
      "generation.anthropicModel": "claude-sonnet-4-5",
      "onboarding.completed": false,
    };
    expect(() => Settings.parse(defaults)).not.toThrow();
    expect(() => Settings.parse({ ...defaults, "updates.channel": "nightly" })).toThrow();
  });

  it("validates latest.json and jitters the update check", () => {
    expect(() =>
      LatestJson.parse({
        version: "0.6.0",
        platforms: { "windows-x86_64": { signature: "sig", url: "https://example.com/x.zip" } },
      }),
    ).not.toThrow();
    expect(() =>
      LatestJson.parse({
        version: "0.6.0",
        platforms: { "windows-x86_64": { signature: "", url: "x" } },
      }),
    ).toThrow();
    const six = 6 * 60 * 60 * 1000;
    expect(nextCheckDelayMs(() => 0.5)).toBe(six);
    expect(nextCheckDelayMs(() => 0)).toBeLessThan(six);
    expect(nextCheckDelayMs(() => 1)).toBeGreaterThan(six);
  });
});

describe("draft outcomes", () => {
  it("keeps an unmeasured rate null rather than zero", () => {
    const o = DraftOutcomes.parse({
      total: 3,
      resolved: 0,
      sentUnedited: 0,
      sentEdited: 0,
      discarded: 0,
      uneditedRate: null,
      meanLengthDelta: null,
    });
    expect(o.uneditedRate).toBeNull();
    expect(formatRate(o.uneditedRate)).toBe("not measured yet");
  });
});
