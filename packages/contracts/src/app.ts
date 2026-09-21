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
  participant: Participant.nullable(),
  hasRelationshipProfile: z.boolean(),
  draft: Draft.nullable(),
});
export type DashboardThread = z.infer<typeof DashboardThread>;

export const Dashboard = z.object({
  people: z.number(),
  conversations: z.number(),
  messages: z.number(),
  ownMessages: z.number(),
  awaiting: z.array(DashboardThread),
  awaitingTotal: z.number(),
  pendingDrafts: z.array(Draft),
  outcomes: DraftOutcomes,
  /** When the last import finished. Null before the first one. */
  lastImportAt: z.string().nullable(),
  /** Whether Mimic prepares replies without being asked. Off by default. */
  autoDraft: z.boolean(),
});
export type Dashboard = z.infer<typeof Dashboard>;

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
 * must not do is imply that mail arrives on its own: nothing does until there
 * is a connector, and the first branch below says so plainly.
 *
 * Whether it counts people or conversations is decided by what is actually
 * waiting rather than by which word sounds friendlier — a group thread is not
 * a person, and an unattributed one is nobody.
 */
export function describeWaiting(d: Dashboard): string {
  if (d.lastImportAt === null) {
    return "I haven't read any of your mail yet, so there's nothing here. Point me at it and this fills up.";
  }
  if (d.awaitingTotal === 0) {
    return "You're all caught up. Nobody is waiting on a reply.";
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
