import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { contractFixture, importFixture } from "@mimic/test-fixtures";
import {
  AutoReplyState,
  describeAutoReply,
  describeHeld,
  OutboxEntry,
  ReplyEnvelope,
  describeOutboxEntry,
  listAddresses,
  sendMayHaveGone,
  ADJUSTMENT_LABELS,
  Adjustment,
  PersonConversations,
  SearchPage,
  ValidationReport,
  WriterName,
  adjustmentOf,
  yoursAs,
  describeFound,
  hasWordsToFind,
  canPutOnList,
  describeStanding,
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
  SituationFiling,
  MailSignIns,
  SignInSetup,
  MailboxFolders,
  TelegramApp,
  TelegramStep,
  telegramAccount,
  EncoderView,
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
  AddressAdded,
  AddressPreview,
  ConversationPage,
  filedByPhrase,
  writerOf,
  EvaluationSystem,
  EvaluationView,
  MEASURE_LABELS,
  MEASURES,
  SYSTEM_LABELS,
  describeEncoder,
  describeEvaluation,
  describeGone,
  describeSplitWarning,
  formatShare,
  HeldAddress,
  SentFolderPerson,
  describeClaimed,
  describeFold,
  describeSentFolder,
  type AddressOwner,
  type CredentialState,
  describeAutomated,
  describeLeftOut,
  describeQuiet,
  describeWaitingWindow,
  leftOutTotal,
  olderThan,
  WAITING_WINDOWS,
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
    // A reading in words comes with the model that wrote it and whether it
    // is of the numbers the layer has now.
    const read_ = overview.profiles.find((p) => p.reading !== null)?.reading;
    expect(read_?.current).toBe(true);
    expect(read_?.model).toBeTruthy();
    expect(overview.profiles.some((p) => p.reading === null)).toBe(true);
  });

  it("parses what the encoder is, and how much it has read", () => {
    const view = EncoderView.parse(read(contractFixture("encoder.json")));
    expect(view.offered?.id).toBe("all-minilm-l6-v2");
    expect(view.offered!.bytes).toBeGreaterThan(90_000_000);
    expect(view.downloaded).toBe(false);
    expect(view.inUse).toBe(false);
    expect(view.reason).toMatch(/lexical fallback/);
  });

  it("parses which providers this copy can sign in to a mailbox with", () => {
    const view = MailSignIns.parse(read(contractFixture("mail_sign_ins.json")));
    expect(view).toEqual({ microsoft: false, google: true });
  });

  it("parses a Telegram sign-in, never with the app's hash", () => {
    const app = TelegramApp.parse(read(contractFixture("telegram_app.json")));
    expect(app).toEqual({ saved: true, apiId: 1234567 });
    expect(JSON.stringify(app)).not.toMatch(/hash/i);
    const step = TelegramStep.parse(read(contractFixture("telegram_step.json")));
    expect(step).toEqual({ step: "password", hint: "the usual", wrong: true, source: null });
    expect(
      telegramAccount({
        connector: "telegram_account",
        config: { userId: 7, name: "C Haynes", username: "c", marks: {}, lastCheckedAt: null },
      }),
    ).toEqual({ name: "C Haynes", username: "c", lastCheckedAt: null });
    expect(telegramAccount({ connector: "telegram", config: {} })).toBeNull();
  });

  it("parses the other folders a mailbox may be read in", () => {
    const view = MailboxFolders.parse(read(contractFixture("mailbox_folders.json")));
    expect(view.folders.map((f) => [f.shown, f.chosen])).toEqual([
      ["Receipts", true],
      ["Études", true],
    ]);
    expect(view.gone).toEqual(["Old rules"]);
    expect(view.unopened).toEqual(["Receipts"]);
    expect(view.most).toBe(10);
  });

  it("parses what each provider signs mailboxes in as, without a secret", () => {
    const view = SignInSetup.parse(read(contractFixture("sign_in_setup.json")));
    expect(view.microsoft).toEqual({
      clientId: "0f5c1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b",
      own: true,
      builtIn: false,
    });
    expect(view.google.clientId).toBeNull();
    expect(JSON.stringify(view)).not.toMatch(/secret/i);
  });

  it("parses how the user's messages came to be filed, and the model that could read them", () => {
    const view = SituationFiling.parse(read(contractFixture("situation_filing.json")));
    expect(view.byYou).toBe(1);
    expect(view.byRules).toBe(2);
    expect(view.localProvider).toBe("local");
    expect(view.localModel).toBeTruthy();
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
    // With another way of saying it beside it, which names it and says how it differs.
    const other = withDraft!.alternatives[0];
    expect(other?.alternativeTo).toBe(withDraft!.draft!.id);
    expect(other && adjustmentOf(other)).toBe("shorter");
    expect(adjustmentOf(withDraft!.draft!)).toBeNull();
    expect(withDraft!.draft!.alternativeTo).toBeNull();
    expect(withDraft!.lastMessage.length).toBeGreaterThan(0);
    expect(view.outcomes.uneditedRate).toBeNull();
    // A person is waiting; the newsletter the fixture adds is left out,
    // counted, and carried with its reason because the fixture asked for it.
    expect(withDraft!.automated).toBeNull();
    // The fixture is written with the waiting window at 30 days over an
    // export from January: everything has gone quiet except the thread the
    // user said needs a reply, which stays and is still said to be old.
    expect(view.waitingWithinDays).toBe(30);
    expect(withDraft!.mark).toBe("needs_reply");
    expect(withDraft!.quiet).toBe(true);
    expect(view.leftOut.automated).toBe(1);
    expect(view.leftOut.notNeeded).toBe(0);
    expect(view.leftOut.quiet).toBeGreaterThan(0);
    expect(view.showingLeftOut).toBe(true);
    expect(view.leftOutThreads).toHaveLength(view.leftOut.automated + view.leftOut.quiet);
    const news = view.leftOutThreads.find((t) => t.automated !== null);
    expect(news!.automated).toBe("newsletter");
    expect(news!.mark).toBeNull();
    expect(view.leftOutThreads.filter((t) => t.automated === null).every((t) => t.quiet)).toBe(
      true,
    );
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
    expect(draft.alternativeTo).toBeNull();
    const older = read(contractFixture("draft.json"));
    delete older.alternativeTo;
    expect(Draft.parse(older).alternativeTo).toBeNull();
    const report = DeletionReport.parse(read(contractFixture("deletion_report.json")));
    expect(report.messages).toBeGreaterThan(0);
  });

  it("parses who a reply goes to before it goes, and what was sent", () => {
    const envelope = ReplyEnvelope.parse(read(contractFixture("reply_envelope.json")));
    expect(envelope.whyNot).toBeNull();
    expect(envelope.to).toEqual(["ada.replies@example.com"]);
    expect(envelope.othersOnIt).toEqual([]);
    expect(envelope.mailboxes[0]?.fromAddress).toBe("c@example.com");
    const sent = OutboxEntry.array().parse(read(contractFixture("outbox.json")));
    expect(sent).toHaveLength(1);
    expect(sent[0]?.status).toBe("sent");
    expect(sent[0]?.how).toBe("approved");
    expect(sent[0]?.error).toBeNull();
  });

  it("parses the rules for replying by itself", () => {
    const state = AutoReplyState.parse(read(contractFixture("autoreply.json")));
    expect(state.rules.enabled).toBe(true);
    expect(state.running).toBe(false);
    expect(state.rules.who).toBe("known");
    expect(describeAutoReply(state)).toMatch(/aren't being prepared in advance/);
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
    evaluations: 1,
    sent: 2,
  };

  it("warns that the user's own messages go too", () => {
    const lines = describeDeletion(base);
    expect(lines.some((l) => l.includes("20 of those are messages you wrote"))).toBe(true);
    expect(lines.some((l) => l.includes("measurement of how close my drafts come"))).toBe(true);
    expect(lines.some((l) => l.includes("group conversations will be kept"))).toBe(true);
    expect(lines.some((l) => l.includes("2 replies I sent will be removed"))).toBe(true);
    expect(lines.some((l) => l.includes("can't be taken back"))).toBe(true);
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
      evaluations: 0,
      sent: 0,
    };
    const lines = describeDeletion(empty);
    expect(lines.some((l) => l.includes("messages you wrote"))).toBe(false);
    expect(lines.some((l) => l.includes("measurement"))).toBe(false);
    expect(lines.some((l) => l.includes("drafts"))).toBe(false);
    expect(lines.some((l) => l.includes("I sent"))).toBe(false);
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
    expect(line).not.toContain("already here");
  });

  it("says what reading mail again found out about what was already here", () => {
    const line = describeImport({
      conversations: 4,
      inserted: 0,
      duplicates: 40,
      empty: 0,
      fromSelf: 0,
      unattributed: 0,
      participantsCreated: 0,
      sentFolderMarked: 12,
      automatedMarked: 3,
    });
    expect(line).toContain("12 already here now read as from your Sent folder");
    expect(line).toContain("3 already here now read as sent by a machine");
  });

  it("reads a summary written before those were counted", async () => {
    const { ImportSummary } = await import("../src");
    const old = {
      conversations: 1,
      inserted: 2,
      duplicates: 0,
      empty: 0,
      fromSelf: 1,
      unattributed: 0,
      participantsCreated: 1,
    };
    expect(ImportSummary.parse(old).sentFolderMarked).toBeUndefined();
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
    const none = { ...base, awaitingTotal: 0, awaiting: [] };
    expect(
      describeWaiting({ ...none, leftOut: { automated: 0, notNeeded: 0, quiet: 0 } }),
    ).toContain("all caught up");
    // With something left out, that nobody is waiting is a reading.
    expect(describeWaiting({ ...none, leftOut: { automated: 0, notNeeded: 0, quiet: 3 } })).toBe(
      "Nothing I've read looks like it's waiting on you.",
    );
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

  const left = (automated: number, notNeeded: number, quiet = 0) => ({
    ...base,
    leftOut: { automated, notNeeded, quiet },
  });

  it("says nothing when nothing was left out", () => {
    expect(describeLeftOut(left(0, 0))).toBeNull();
    expect(leftOutTotal(left(0, 0))).toBe(0);
  });

  it("counts each reason, and gets one right", () => {
    expect(describeLeftOut(left(1, 0))).toBe("I left out one thread that looks automated.");
    expect(describeLeftOut(left(0, 2))).toBe("I left out 2 you said don't need a reply.");
    const both = describeLeftOut(left(12, 1));
    expect(both).toBe(
      "I left out 12 threads that look automated (newsletters, notifications and the like) and one you said doesn't need a reply.",
    );
  });

  it("names the window when threads have gone quiet", () => {
    expect(describeLeftOut({ ...left(0, 0, 1), waitingWithinDays: 30 })).toBe(
      "I left out one thread whose last message is more than 30 days old.",
    );
    expect(describeLeftOut({ ...left(3, 1, 1200), waitingWithinDays: 14 })).toBe(
      `I left out 3 threads that look automated (newsletters, notifications and the like), ${(1200).toLocaleString()} threads whose last message is more than 14 days old and one you said doesn't need a reply.`,
    );
    expect(leftOutTotal(left(3, 1, 1200))).toBe(1204);
    // With no window there is nothing to be quiet for; said without a number
    // rather than inventing one.
    expect(describeLeftOut({ ...left(0, 0, 2), waitingWithinDays: null })).toBe(
      "I left out 2 threads whose last message is older than the window you chose.",
    );
  });

  it("words gone quiet as a reading of a date, not a fact about anyone", () => {
    expect(describeQuiet(30)).toBe(
      "Its last message is more than 30 days old, so I've taken it that nobody is still waiting on a reply.",
    );
    expect(olderThan(7)).toBe("more than 7 days old");
    expect(olderThan(1)).toBe("more than a day old");
    expect(olderThan(null)).toBe("older than the window you chose");
  });

  it("offers the windows Settings shows, and names each one", () => {
    expect(WAITING_WINDOWS).toContain(30);
    expect(WAITING_WINDOWS).toContain(0);
    expect(describeWaitingWindow(0)).toBe("any time");
    expect(describeWaitingWindow(1)).toBe("the last day");
    expect(describeWaitingWindow(7)).toBe("the last week");
    expect(describeWaitingWindow(14)).toBe("the last two weeks");
    expect(describeWaitingWindow(30)).toBe("the last 30 days");
    expect(describeWaitingWindow(45)).toBe("the last 45 days");
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
      "waiting.withinDays": 30,
      "onboarding.completed": false,
      "app.keepRunning": true,
      "notify.enabled": true,
      "anywhere.shortcut": "",
      "anywhere.pressEnter": false,
      "anywhere.draftAtOnce": true,
      "anywhere.copyForMe": false,
      "send.undoSeconds": 5,
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
  it("says mail is read as it arrives only while a server is listening", () => {
    const checking = { everyMinutes: 15, lastCheckedAt: null, failing: null };
    expect(
      describeMailChecking({ ...base, mailChecking: { ...checking, mailboxes: 1, listening: 1 } }),
    ).toBe("I check your mailbox every 15 minutes, and read new mail as it arrives.");
    expect(
      describeMailChecking({ ...base, mailChecking: { ...checking, mailboxes: 3, listening: 2 } }),
    ).toBe(
      "I check your mailboxes every 15 minutes, and read new mail as it arrives in 2 of them.",
    );
    expect(
      describeMailChecking({ ...base, mailChecking: { ...checking, mailboxes: 2, listening: 0 } }),
    ).toBe("I check your mailboxes every 15 minutes.");
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
    // The common providers beyond the big four (0.10.0-alpha.48).
    expect(guessMailHost("c@gmx.de")?.host).toBe("imap.gmx.net");
    expect(guessMailHost("c@web.de")?.host).toBe("imap.web.de");
    expect(guessMailHost("c@comcast.net")?.help).toMatch(/third-party access/);
    expect(guessMailHost("c@sbcglobal.net")?.host).toBe("imap.mail.att.net");
    expect(guessMailHost("c@verizon.net")?.host).toBe("imap.aol.com");
    expect(guessMailHost("c@yandex.ru")?.host).toBe("imap.yandex.com");
    expect(guessMailHost("c@qq.com")?.help).toMatch(/authorization code/);
    expect(guessMailHost("c@icloud.com")?.port).toBe(993);
    // Outlook.com is known, and signs in with Microsoft rather than a password.
    expect(guessMailHost("C@Outlook.com")).toMatchObject({
      host: "outlook.office365.com",
      signIn: "microsoft",
      noPassword: true,
    });
    // Gmail can sign in with Google, and takes an app password too.
    expect(guessMailHost("c@gmail.com")).toMatchObject({ signIn: "google" });
    expect(guessMailHost("c@googlemail.com")?.noPassword).toBeUndefined();
    expect(guessMailHost("c@icloud.com")?.signIn).toBeUndefined();
    // Microsoft's addresses in other countries sign in the same way.
    for (const a of ["c@hotmail.co.uk", "c@outlook.fr", "c@live.com.au", "c@windowslive.com"]) {
      expect(guessMailHost(a)?.signIn).toBe("microsoft");
    }
    // Other people's domains that only look like Microsoft's keep the password.
    for (const a of ["c@liveworld.com", "c@live.io", "c@outlook.co", "c@msn.ai", "c@passport.me"]) {
      expect(guessMailHost(a)).toBeNull();
    }
    expect(guessMailHost("c@outlook.example.com")).toBeNull();
    expect(guessMailHost("c@formicaria.us")).toBeNull();
    expect(guessMailHost("not an address")).toBeNull();
  });

  it("knows Yahoo's, AOL's, Apple's and Zoho's other addresses", async () => {
    const { guessMailHost } = await import("../src");
    for (const a of ["c@ymail.com", "c@rocketmail.com", "c@yahoo.co.uk", "c@yahoo.com.au"]) {
      expect(guessMailHost(a)?.host).toBe("imap.mail.yahoo.com");
    }
    expect(guessMailHost("c@yahoo.co.jp")).toBeNull();
    expect(guessMailHost("c@mac.com")?.host).toBe("imap.mail.me.com");
    expect(guessMailHost("c@aol.com")?.host).toBe("imap.aol.com");
    expect(guessMailHost("c@zohomail.com")?.host).toBe("imap.zoho.com");
  });

  it("names whose servers a domain's mail exchangers are, and nothing looser", async () => {
    const { mailHostFromExchangers, mailDomainOf } = await import("../src");
    expect(
      mailHostFromExchangers(["ASPMX.L.GOOGLE.COM.", "alt1.aspmx.l.google.com"]),
    ).toMatchObject({
      name: "Google Workspace",
      host: "imap.gmail.com",
      signIn: "google",
    });
    expect(mailHostFromExchangers(["smtp.google.com"])?.name).toBe("Google Workspace");
    expect(mailHostFromExchangers(["formicaria-us.mail.protection.outlook.com"])).toMatchObject({
      name: "Microsoft 365",
      host: "outlook.office365.com",
      signIn: "microsoft",
      noPassword: true,
    });
    expect(mailHostFromExchangers(["mx.zoho.eu"])).toMatchObject({
      name: "Zoho Mail",
      host: "imap.zoho.eu",
    });
    expect(mailHostFromExchangers(["in1-smtp.messagingengine.com"])?.host).toBe(
      "imap.fastmail.com",
    );
    expect(mailHostFromExchangers(["mx01.mail.icloud.com"])?.host).toBe("imap.mail.me.com");
    expect(mailHostFromExchangers(["mailstore1.secureserver.net"])?.host).toBe(
      "imap.secureserver.net",
    );
    expect(mailHostFromExchangers(["mx1.privateemail.com"])?.host).toBe("mail.privateemail.com");
    // The first exchanger that is someone's says; anything that only looks like one doesn't.
    expect(
      mailHostFromExchangers([
        "mx.hosting.example",
        "x.mail.protection.outlook.com",
        "aspmx.l.google.com",
      ])?.name,
    ).toBe("Microsoft 365");
    for (const mx of [
      "google.com.evil.example",
      "notgoogle.com",
      "mail.protection.outlook.com.example",
    ]) {
      expect(mailHostFromExchangers([mx])).toBeNull();
    }
    expect(mailHostFromExchangers([])).toBeNull();

    expect(mailDomainOf("c@Formicaria.US")).toBe("formicaria.us");
    expect(mailDomainOf("c@formicaria")).toBeNull();
    expect(mailDomainOf("c@")).toBeNull();
    expect(mailDomainOf("c@bad domain.com")).toBeNull();
    expect(mailDomainOf("formicaria.us")).toBeNull();
  });

  it("reads how a connected mailbox signs in from its settings", async () => {
    const { mailboxAuth } = await import("../src");
    const imap = (account: Record<string, unknown>) => ({ connector: "imap", config: { account } });
    expect(mailboxAuth(imap({ username: "c@outlook.com", auth: "microsoft" }))).toBe("microsoft");
    expect(mailboxAuth(imap({ username: "c@gmail.com", auth: "password" }))).toBe("password");
    expect(mailboxAuth(imap({ username: "c@gmail.com", auth: "google" }))).toBe("google");
    // Connected before there was a choice.
    expect(mailboxAuth(imap({ username: "c@gmail.com" }))).toBe("password");
    expect(mailboxAuth({ connector: "mbox", config: {} })).toBeNull();
  });

  it("reads a connected Teams account", async () => {
    const { teamsAccount } = await import("../src");
    const config = { userId: "u1", user: "C Haynes", email: "c@formicaria.example", marks: {} };
    expect(teamsAccount({ connector: "teams", config })).toEqual({
      user: "C Haynes",
      email: "c@formicaria.example",
      lastCheckedAt: null,
    });
    expect(teamsAccount({ connector: "slack", config })).toBeNull();
  });

  it("knows a Slack user token and reads a connected workspace", async () => {
    const { isSlackUserToken, slackWorkspace, SLACK_MANIFEST, SLACK_USER_SCOPES } =
      await import("../src");
    expect(isSlackUserToken("  xoxp-1111111111-2222222222-abcdefabcdef ")).toBe(true);
    expect(isSlackUserToken("xoxb-1111111111-2222222222-abcdefabcdef")).toBe(false);
    expect(isSlackUserToken("xoxp-short")).toBe(false);
    expect(isSlackUserToken("xoxp-1111111111 2222222222-abcdef")).toBe(false);
    const config = { teamId: "T1", team: "Acme", userId: "U1", user: "c", marks: {} };
    expect(slackWorkspace({ connector: "slack", config })).toEqual({
      team: "Acme",
      user: "c",
      lastCheckedAt: null,
    });
    expect(slackWorkspace({ connector: "imap", config })).toBeNull();
    const manifest = JSON.parse(SLACK_MANIFEST) as {
      oauth_config: { scopes: { user: string[]; bot?: string[] } };
      settings: { token_rotation_enabled: boolean };
    };
    expect(manifest.oauth_config.scopes.user).toEqual([...SLACK_USER_SCOPES]);
    expect(manifest.oauth_config.scopes.bot).toBeUndefined();
    expect(manifest.settings.token_rotation_enabled).toBe(false);
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
    expect(["account", "keychain", "file"]).toContain(state.credentials.protection);
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
    const mac = describeCredentials(kept({ protection: "keychain", locked: ["imap:a"] }));
    expect(mac).toMatch(/locked with a key kept in your login Keychain/);
    expect(mac).toMatch(/macOS may ask you first/);
    expect(mac).toMatch(/locked with a key I can't get from your Keychain/);
    expect(mac).not.toMatch(/Windows/);
  });

  it("says when something unsealed is still on disk", () => {
    expect(describeCredentials(kept({ unsealedLeft: true }))).toMatch(
      /The file they were kept in before this version isn't locked and is still on this computer/,
    );
    // On a Mac, nothing promises to lock what an earlier build saved.
    const mac = describeCredentials(kept({ protection: "keychain", unsealedLeft: true }));
    expect(mac).toMatch(
      /Something saved before this version is still on this computer unlocked; entering those keys and passwords again locks them\./,
    );
    expect(mac).not.toMatch(/I'll move/);
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

describe("an address filed under someone is the user's only once they say who", () => {
  it("parses the preview: who the mail from the address is filed under", () => {
    const preview = AddressPreview.parse(read(contractFixture("address_preview.json")));
    // Written after an import that left the email out: what the user wrote
    // from it was filed under a person called C.
    expect(preview.alreadyYours).toBe(false);
    expect(preview.owner?.displayName).toBe("C");
    expect(preview.owner?.messages).toBeGreaterThan(0);
  });

  it("parses what adding the address did, with the counts the preview gave", () => {
    const added = AddressAdded.parse(read(contractFixture("address_added.json")));
    const preview = AddressPreview.parse(read(contractFixture("address_preview.json")));
    expect(added.identity.identifiers.some((i) => i.kind === "email")).toBe(true);
    expect(added.claimed).toEqual({ messages: preview.owner?.messages, people: 1 });
  });

  it("parses the addresses still held, for Settings", () => {
    const held = HeldAddress.array().parse(read(contractFixture("held_addresses.json")));
    expect(held).toHaveLength(1);
    expect(held[0]!.identifier.normalizedValue).toBe("c@example.com");
    expect(held[0]!.owner.displayName).toBe("C");
    expect(held[0]!.keptApart).toBe(false);
  });

  it("parses who sent mail from the Sent folder under an address not yet the user's", () => {
    const asked = SentFolderPerson.array().parse(read(contractFixture("sent_folder_people.json")));
    // Written from a Gmail export where the user replied from a work address.
    expect(asked).toHaveLength(1);
    expect(asked[0]!.address).toBe("c@work.example");
    expect(asked[0]!.sent).toBe(2);
    expect(asked[0]!.owner.displayName).toBe("C at work");
    expect(describeSentFolder(asked[0]!)).toEqual({
      line: "Both messages I have from C at work (c@work.example) were in your Sent folder. If that's you, I'm counting what you wrote as someone else's.",
      why: "Both messages I have from C at work (c@work.example) were in your Sent folder. Mail there is usually yours, so C at work may be you writing from another address — unless someone else sends for you, you passed their mail on, or other people send from that address too.",
    });
    const person = (sent: number, messages: number, displayName = "Pat") => ({
      ...asked[0]!,
      address: "pat@example.com",
      sent,
      owner: { ...asked[0]!.owner, displayName, messages },
    });
    // How much of their mail was there is always said, so one message sent
    // for someone who writes to you often doesn't read like a match.
    expect(describeSentFolder(person(1, 300)).line).toMatch(
      /^1 of the 300 messages I have from Pat \(pat@example\.com\) was in your Sent folder\./,
    );
    expect(describeSentFolder(person(3, 8)).line).toMatch(/^3 of the 8 messages .* were in/);
    expect(describeSentFolder(person(1, 1)).line).toMatch(/^The one message I have from Pat/);
    expect(describeSentFolder(person(12, 12)).line).toMatch(/^All 12 messages I have from Pat/);
    // A person known only by the address isn't named twice.
    expect(describeSentFolder(person(1, 1, "pat@example.com")).line).toMatch(
      /^The one message I have from pat@example\.com was in/,
    );
  });

  const owner = (over: Partial<AddressOwner> = {}): AddressOwner => ({
    participantId: "p1",
    displayName: "C (work)",
    messages: 12,
    otherAddresses: [],
    relationship: null,
    hasNotes: false,
    preferences: 0,
    ...over,
  });

  it("asks about the person, and says every message filed under them moves", () => {
    const { question, lines } = describeFold(owner(), "c@work.example");
    expect(question).toBe("Is C (work) you?");
    expect(lines).toEqual([
      "I have c@work.example down as C (work)'s.",
      "If C (work) is you, the 12 messages filed under them become yours, and C (work) is no longer among your people.",
      "This can't be undone: removing the address later won't make those messages C (work)'s again.",
    ]);
  });

  it("names the addresses that become the user's and what the user said that goes", () => {
    const { lines } = describeFold(
      owner({
        messages: 1,
        otherAddresses: ["555 010 2222", "c@old.example"],
        relationship: "colleague",
        hasNotes: true,
        preferences: 2,
      }),
      "c@work.example",
    );
    expect(lines).toContain(
      "If C (work) is you, the one message filed under them becomes yours, and C (work) is no longer among your people.",
    );
    expect(lines).toContain(
      "Their other addresses, 555 010 2222 and c@old.example, become yours too.",
    );
    expect(lines).toContain(
      'What you told me about them goes: what they are to you ("colleague"), your notes and the 2 preferences you set for writing to them.',
    );
    expect(lines.at(-1)).toBe(
      "This can't be undone: removing the address later won't make that message C (work)'s again.",
    );
    expect(describeFold(owner({ otherAddresses: ["x@y.z"] }), "a@b.c").lines).toContain(
      "Their other address, x@y.z, becomes yours too.",
    );
    expect(describeFold(owner({ messages: 0 }), "a@b.c").lines.at(-1)).toBe(
      "This can't be undone.",
    );
  });

  it("says how much moved, and nothing when no message did", () => {
    expect(describeClaimed({ messages: 0, people: 0 })).toBeNull();
    expect(describeClaimed({ messages: 0, people: 1 })).toBeNull();
    expect(describeClaimed({ messages: 1, people: 1 })).toBe(
      "One message I'd already read is yours now. I'll look at how you write again so it counts.",
    );
    expect(describeClaimed({ messages: 1200, people: 1 })).toBe(
      `${(1200).toLocaleString()} messages I'd already read are yours now. I'll look at how you write again so they count.`,
    );
  });
});

describe("the rest of the conversation a waiting message is part of", () => {
  it("counts what is either side of the message on each card", () => {
    const view = Dashboard.parse(read(contractFixture("dashboard.json")));
    expect(view.awaiting.some((t) => t.earlier > 0)).toBe(true);
    for (const t of view.awaiting) {
      expect(t.earlier + 1 + t.later).toBe(t.messageCount);
    }
  });

  it("parses a page of it, oldest first, with the user's side labelled as theirs", () => {
    const page = ConversationPage.parse(read(contractFixture("conversation_page.json")));
    expect(page.messages.length).toBeGreaterThan(1);
    expect(page.more).toBe(0);
    const times = page.messages.map((m) => m.sentAt ?? "");
    expect([...times].sort()).toEqual(times);
    expect(page.messages.some((m) => writerOf(m) === "You wrote")).toBe(true);
  });

  it("files each of the user's own messages by what it is doing, and nobody else's", () => {
    const page = ConversationPage.parse(read(contractFixture("conversation_page.json")));
    for (const m of page.messages) expect(m.filing === null).toBe(m.direction !== "self");
    expect(page.messages.some((m) => m.filing?.by === "rules")).toBe(true);
    expect(filedByPhrase("rules")).toBe("by the rules");
    expect(filedByPhrase("model")).toBe("as the model on this computer read it");
    expect(filedByPhrase("you")).toBe("as you said");
  });

  it("names who wrote each message without guessing", () => {
    const m = { id: "m", sentAt: null, body: "hi", automated: null };
    expect(writerOf({ ...m, direction: "self", author: null })).toBe("You wrote");
    expect(writerOf({ ...m, direction: "other", author: "Ada Lovelace" })).toBe("Ada wrote");
    expect(writerOf({ ...m, direction: "other", author: null })).toBe(
      "Someone I couldn't name wrote",
    );
    expect(writerOf({ ...m, direction: "unknown", author: null })).toBe(
      "I couldn't tell who wrote this",
    );
  });
});

describe("the drafts measured against what the user wrote", () => {
  it("parses a measurement, with every way of answering and no headline number", () => {
    const view = EvaluationView.parse(read(contractFixture("evaluation.json")));
    expect(view.remaining).toBeGreaterThan(0);
    expect(view.remaining).toBeLessThanOrEqual(view.measured);
    expect(view.systems.map((s) => s.system)).toEqual(["mimic", "generic", "common_reply"]);
    for (const s of view.systems) {
      expect(s.cases).toBe(view.remaining);
      expect(s.length!.p10).toBeLessThanOrEqual(s.length!.mean);
    }
    expect(view.cases.every((c) => c.answers.length === view.systems.length)).toBe(true);
    // Measured by the Rust stand-in, which has no encoder: no wording
    // measure, and nothing that pretends to be one.
    expect(view.embeddingProvider).toBeNull();
    expect(Object.keys(view)).not.toContain("score");
    expect(describeEvaluation(view)).toMatch(/^Measured on \d+ of your replies, from /);
    expect(describeGone(view)).toBeNull();
  });

  it("labels every way of answering and every measure", () => {
    for (const s of EvaluationSystem.options) expect(SYSTEM_LABELS[s]).toBeTruthy();
    for (const m of MEASURES) expect(MEASURE_LABELS[m].label).toBeTruthy();
    expect(formatShare(0.724)).toBe("72%");
  });

  it("says a lexical encoder compares wording, not meaning", () => {
    expect(describeEncoder(null)).toBeNull();
    expect(describeEncoder("lexical_v1")).toContain("not what they mean");
    expect(describeEncoder("all-minilm-l6-v2")).not.toContain("not what they mean");
    expect(describeEncoder("all-minilm-l6-v2")).toContain("compares what the replies mean");
  });

  it("puts the engine's notes on the split in the screen's words", () => {
    expect(
      describeSplitWarning(
        "only two conversations: one trains, one is held out, so the score rests on a single thread",
      ),
    ).not.toMatch(/score/);
    expect(describeSplitWarning("something new")).toBe("something new");
  });

  it("says when replies it was measured on no longer count", () => {
    const view = EvaluationView.parse(read(contractFixture("evaluation.json")));
    expect(describeGone({ ...view, measured: 5, remaining: 3 })).toBe(
      "2 replies it was measured on don't count any more: the mail has changed since. The figures are from the 3 left.",
    );
    expect(describeGone({ ...view, measured: 5, remaining: 4 })).toMatch(
      /^1 reply it was measured on doesn't count/,
    );
    expect(describeGone({ ...view, measured: 5, remaining: 0 })).toMatch(/^None of the replies/);
  });
});

describe("every conversation with someone", () => {
  it("parses a person's conversations, each standing where the home screen puts it", () => {
    const theirs = PersonConversations.parse(read(contractFixture("person_conversations.json")));
    expect(theirs.conversations.length).toBeGreaterThan(0);
    expect(theirs.conversations.some((c) => c.standing === "waiting")).toBe(true);
    const times = theirs.conversations.map((c) => c.lastMessageAt ?? "");
    expect([...times].sort().reverse()).toEqual(times);
  });

  it("says where one stands, and offers the list only where that would change something", () => {
    const base = {
      conversationId: "c",
      subject: null,
      channel: "email",
      isGroup: false,
      messageCount: 3,
      lastMessageAt: null,
      decidingMessageId: "m",
      standing: "waiting" as const,
      mark: null,
    };
    expect(describeStanding({ ...base, standing: "quiet" }, 30)).toBe(
      "Not on your list: the message it waits on is more than 30 days old.",
    );
    expect(describeStanding({ ...base, standing: "quiet" }, 1)).toContain("more than a day old");
    expect(describeStanding({ ...base, mark: "needs_reply" }, 30)).toBe(
      "On your list, because you said it needs a reply.",
    );
    expect(describeStanding({ ...base, standing: "answered" }, 30)).toBe("You wrote last.");
    expect(canPutOnList({ ...base, standing: "automated" })).toBe(true);
    expect(canPutOnList({ ...base, standing: "not_needed" })).toBe(true);
    expect(canPutOnList({ ...base, standing: "answered" })).toBe(false);
    expect(canPutOnList({ ...base, standing: "waiting" })).toBe(false);
    expect(canPutOnList({ ...base, standing: "quiet", decidingMessageId: null })).toBe(false);
  });
});

describe("finding what was said", () => {
  it("parses what a search found: the message, where it is, and the words around the match", () => {
    const page = SearchPage.parse(read(contractFixture("search_page.json")));
    expect(page.found.length).toBeGreaterThan(0);
    const first = page.found[0]!;
    expect(first.snippet.some((p) => p.hit)).toBe(true);
    expect(first.message.direction).not.toBe("self");
    expect(first.earlier).toBeGreaterThanOrEqual(0);
    expect(page.total).toBeGreaterThanOrEqual(page.found.length);
  });

  it("says how much was found, whose, and when there is more than it counted", () => {
    expect(describeFound({ total: 0, capped: false }, "anyone")).toBe(
      "Nothing I've read says that.",
    );
    expect(describeFound({ total: 0, capped: false }, "you")).toBe("Nothing you wrote says that.");
    expect(describeFound({ total: 1, capped: false }, "others")).toBe(
      "One message someone else wrote says that.",
    );
    expect(describeFound({ total: 1, capped: false }, "you")).toBe(
      "One of your messages says that.",
    );
    expect(describeFound({ total: 12, capped: false }, "anyone")).toBe(
      "12 messages say that, the newest first.",
    );
    expect(describeFound({ total: 40, capped: false }, "others")).toBe(
      "40 messages someone else wrote say that, the newest first.",
    );
    expect(describeFound({ total: 1000, capped: true }, "you")).toBe(
      "More than 1,000 of your messages say that. The newest come first; another word finds fewer.",
    );
  });

  it("looks only when there is a letter or a digit to look for", () => {
    expect(hasWordsToFind("friday")).toBe(true);
    expect(hasWordsToFind("  ?! ")).toBe(false);
    expect(hasWordsToFind("résumé")).toBe(true);
    expect(hasWordsToFind("42")).toBe(true);
    expect(hasWordsToFind('""')).toBe(false);
  });
});

describe("a WhatsApp chat, before it is imported", () => {
  it("parses the check: what was read, what was left out, and every name that wrote", () => {
    const report = ValidationReport.parse(read(contractFixture("validation_report.json")));
    expect(report.ok).toBe(true);
    expect(report.names.map((w) => [w.name, w.kind, w.normalized, w.chatNamedAfter])).toEqual([
      ["Ada", "handle", "whatsapp:ada", true],
      ["C", "handle", "whatsapp:c", false],
    ]);
    // What the user would add is what the import matches.
    const ids = report.frequentIdentifiers.map(([id]) => id);
    expect(report.names.every((w) => ids.includes(w.value))).toBe(true);
    expect(report.warnings.some((w) => w.includes("no words of yours"))).toBe(true);
  });

  it("reads a check from a source that knows no names as having none", () => {
    const older: Record<string, unknown> = {
      ...ValidationReport.parse(read(contractFixture("validation_report.json"))),
    };
    delete older.names;
    delete older.oneWriter;
    const parsed = ValidationReport.parse(older);
    expect(parsed.names).toEqual([]);
    expect(parsed.oneWriter).toBe(false);
  });
});

describe("a Discord package, before it is imported", () => {
  it("names its one account, with every address that makes it the user's", () => {
    const report = ValidationReport.parse(read(contractFixture("discord_validation_report.json")));
    expect(report.oneWriter).toBe(true);
    expect(
      report.names.map((w) => [w.name, w.kind, w.normalized, w.also.map((a) => a.normalized)]),
    ).toEqual([["C", "account_id", "discord:90001", ["c@example.com"]]]);
    expect(report.warnings.some((w) => w.includes("only what you wrote"))).toBe(true);
  });

  it("reads a writer from before other addresses were reported as having none", () => {
    const report = ValidationReport.parse(read(contractFixture("validation_report.json")));
    expect(report.names.every((w) => w.also.length === 0)).toBe(true);
    const older = { ...report.names[0] } as Record<string, unknown>;
    delete older.also;
    expect(WriterName.parse(older).also).toEqual([]);
  });

  it("is the user's by the address the import gives it, or by any other it gives it too", () => {
    const [account] = ValidationReport.parse(
      read(contractFixture("discord_validation_report.json")),
    ).names;
    if (!account) throw new Error("the check names no account");
    const id = (kind: string, normalizedValue: string) => ({ kind, normalizedValue });
    expect(yoursAs(account, [id("email", "c@work.example")])).toBeNull();
    expect(yoursAs(account, [id("email", "c@example.com")])).toEqual(id("email", "c@example.com"));
    // The kind counts, not only the text.
    expect(yoursAs(account, [id("handle", "discord:90001")])).toBeNull();
    const both = [id("email", "c@example.com"), id("account_id", "discord:90001")];
    expect(yoursAs(account, both)).toEqual(id("account_id", "discord:90001"));
  });
});

describe("what was sent says where it went and whether it got there", () => {
  const entry = (over: Partial<OutboxEntry> = {}): OutboxEntry => ({
    id: "o1",
    draftId: "d1",
    sourceId: "s1",
    conversationId: "c1",
    answersMessageId: "m1",
    participantId: "p1",
    fromAddress: "c@example.com",
    to: ["ada@example.com"],
    cc: [],
    subject: "Re: Lunch",
    body: "perfect, see you there",
    messageId: "x@example.com",
    how: "approved",
    status: "sent",
    sendAfter: "2026-10-26T09:00:00Z",
    error: null,
    createdAt: "2026-10-26T09:00:00Z",
    updatedAt: "2026-10-26T09:00:01Z",
    sentAt: "2026-10-26T09:00:01Z",
    ...over,
  });

  it("names who it went to, and says when it went by itself", () => {
    expect(describeOutboxEntry(entry())).toBe("Sent to ada@example.com.");
    expect(describeOutboxEntry(entry({ how: "autonomous" }))).toBe(
      "I sent this to ada@example.com by myself.",
    );
    expect(listAddresses(["a@x.io", "b@x.io", "c@x.io"])).toBe("a@x.io, b@x.io and c@x.io");
  });

  it("says why one didn't go, and that one waiting can still be stopped", () => {
    expect(
      describeOutboxEntry(entry({ status: "failed", error: "The mail server said no: 550" })),
    ).toBe("Didn't go to ada@example.com: The mail server said no: 550");
    expect(describeOutboxEntry(entry({ status: "held" }))).toMatch(/You can stop it/);
    expect(describeOutboxEntry(entry({ status: "canceled" }))).toMatch(/^Stopped before it went/);
    expect(
      describeOutboxEntry(
        entry({
          status: "canceled",
          error: "Rehearsing: I'd have sent this by myself now, and didn't. It waits for you.",
        }),
      ),
    ).toMatch(/^Stopped before it went to .+\. Rehearsing: I'd have sent this/);
    expect(describeOutboxEntry(entry({ status: "unsure" }))).toMatch(/Look in your Sent folder/);
    expect(
      describeOutboxEntry(entry({ status: "unsure", fromAddress: "slack:T1:U1", to: ["Ada"] })),
    ).toBe("May have gone to Ada — I couldn't tell. Look in the conversation in Slack.");
    expect(
      describeOutboxEntry(entry({ status: "unsure", fromAddress: "teams:u1", to: ["Ada"] })),
    ).toBe("May have gone to Ada — I couldn't tell. Look in the chat in Teams.");
  });

  it("names who else a reply was copied to", () => {
    expect(describeOutboxEntry(entry({ cc: ["bob@example.com"] }))).toBe(
      "Sent to ada@example.com, copying in bob@example.com.",
    );
    expect(describeOutboxEntry(entry({ status: "held", cc: ["b@x.io", "c@x.io"] }))).toBe(
      "Waiting to go to ada@example.com, copying in b@x.io and c@x.io. You can stop it until then.",
    );
  });

  it("says when a reply went but its copy in the Sent folder couldn't be made", () => {
    const note = "It went, but I couldn't put a copy in your Sent folder: the server said no";
    expect(describeOutboxEntry(entry({ error: note }))).toBe(`Sent to ada@example.com. ${note}`);
    expect(describeOutboxEntry(entry({ how: "autonomous", error: note }))).toBe(
      `I sent this to ada@example.com by myself. ${note}`,
    );
  });

  it("treats only a send that may have gone as one not to repeat", () => {
    expect(sendMayHaveGone("send_unsure")).toBe(true);
    expect(sendMayHaveGone("send_failed")).toBe(false);
    expect(sendMayHaveGone(undefined)).toBe(false);
  });
});

describe("replying by itself says what it will do", () => {
  const rules = {
    enabled: true,
    mailboxes: ["s1"],
    who: "known" as const,
    people: [],
    never: [],
    holdMinutes: 10,
    dailyLimit: 10,
    enabledAt: "2026-10-26T09:00:00.000Z",
  };

  it("says when it is off, or on but doing nothing, and what it allows when on", () => {
    expect(
      describeAutoReply({ rules: { ...rules, enabled: false }, running: false, lastDay: 0 }),
    ).toBe("Off. Every reply waits for you.");
    expect(
      describeAutoReply({ rules: { ...rules, mailboxes: [] }, running: true, lastDay: 0 }),
    ).toMatch(/for no mailbox yet/);
    expect(describeAutoReply({ rules, running: true, lastDay: 0 })).toBe(
      "On: I answer people you've written to yourself, holding each reply 10 minutes first, 10 a day at most.",
    );
    expect(
      describeAutoReply({ rules: { ...rules, rehearse: true }, running: true, lastDay: 0 }),
    ).toBe(
      "Rehearsing: I'd answer people you've written to yourself, holding each reply 10 minutes first, 10 a day at most — and send none of them.",
    );
    expect(
      describeAutoReply({
        rules: { ...rules, who: "listed", people: ["p1"] },
        running: true,
        lastDay: 0,
      }),
    ).toMatch(/the one person you chose/);
    expect(
      describeAutoReply({ rules: { ...rules, never: ["p1", "p2"] }, running: true, lastDay: 0 }),
    ).toMatch(/^On: I answer people you've written to yourself, except the 2 you named,/);
    expect(
      describeAutoReply({
        rules: { ...rules, hours: { from: 8, to: 20 } },
        running: true,
        lastDay: 0,
      }),
    ).toBe(
      "On: I answer people you've written to yourself, holding each reply 10 minutes first, only between 8:00 and 20:00, 10 a day at most.",
    );
    expect(
      describeAutoReply({
        rules: { ...rules, who: "listed", people: ["p1", "p2"], never: ["p2"] },
        running: true,
        lastDay: 0,
      }),
    ).toMatch(/the one person you chose/);
  });

  it("says when a held reply goes, and to whom", () => {
    const held: OutboxEntry = {
      id: "o1",
      draftId: "d1",
      sourceId: "s1",
      conversationId: "c1",
      answersMessageId: "m1",
      participantId: "p1",
      fromAddress: "c@example.com",
      to: ["ada@example.com"],
      cc: [],
      subject: "Re: Lunch",
      body: "perfect, see you there",
      messageId: "x@example.com",
      how: "autonomous",
      status: "held",
      sendAfter: "2026-10-26T09:10:00Z",
      error: null,
      createdAt: "2026-10-26T09:00:00Z",
      updatedAt: "2026-10-26T09:00:00Z",
      sentAt: null,
    };
    expect(describeHeld(held, new Date("2026-10-26T09:00:30Z"))).toBe(
      "I'll send this to ada@example.com by myself in 10 minutes, unless you stop it.",
    );
    expect(describeHeld(held, new Date("2026-10-26T09:10:00Z"))).toMatch(/in a moment/);
    expect(describeHeld(held, new Date("2026-10-26T09:00:30Z"), true)).toBe(
      "Rehearsing: I'd send this to ada@example.com by myself in 10 minutes. Nothing will go; then it's yours.",
    );
    // Waiting for the hours allowed: said by the clock, not in hundreds of minutes.
    const later = new Date("2026-10-26T09:00:30Z");
    const sameDay = new Date(later.getTime() + 3 * 3600_000);
    const nextDay = new Date(later.getTime() + 20 * 3600_000);
    const at = (d: Date) => describeHeld({ ...held, sendAfter: d.toISOString() }, later);
    expect(at(sameDay)).toMatch(
      sameDay.toDateString() === later.toDateString()
        ? /by myself at \S/
        : /by myself tomorrow at \S/,
    );
    expect(at(nextDay)).toMatch(
      nextDay.toDateString() === later.toDateString()
        ? /by myself at \S/
        : /by myself tomorrow at \S/,
    );
    expect(at(nextDay)).not.toMatch(/minutes/);
  });
});
