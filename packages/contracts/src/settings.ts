import { z } from "zod";

export const Settings = z.object({
  "general.theme": z.enum(["dark", "light"]),
  "performance.workerConcurrency": z.number(),
  "privacy.networkFeatures": z.boolean(),
  "updates.channel": z.enum(["stable", "beta"]),
  "updates.automatic": z.boolean(),
  "diagnostics.includePaths": z.boolean(),
  "generation.provider": z.string(),
  "generation.localUrl": z.string(),
  "generation.localModel": z.string(),
  "generation.anthropicModel": z.string(),
  "onboarding.completed": z.boolean(),
});
export type Settings = z.infer<typeof Settings>;
export type SettingKey = keyof Settings;
