import { z } from "zod";
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
