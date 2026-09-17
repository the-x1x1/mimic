import { z } from "zod";

export const UpdateState = z.object({
  currentVersion: z.string(),
  latestSeenVersion: z.string().nullable(),
  stagedVersion: z.string().nullable(),
  channel: z.string(),
  lastCheckedAt: z.string().nullable(),
  lastUpdateResult: z.string().nullable(),
  updateError: z.string().nullable(),
});
export type UpdateState = z.infer<typeof UpdateState>;

export const InstallGuard = z.object({
  allowed: z.boolean(),
  reason: z.string(),
  activeJobs: z.number(),
});
export type InstallGuard = z.infer<typeof InstallGuard>;

/** Tauri updater `latest.json` shape (validated by scripts/verify-release.ps1 and tests). */
export const LatestJson = z.object({
  version: z.string(),
  notes: z.string().optional(),
  pub_date: z.string().optional(),
  platforms: z.record(
    z.string(),
    z.object({ signature: z.string().min(1), url: z.string().url() }),
  ),
});
export type LatestJson = z.infer<typeof LatestJson>;

/** Background check cadence: 6h ± up to 20 min jitter (spec §24.3). */
export function nextCheckDelayMs(random: () => number = Math.random): number {
  const base = 6 * 60 * 60 * 1000;
  const jitter = Math.floor((random() * 2 - 1) * 20 * 60 * 1000);
  return base + jitter;
}
