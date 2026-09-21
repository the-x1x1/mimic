import { z } from "zod";

/**
 * Named for what they look like, not for a brightness. An install from 0.8.0
 * or earlier stored "dark" or "light"; migration 0006 carries those over, so
 * nothing outside it needs to know they existed.
 */
export const Theme = z.enum(["plain", "paper", "night"]);
export type Theme = z.infer<typeof Theme>;

export const Settings = z.object({
  "general.theme": Theme,
  "performance.workerConcurrency": z.number(),
  "privacy.networkFeatures": z.boolean(),
  "updates.channel": z.enum(["stable", "beta"]),
  "updates.automatic": z.boolean(),
  "diagnostics.includePaths": z.boolean(),
  "generation.provider": z.string(),
  "generation.localUrl": z.string(),
  "generation.localModel": z.string(),
  "generation.anthropicModel": z.string(),
  /** Prepare replies without being asked. Off unless the user turns it on. */
  "assist.autoDraft": z.boolean(),
  "onboarding.completed": z.boolean(),
});
export type Settings = z.infer<typeof Settings>;
export type SettingKey = keyof Settings;
