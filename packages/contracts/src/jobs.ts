import { z } from "zod";

export const JobStatus = z.enum([
  "queued",
  "running",
  "paused",
  "completed",
  "failed",
  "canceled",
  "interrupted",
]);
export type JobStatus = z.infer<typeof JobStatus>;

export const JobError = z.object({ code: z.string(), message: z.string() }).passthrough();

export const Job = z.object({
  id: z.string(),
  type: z.string(),
  status: JobStatus,
  payload: z.record(z.string(), z.unknown()).or(z.null()),
  progressCurrent: z.number(),
  progressTotal: z.number(),
  phase: z.string().nullable(),
  resumable: z.boolean(),
  createdAt: z.string(),
  startedAt: z.string().nullable(),
  heartbeatAt: z.string().nullable(),
  completedAt: z.string().nullable(),
  result: z.unknown().nullable(),
  error: JobError.nullable(),
});
export type Job = z.infer<typeof Job>;

export const JobEvent = z.object({
  jobId: z.string(),
  type: z.string(),
  status: z.string(),
  phase: z.string().nullable(),
  progressCurrent: z.number(),
  progressTotal: z.number(),
  message: z.string().nullable(),
});
export type JobEvent = z.infer<typeof JobEvent>;

export const JOB_KINDS = {
  scanLibrary: "scan_library",
  ingestLightroom: "ingest_lightroom",
  analyzeLibrary: "analyze_library",
  trainStyle: "train_style",
} as const;

export const JOB_LABELS: Record<string, string> = {
  scan_library: "Scanning folder library",
  ingest_lightroom: "Capturing from Lightroom",
  analyze_library: "Analyzing photos",
  train_style: "Training Style Brain",
};
