import { z } from "zod";
import { Participant } from "./identity";
import { Draft, DraftOutcomes } from "./compose";
import { Job } from "./jobs";
import { UpdateState } from "./updater";

export const AppInfo = z.object({
  version: z.string(),
  engineProtocolVersion: z.number(),
  analysisVersion: z.string(),
  dataRoot: z.string(),
  startedAt: z.string(),
  schemaVersion: z.number(),
  devMode: z.boolean(),
  demoMode: z.boolean(),
  os: z.string(),
});
export type AppInfo = z.infer<typeof AppInfo>;

export const EngineStatus = z.object({
  state: z.enum(["stopped", "starting", "ready", "failed"]),
  pid: z.number().nullable(),
  engineVersion: z.string().nullable(),
  protocolVersion: z.number().nullable(),
  pythonVersion: z.string().nullable(),
  accelerator: z.string().nullable(),
  restarts: z.number(),
  lastError: z.string().nullable(),
  capabilities: z.unknown(),
});
export type EngineStatus = z.infer<typeof EngineStatus>;

export const EngineEvent = z.discriminatedUnion("event", [
  z.object({
    event: z.literal("job.progress"),
    jobId: z.string(),
    phase: z.string(),
    current: z.number(),
    total: z.number(),
    message: z.string().nullish(),
  }),
  z.object({ event: z.literal("log"), level: z.string(), message: z.string() }),
  z.object({ event: z.literal("engine.exited"), code: z.number().nullable() }),
]);
export type EngineEvent = z.infer<typeof EngineEvent>;

/**
 * Each step is a fact about the database, not a checkbox the UI ticks. A user
 * who deletes everything goes back to step one, which is correct.
 */
export const OnboardingState = z.object({
  completed: z.boolean(),
  hasIdentity: z.boolean(),
  hasSource: z.boolean(),
  hasOwnMessages: z.boolean(),
  hasVoiceProfile: z.boolean(),
});
export type OnboardingState = z.infer<typeof OnboardingState>;

/** The first unfinished step, or null when there is nothing left to do. */
export function nextOnboardingStep(
  s: OnboardingState,
): "identity" | "source" | "import" | "analyze" | null {
  if (!s.hasIdentity) return "identity";
  if (!s.hasSource) return "source";
  if (!s.hasOwnMessages) return "import";
  if (!s.hasVoiceProfile) return "analyze";
  return null;
}

/**
 * Whether the user can leave onboarding and use the app, which is a weaker
 * condition than having finished every step. Someone whose mailbox holds
 * fifteen of their own messages will never get a measurable profile, and
 * refusing to let them in would be a permanent lock-out rather than a
 * standard. Compose already says plainly when it is drafting without a
 * measured style.
 */
export function canFinishOnboarding(s: OnboardingState): boolean {
  return s.hasIdentity && s.hasSource && s.hasOwnMessages;
}

/**
 * Whether the user can leave onboarding to look around, which is weaker again
 * than `canFinishOnboarding`: nothing has been imported, so the app will be
 * empty. It is allowed anyway. Exporting a mailbox is a real piece of work,
 * and an app that will not open its own front door until you have done it is
 * one nobody can evaluate before committing to it. Every screen already has an
 * honest empty state, and the dashboard says in as many words that nothing has
 * been imported and that Mimic is not connected to a mailbox.
 *
 * Identity is the one thing that stays mandatory, and not as a formality: the
 * importer decides a message's `direction` by matching it against the
 * identifiers declared here, so an import run before then attributes nothing
 * to anyone and quietly produces a corpus with no evidence of the user's
 * writing in it. One field is a fair price for not having that happen
 * silently.
 */
export function canLeaveOnboarding(s: OnboardingState): boolean {
  return s.hasIdentity;
}

/**
 * What the import step should show. The dead case is the middle one: an import
 * that read the file, attributed nothing to the user, and left the step
 * unfinished with no button on it. That happens when the declared identifiers
 * do not match the addresses in the export, which is the most likely mistake
 * on a first run, so it gets a named state and a way out.
 */
export function importStepState(
  sources: Array<{ status: string; messageCount: number }>,
): "no-source" | "not-started" | "running" | "failed" | "none-of-yours" {
  if (sources.length === 0) return "no-source";
  if (sources.some((s) => s.status === "importing")) return "running";
  if (sources.every((s) => s.status === "failed")) return "failed";
  if (sources.some((s) => s.status !== "imported" && s.status !== "failed")) return "not-started";
  return "none-of-yours";
}

export const EventRow = z.object({
  id: z.number(),
  level: z.string(),
  category: z.string(),
  eventType: z.string(),
  entityType: z.string().nullable(),
  entityId: z.string().nullable(),
  payloadJson: z.string(),
  createdAt: z.string(),
});
export type EventRow = z.infer<typeof EventRow>;

export const DiagnosticsBundle = z.object({
  generatedAt: z.string(),
  appVersion: z.string(),
  os: z.string(),
  arch: z.string(),
  dbSchemaVersion: z.number(),
  tableCounts: z.array(z.tuple([z.string(), z.number()])),
  engine: z.unknown(),
  providers: z.unknown(),
  updateState: z.unknown(),
  recentErrors: z.array(z.unknown()),
  jobSummaries: z.array(z.unknown()),
  includePaths: z.boolean(),
});
export type DiagnosticsBundle = z.infer<typeof DiagnosticsBundle>;

export const SystemStatus = z.object({
  engine: EngineStatus,
  activeJobs: z.array(Job),
  update: UpdateState,
  counts: z.object({
    sources: z.number(),
    messages: z.number(),
    ownMessages: z.number(),
    people: z.number(),
    drafts: z.number(),
  }),
});
export type SystemStatus = z.infer<typeof SystemStatus>;

export const CommandError = z.object({ code: z.string(), message: z.string() });
export type CommandError = z.infer<typeof CommandError>;

// ------------------------------------------------------------- dashboard

/**
 * Why a message looks automated, read from its headers when it was imported:
 * `newsletter`, `bulk`, `auto_reply`, `report` or `no_reply_address`
 * (`sources::automated` in Rust). A string rather than an enum so that one
 * unfamiliar reason cannot stop the whole home screen from parsing;
 * `describeAutomated` words the ones it knows and says something true about
 * the rest.
 */
export const AutomatedReason = z.string();

/**
 * One message of a conversation as the screen shows it. Whether the user
 * wrote it is decided at import, by their addresses; nothing here guesses.
 */
/**
 * Who filed one of the user's messages under what it is doing: the rules, a
 * model on this computer, or the user — each later one over the earlier.
 */
export const FiledBy = z.enum(["rules", "model", "you"]);
export type FiledBy = z.infer<typeof FiledBy>;

/** What one of the user's messages is filed under as doing (situation ids, strongest first), and by whom. */
export const Filing = z.object({
  by: FiledBy,
  situations: z.array(z.string()),
});
export type Filing = z.infer<typeof Filing>;

/** How a filing came about, as the line under the message says it. */
export function filedByPhrase(by: FiledBy): string {
  if (by === "you") return "as you said";
  if (by === "model") return "as the model on this computer read it";
  return "by the rules";
}

export const ThreadMessage = z.object({
  id: z.string(),
  direction: z.enum(["self", "other", "unknown"]),
  /** Who wrote it, when it was not the user and they could be named. */
  author: z.string().nullable(),
  sentAt: z.string().nullable(),
  body: z.string(),
  /** Why it looks automated, from its headers, when it does. A reading. */
  automated: AutomatedReason.nullable(),
  /** For the user's own messages, what it is filed under as doing; null for anyone else's. */
  filing: Filing.nullable(),
});
export type ThreadMessage = z.infer<typeof ThreadMessage>;

/**
 * Part of a conversation, oldest first, read from one of its messages toward
 * the start or the end, and how many messages are further that way.
 */
export const ConversationPage = z.object({
  messages: z.array(ThreadMessage),
  more: z.number(),
});
export type ConversationPage = z.infer<typeof ConversationPage>;

/** Which way from one of its messages a conversation is read. */
export const Toward = z.enum(["earlier", "later"]);
export type Toward = z.infer<typeof Toward>;

/** Who wrote a message of a conversation, as its label says it. */
export function writerOf(m: Pick<ThreadMessage, "direction" | "author">): string {
  if (m.direction === "self") return "You wrote";
  if (m.direction === "unknown") return "I couldn't tell who wrote this";
  return `${m.author ? m.author.split(" ")[0] : "Someone I couldn't name"} wrote`;
}

/** What the user said about whether a thread needs a reply. */
export const ThreadMark = z.enum(["no_reply_needed", "needs_reply"]);
export type ThreadMark = z.infer<typeof ThreadMark>;

/**
 * Where a conversation stands, by the same reading as the home screen: the
 * user wrote last, it is waiting, it was left off the list and why, or
 * nothing in it could be told to be anyone's.
 */
export const Standing = z.enum([
  "answered",
  "waiting",
  "automated",
  "quiet",
  "not_needed",
  "undecided",
]);
export type Standing = z.infer<typeof Standing>;

/** One conversation someone is in, and where it stands. */
export const PersonConversation = z.object({
  conversationId: z.string(),
  subject: z.string().nullable(),
  channel: z.string(),
  isGroup: z.boolean(),
  messageCount: z.number(),
  lastMessageAt: z.string().nullable(),
  /** The message that decides where it stands, which a mark is about. */
  decidingMessageId: z.string().nullable(),
  standing: Standing,
  mark: ThreadMark.nullable(),
});
export type PersonConversation = z.infer<typeof PersonConversation>;

/** A page of someone's conversations, most recently active first. */
export const PersonConversations = z.object({
  conversations: z.array(PersonConversation),
  /** How many more there are after these. */
  more: z.number(),
  /** The waiting window they were judged against; null for any age. */
  waitingWithinDays: z.number().nullable(),
});
export type PersonConversations = z.infer<typeof PersonConversations>;

/** Where a conversation stands, as one line. */
export function describeStanding(c: PersonConversation, withinDays: number | null): string {
  switch (c.standing) {
    case "answered":
      return "You wrote last.";
    case "waiting":
      return c.mark === "needs_reply"
        ? "On your list, because you said it needs a reply."
        : "On your list: waiting on you.";
    case "automated":
      return "Not on your list: its last message looks automated.";
    case "quiet":
      return `Not on your list: the message it waits on is ${olderThan(withinDays)}.`;
    case "not_needed":
      return "Not on your list: you said it needs no reply.";
    case "undecided":
      return "I couldn't tell who wrote what in it, so it isn't on your list.";
  }
}

/**
 * Whether saying a conversation needs a reply would put it on the list: only
 * one left off it, with a message to say it about.
 */
export function canPutOnList(c: PersonConversation): boolean {
  return (
    (c.standing === "automated" || c.standing === "quiet" || c.standing === "not_needed") &&
    c.decidingMessageId !== null
  );
}

/**
 * One row of the home screen: a conversation whose last message came from
 * someone else and was never answered, plus the draft Mimic has for it, if it
 * has one. `draft` is null until a draft actually exists — there is no
 * placeholder state.
 */
export const DashboardThread = z.object({
  conversationId: z.string(),
  channel: z.string(),
  subject: z.string().nullable(),
  isGroup: z.boolean(),
  messageCount: z.number(),
  lastMessage: z.string(),
  lastMessageAt: z.string().nullable(),
  lastMessageId: z.string(),
  /**
   * Messages of the conversation before the one on screen, and after it.
   * Those after it do not decide whether it is waiting — they look automated,
   * or who wrote them could not be told — and are counted so they can be shown.
   */
  earlier: z.number(),
  later: z.number(),
  participant: Participant.nullable(),
  hasRelationshipProfile: z.boolean(),
  /** A draft written for `lastMessage`, never one written for an earlier message. */
  draft: Draft.nullable(),
  /** Why the last message looks automated, when it does. A reading, not a fact. */
  automated: AutomatedReason.nullable(),
  /** What the user said about this thread, while it still applies. */
  mark: ThreadMark.nullable(),
  /**
   * Its last message is older than the waiting window (`waitingWithinDays`).
   * On a thread that is waiting, only because the user said it needs a reply.
   */
  quiet: z.boolean(),
});
export type DashboardThread = z.infer<typeof DashboardThread>;

export const Dashboard = z.object({
  people: z.number(),
  conversations: z.number(),
  messages: z.number(),
  ownMessages: z.number(),
  awaiting: z.array(DashboardThread),
  awaitingTotal: z.number(),
  /**
   * Unanswered threads not in `awaiting`, counted by why. Counts, never
   * estimates. One that looks automated is counted there even if it is also
   * old.
   */
  leftOut: z.object({ automated: z.number(), notNeeded: z.number(), quiet: z.number() }),
  /** Those threads, only when they were asked for. */
  leftOutThreads: z.array(DashboardThread),
  /** Whether they were asked for: empty and not asked for are different answers. */
  showingLeftOut: z.boolean(),
  /**
   * How many days back a thread can be waiting; null for any age. Older ones
   * are left out as `quiet` unless the user said they need a reply.
   */
  waitingWithinDays: z.number().nullable(),
  pendingDrafts: z.array(Draft),
  outcomes: DraftOutcomes,
  /** When the last import finished. Null before the first one. */
  lastImportAt: z.string().nullable(),
  /** Whether Mimic prepares replies without being asked. Off by default. */
  autoDraft: z.boolean(),
  /** Null unless a mailbox is connected. `everyMinutes` 0 means checking is off. */
  mailChecking: z
    .object({
      mailboxes: z.number(),
      everyMinutes: z.number(),
      lastCheckedAt: z.string().nullable(),
      /** Why the last check failed, when it did. */
      failing: z.string().nullable(),
    })
    .nullable(),
});
export type Dashboard = z.infer<typeof Dashboard>;

/** Whether mail comes in by itself, in one sentence, or null when it does not. */
export function describeMailChecking(d: Dashboard): string | null {
  const m = d.mailChecking;
  if (!m) return null;
  const which = m.mailboxes === 1 ? "your mailbox" : "your mailboxes";
  if (m.failing) {
    return `The last check of ${which} failed: ${m.failing}`;
  }
  if (m.everyMinutes === 0) {
    return `Checking ${which} is turned off, so nothing new comes in until you ask.`;
  }
  return `I check ${which} every ${m.everyMinutes} minutes.`;
}

/**
 * Why Mimic thinks a message was sent by a machine, as a clause that follows
 * "It looks automated to me:". Worded as what the headers say, because that is
 * all Mimic knows — it never read the words to decide.
 */
export function describeAutomated(reason: string | null): string | null {
  switch (reason) {
    case null:
      return null;
    case "newsletter":
      return "it came through a mailing list with nowhere to reply to the list.";
    case "bulk":
      return "the sender marked it as junk mail.";
    case "auto_reply":
      return "it was sent automatically, like an out-of-office reply.";
    case "report":
      return "it's a delivery report, like a bounce or a read receipt.";
    case "no_reply_address":
      return "it came from an address that says not to reply to it.";
    default:
      return "its headers say a machine sent it.";
  }
}

/** Every unanswered thread that is not on the waiting list, for whatever reason. */
export function leftOutTotal(d: Dashboard): number {
  return d.leftOut.automated + d.leftOut.notNeeded + d.leftOut.quiet;
}

/**
 * How old a quiet thread's last message is, as "more than 30 days old", or the
 * same without a number when there is none.
 */
export function olderThan(days: number | null): string {
  if (days === null) return "older than the window you chose";
  return days === 1 ? "more than a day old" : `more than ${days.toLocaleString()} days old`;
}

/**
 * Why a thread that has gone quiet was left out, for the thread itself.
 * Mimic's reading of a date, worded as one: nobody said nobody is waiting.
 */
export function describeQuiet(days: number | null): string {
  return `Its last message is ${olderThan(days)}, so I've taken it that nobody is still waiting on a reply.`;
}

/** "a, b and c", "a and b", or "a". */
function listOf(parts: string[]): string {
  if (parts.length <= 1) return parts.join("");
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

/**
 * What was left out of the waiting list and why, in one sentence, or null when
 * nothing was. Said every time something is left out, so leaving a thread out
 * never quietly hides it.
 */
export function describeLeftOut(d: Dashboard): string | null {
  const { automated, notNeeded, quiet } = d.leftOut;
  if (automated === 0 && notNeeded === 0 && quiet === 0) return null;
  const parts: string[] = [];
  if (automated === 1) parts.push("one thread that looks automated");
  if (automated > 1) {
    parts.push(
      `${automated.toLocaleString()} threads that look automated (newsletters, notifications and the like)`,
    );
  }
  const age = olderThan(d.waitingWithinDays);
  if (quiet === 1) parts.push(`one thread whose last message is ${age}`);
  if (quiet > 1) parts.push(`${quiet.toLocaleString()} threads whose last message is ${age}`);
  if (notNeeded === 1) parts.push("one you said doesn't need a reply");
  if (notNeeded > 1) parts.push(`${notNeeded.toLocaleString()} you said don't need a reply`);
  return `I left out ${listOf(parts)}.`;
}

export const AssistSummary = z.object({
  considered: z.number(),
  drafted: z.number(),
  failed: z.number(),
  disabled: z.boolean(),
});
export type AssistSummary = z.infer<typeof AssistSummary>;

/**
 * The one sentence at the top of the home screen, in Mimic's own voice.
 *
 * Mimic speaks in the first person here and everywhere else a person reads it,
 * because "Mimic reads exports you point it at" is a sentence about a program
 * and "I haven't read any of your mail yet" is a sentence to a person. What it
 * must not do is imply that mail arrives on its own when no mailbox is
 * connected; `describeMailChecking` is the only thing that may say it does.
 *
 * Whether it counts people or conversations is decided by what is actually
 * waiting rather than by which word sounds friendlier — a group thread is not
 * a person, and an unattributed one is nobody.
 */
export function describeWaiting(d: Dashboard): string {
  if (d.lastImportAt === null) {
    return d.mailChecking && !d.mailChecking.failing
      ? "I'm reading your mail for the first time. This fills up as I go."
      : "I haven't read any of your mail yet, so there's nothing here. Point me at it and this fills up.";
  }
  if (d.awaitingTotal === 0) {
    // With something left out, that nobody is waiting is a reading — of
    // headers, dates and what the user said — and is worded as one.
    return leftOutTotal(d) > 0
      ? "Nothing I've read looks like it's waiting on you."
      : "You're all caught up. Nobody is waiting on a reply.";
  }
  const everyoneIsAPerson = d.awaiting.every((t) => t.participant !== null && !t.isGroup);
  const noun = everyoneIsAPerson
    ? d.awaitingTotal === 1
      ? "person is"
      : "people are"
    : d.awaitingTotal === 1
      ? "conversation is"
      : "conversations are";
  const count = d.awaitingTotal === 1 ? "One" : String(d.awaitingTotal);
  const shown =
    d.awaiting.length < d.awaitingTotal ? ` Here are the ${d.awaiting.length} most recent.` : "";
  return `${count} ${noun} waiting on you.${shown}`;
}
