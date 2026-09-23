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
  /** Minutes between checks of a connected mailbox; 0 is off. */
  "mail.checkEveryMinutes": z.number(),
  /**
   * How many days back a thread can be waiting on a reply; 0 is any age.
   * Older ones are left out as gone quiet, counted, and shown on request.
   */
  "waiting.withinDays": z.number(),
  "onboarding.completed": z.boolean(),
});
export type Settings = z.infer<typeof Settings>;
export type SettingKey = keyof Settings;

/** The waiting windows Settings offers, in days; 0 is any age. */
export const WAITING_WINDOWS = [7, 14, 30, 90, 0] as const;

/** A waiting window as the end of "the last message is from …". */
export function describeWaitingWindow(days: number): string {
  if (days <= 0) return "any time";
  if (days === 1) return "the last day";
  if (days === 7) return "the last week";
  if (days === 14) return "the last two weeks";
  return `the last ${days.toLocaleString()} days`;
}
