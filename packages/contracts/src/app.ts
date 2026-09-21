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
 * What the dashboard says about itself, in one sentence. Kept here so the
 * screen cannot quietly start implying that messages arrive on their own:
 * nothing does until a connector exists, and this sentence says so.
 */
export function describeFeed(d: Dashboard): string {
  if (d.lastImportAt === null) {
    return "Nothing imported yet. Mimic reads exports you point it at; it is not connected to a mailbox.";
  }
  if (d.awaitingTotal === 0) {
    return "Nothing is waiting on you in what has been imported.";
  }
  const shown =
    d.awaiting.length < d.awaitingTotal ? ` Showing the ${d.awaiting.length} most recent.` : "";
  const threads = d.awaitingTotal === 1 ? "1 thread" : `${d.awaitingTotal} threads`;
  return `${threads} ended with a message from someone else.${shown}`;
}
