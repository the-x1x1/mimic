import { z } from "zod";

export const Settings = z.object({
  "general.theme": z.enum(["dark", "light"]),
  "performance.workerConcurrency": z.number(),
  "performance.accelerator": z.string(),
  "performance.previewCacheMaxMb": z.number(),
  "performance.inferenceBatchSize": z.number(),
  "privacy.networkFeatures": z.boolean(),
  "updates.channel": z.enum(["stable", "beta"]),
  "updates.automatic": z.boolean(),
  "diagnostics.includePaths": z.boolean(),
  "review.highThreshold": z.number(),
  "review.mediumThreshold": z.number(),
  "onboarding.completed": z.boolean(),
});
export type Settings = z.infer<typeof Settings>;
export type SettingKey = keyof Settings;
