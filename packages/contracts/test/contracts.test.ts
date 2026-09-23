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
  LearningOverview,
  Settings,
  SituationSummary,
  VoiceOverview,
  Dashboard,
  canFinishOnboarding,
  canLeaveOnboarding,
  composeReadiness,
  describeDeletion,
  describeWaiting,
  describePeopleList,
  PeopleView,
  ProviderState,
  describeCredentials,
  type CredentialState,
  describeAutomated,
  describeLeftOut,
  describeMailChecking,
  ThreadMark,
  describeImport,
  describeMetric,
  describeSituation,
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

  it("parses the dashboard, with a prepared draft attached to its thread", () => {
    const view = Dashboard.parse(read(contractFixture("dashboard.json")));
    expect(view.messages).toBeGreaterThan(0);
    expect(view.awaiting.length).toBeGreaterThan(0);
    // The fixture is generated with assisted drafting on, so at least one
    // waiting thread carries an unresolved draft — the state the approve /
    // modify / reject controls render against.
    const withDraft = view.awaiting.find((t) => t.draft !== null);
    expect(withDraft).toBeDefined();
    expect(withDraft!.draft!.outcome).toBeNull();
    expect(withDraft!.lastMessage.length).toBeGreaterThan(0);
    expect(view.outcomes.uneditedRate).toBeNull();
    // A person is waiting; the newsletter the fixture adds is left out,
    // counted, and carried with its reason because the fixture asked for it.
    expect(withDraft!.automated).toBeNull();
    expect(view.leftOut).toEqual({ automated: 1, notNeeded: 0 });
    expect(view.showingLeftOut).toBe(true);
    expect(view.leftOutThreads).toHaveLength(1);
    expect(view.leftOutThreads[0]!.automated).toBe("newsletter");
    expect(view.leftOutThreads[0]!.mark).toBeNull();
  });

  it("parses the situation vocabulary, all six, with counts rather than scores", () => {
    const list = SituationSummary.array().parse(read(contractFixture("situations.json")));
    expect(list.map((s) => s.id)).toEqual([
      "declining",
      "scheduling",
      "apologising",
      "thanking",
      "explaining",
      "disagreeing",
    ]);
    expect(list.every((s) => s.measurable === s.ownMessages >= MIN_SAMPLE)).toBe(true);
  });

  it("parses a generation context written for a situation read from a note", () => {
    const ctx = GenerationContext.parse(read(contractFixture("generation_context_situation.json")));
    expect(ctx.situation).not.toBeNull();
    expect(ctx.situation!.id).toBe("declining");
    expect(ctx.situation!.source).toBe("fromNote");
    expect(ctx.situation!.cue).toBeTruthy();
    // No situation is a real, common state and must parse too.
    const plain = GenerationContext.parse(read(contractFixture("generation_context.json")));
    expect(plain.situation).toBeNull();
  });

  it("parses what was learned from sent drafts, holding and forming apart", () => {
    const o = LearningOverview.parse(read(contractFixture("learning.json")));
    expect(o.draftsConsidered).toBeGreaterThan(0);
    expect(o.minAgreeing).toBe(3);
    const holding = o.patterns.filter((p) => p.holds);
    expect(holding.length).toBeGreaterThan(0);
    expect(holding.every((p) => p.agreeing >= o.minAgreeing && p.share > 0.5)).toBe(true);
    expect(o.patterns.every((p) => p.summary.length > 0)).toBe(true);
    expect(o.notes.length).toBeGreaterThan(0);
    expect(o.notes.every((n) => !n.text.startsWith("said:"))).toBe(true);
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
    expect(messages.length).toBe(46);
    const identifiable = messages.filter(
      (m: { from: Record<string, unknown> }) =>
        m.from.email || m.from.phone || m.from.handle || m.from.accountId,
    );
    expect(identifiable.length).toBe(45);
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

  it("lets someone who has imported nothing look around anyway", () => {
    // The screen that prompted this: identity declared, no source yet, and no
    // way forward but exporting a mailbox first.
    const atSource = { ...base, hasSource: false, hasOwnMessages: false };
    expect(canFinishOnboarding(atSource)).toBe(false);
    expect(canLeaveOnboarding(atSource)).toBe(true);
    // Same at the import step, where a source exists but nothing of the user's
    // has landed yet.
    expect(canLeaveOnboarding({ ...base, hasOwnMessages: false })).toBe(true);
  });

  it("holds the identity step, because an import before it attributes nothing", () => {
    expect(canLeaveOnboarding({ ...base, hasIdentity: false })).toBe(false);
    expect(canFinishOnboarding({ ...base, hasIdentity: false })).toBe(false);
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

describe("the home screen speaks for itself, and does not pretend to be an inbox", () => {
  const base = Dashboard.parse(read(contractFixture("dashboard.json")));

  it("says nothing has been read yet, and that nothing arrives on its own", () => {
    const line = describeWaiting({ ...base, lastImportAt: null });
    expect(line).toContain("haven't read any of your mail");
    expect(line).toContain("Point me at it");
  });

  it("counts what is waiting, not what fits on the screen", () => {
    const shown = base.awaiting.slice(0, 1);
    const line = describeWaiting({ ...base, awaitingTotal: 9, awaiting: shown });
    expect(line).toContain("9 ");
    expect(line).toContain(`Here are the ${shown.length} most recent`);
  });

  it("says when nothing is waiting instead of showing an empty list", () => {
    expect(describeWaiting({ ...base, awaitingTotal: 0, awaiting: [] })).toContain("all caught up");
  });

  it("counts people only when every waiting thread really is one person", () => {
    const person = base.awaiting.find((t) => t.participant !== null && !t.isGroup);
    expect(person, "the fixture should contain an attributed one-to-one thread").toBeDefined();
    expect(describeWaiting({ ...base, awaitingTotal: 1, awaiting: [person!] })).toBe(
      "One person is waiting on you.",
    );
    // A group is not a person, and an unattributed thread is nobody: the
    // sentence steps back to "conversation" rather than guessing.
    expect(
      describeWaiting({
        ...base,
        awaitingTotal: 1,
        awaiting: [{ ...person!, isGroup: true }],
      }),
    ).toBe("One conversation is waiting on you.");
    expect(
      describeWaiting({
        ...base,
        awaitingTotal: 2,
        awaiting: [person!, { ...person!, participant: null }],
      }),
    ).toBe("2 conversations are waiting on you.");
  });
});

describe("people are people", () => {
  const view = PeopleView.parse(read(contractFixture("people.json")));

  it("parses the People screen, with the automated sender apart", () => {
    expect(view.peopleTotal).toBe(2);
    expect(view.people.every((p) => !p.automated)).toBe(true);
    expect(view.automatedSendersTotal).toBe(1);
    expect(view.showingAutomated).toBe(true);
    expect(view.automatedSenders[0]!.automated).toBe(true);
  });

  it("says what it left out, and when the list is cut short", () => {
    expect(describePeopleList({ ...view, automatedSendersTotal: 0 })).toBeNull();
    expect(describePeopleList({ ...view, automatedSendersTotal: 1 })).toBe(
      "I left out one sender whose mail all looks automated.",
    );
    const cut = describePeopleList({ ...view, peopleTotal: 250, automatedSendersTotal: 0 });
    expect(cut).toBe(`Here are the ${view.people.length} most recent of 250.`);
    expect(describePeopleList({ ...view, automatedSendersTotal: 40 })).toMatch(
      /^I left out 40 senders whose mail all looks automated/,
    );
  });
});

describe("what was left out is said, and said as a reading", () => {
  const base = Dashboard.parse(read(contractFixture("dashboard.json")));

  it("says nothing when nothing was left out", () => {
    expect(describeLeftOut({ ...base, leftOut: { automated: 0, notNeeded: 0 } })).toBeNull();
  });

  it("counts each reason, and gets one right", () => {
    expect(describeLeftOut({ ...base, leftOut: { automated: 1, notNeeded: 0 } })).toBe(
      "I left out one thread that looks automated.",
    );
    expect(describeLeftOut({ ...base, leftOut: { automated: 0, notNeeded: 2 } })).toBe(
      "I left out 2 you said don't need a reply.",
    );
    const both = describeLeftOut({ ...base, leftOut: { automated: 12, notNeeded: 1 } });
    expect(both).toBe(
      "I left out 12 threads that look automated (newsletters, notifications and the like) and one you said doesn't need a reply.",
    );
  });

  it("words every reason Rust stores, and something true for one it does not know", () => {
    for (const reason of ["newsletter", "bulk", "auto_reply", "report", "no_reply_address"]) {
      const line = describeAutomated(reason);
      expect(line).toBeTruthy();
      expect(line).not.toMatch(/headers say a machine/);
    }
    expect(describeAutomated("something_new")).toBe("its headers say a machine sent it.");
    expect(describeAutomated(null)).toBeNull();
  });

  it("accepts the two marks and nothing else", () => {
    expect(ThreadMark.parse("no_reply_needed")).toBe("no_reply_needed");
    expect(ThreadMark.parse("needs_reply")).toBe("needs_reply");
    expect(() => ThreadMark.parse("snoozed")).toThrow();
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
      "general.theme": "plain",
      "performance.workerConcurrency": 2,
      "privacy.networkFeatures": false,
      "updates.channel": "stable",
      "updates.automatic": true,
      "diagnostics.includePaths": false,
      "generation.provider": "local",
      "generation.localUrl": "http://127.0.0.1:11434/v1",
      "generation.localModel": "llama3.2:3b",
      "generation.anthropicModel": "claude-sonnet-4-5",
      "assist.autoDraft": false,
      "mail.checkEveryMinutes": 15,
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

describe("describeSituation words a reading as a reading", () => {
  it("says nothing when there is no situation", () => {
    expect(describeSituation(null)).toBeNull();
    expect(describeSituation(undefined)).toBeNull();
  });
  it("credits the user only when they chose it", () => {
    expect(
      describeSituation({ id: "declining", label: "Saying no", source: "chosen", cue: null }),
    ).toBe("Written as saying no, because you said so.");
    const read = describeSituation({
      id: "declining",
      label: "Saying no",
      source: "fromNote",
      cue: "say no",
    })!;
    expect(read).toMatch(/^Your note read like saying no/);
    expect(read).not.toMatch(/you said/);
  });
});

describe("the home screen says whether mail arrives by itself, and only when it does", () => {
  const base = Dashboard.parse(read(contractFixture("dashboard.json")));
  it("says nothing about checking when no mailbox is connected", () => {
    expect(base.mailChecking).toBeNull();
    expect(describeMailChecking(base)).toBeNull();
  });
  it("names the interval, or that checking is off", () => {
    const on = {
      ...base,
      mailChecking: { mailboxes: 1, everyMinutes: 15, lastCheckedAt: null, failing: null },
    };
    expect(describeMailChecking(on)).toBe("I check your mailbox every 15 minutes.");
    const off = {
      ...base,
      mailChecking: { mailboxes: 2, everyMinutes: 0, lastCheckedAt: null, failing: null },
    };
    expect(describeMailChecking(off)).toMatch(/^Checking your mailboxes is turned off/);
  });
  it("reads a first check as a first check, not as nothing imported", () => {
    const first = {
      ...base,
      lastImportAt: null,
      mailChecking: { mailboxes: 1, everyMinutes: 15, lastCheckedAt: null, failing: null },
    };
    expect(describeWaiting(first)).toMatch(/first time/);
    const broken = { ...first, mailChecking: { ...first.mailChecking, failing: "wrong password" } };
    expect(describeWaiting(broken)).not.toMatch(/first time/);
    expect(describeMailChecking(broken)).toBe(
      "The last check of your mailbox failed: wrong password",
    );
  });
});

describe("connecting a mailbox", () => {
  it("guesses the server for the common providers and nothing for the rest", async () => {
    const { guessMailHost } = await import("../src");
    expect(guessMailHost("c@gmail.com")?.host).toBe("imap.gmail.com");
    expect(guessMailHost("c@icloud.com")?.port).toBe(993);
    // Outlook.com is known, and known not to work with a password.
    expect(guessMailHost("C@Outlook.com")?.supported).toBe(false);
    expect(guessMailHost("c@formicaria.us")).toBeNull();
    expect(guessMailHost("not an address")).toBeNull();
  });
});

describe("saved passwords are described as they are kept", () => {
  const kept = (over: Partial<CredentialState> = {}): CredentialState => ({
    protection: "account",
    unsealedLeft: false,
    locked: [],
    unreadable: null,
    ...over,
  });

  it("parses the provider state, with keys and never values", () => {
    const state = ProviderState.parse(read(contractFixture("provider_state.json")));
    expect(state.configuredSecrets).toEqual(["provider.anthropic.apiKey"]);
    expect(["account", "file"]).toContain(state.credentials.protection);
    // The fixture has one password saved on another account, which is the
    // state the Settings screen has to explain.
    expect(state.credentials.locked).toEqual(["imap:elsewhere"]);
    expect(state.credentials.unsealedLeft).toBe(false);
    expect(state.credentials.unreadable).toBeNull();
    expect(JSON.stringify(state)).not.toContain("sk-");
  });

  it("says what protects them, and what does not", () => {
    const sealed = describeCredentials(kept());
    expect(sealed).toMatch(/locked to your Windows account/);
    expect(sealed).toMatch(/can't be opened without your Windows password/);
    expect(sealed).toMatch(/A program you run yourself could still unlock it/);
    expect(sealed).toMatch(
      /an administrator of this computer or, on a work account, your organisation's IT/,
    );
    expect(sealed).not.toMatch(/before this version/);
    const plain = describeCredentials(kept({ protection: "file" }));
    expect(plain).toMatch(/isn't locked to your account/);
    expect(plain).not.toMatch(/locked to your Windows account/);
  });

  it("says when something unsealed is still on disk", () => {
    expect(describeCredentials(kept({ unsealedLeft: true }))).toMatch(
      /The file they were kept in before this version isn't locked and is still on this computer/,
    );
  });

  it("says when the file could not be read, and what that means", () => {
    expect(describeCredentials(kept({ unreadable: "setAside" }))).toMatch(
      /I put it aside; any you saved before then need entering again\./,
    );
    expect(describeCredentials(kept({ unreadable: "leftAlone" }))).toMatch(/won't save over it/);
  });

  it("names how many need entering again, and only when some do", () => {
    expect(describeCredentials(kept({ locked: ["imap:a"] }))).toMatch(
      /I can't unlock one saved key or password any more: it was locked to another Windows account or computer, or before your Windows password was reset, so it needs entering again\./,
    );
    expect(describeCredentials(kept({ locked: ["imap:a", "provider.anthropic.apiKey"] }))).toMatch(
      /I can't unlock 2 saved keys or passwords any more: .* they need entering again\./,
    );
    expect(describeCredentials(kept())).not.toMatch(/can't unlock/);
  });
});
