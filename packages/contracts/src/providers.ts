import { z } from "zod";

export const ProviderInfo = z.object({
  id: z.string(),
  displayName: z.string(),
  /** True when nothing leaves this machine. The UI must show this. */
  local: z.boolean(),
  model: z.string(),
  description: z.string(),
  requiresCredential: z.boolean(),
});
export type ProviderInfo = z.infer<typeof ProviderInfo>;

export const ProviderState = z.object({
  providers: z.array(ProviderInfo),
  active: z.string().nullable(),
  /** Which credential keys have a value. Never the values. */
  configuredSecrets: z.array(z.string()),
});
export type ProviderState = z.infer<typeof ProviderState>;

export const ANTHROPIC_SECRET_KEY = "provider.anthropic.apiKey";

/**
 * What is true about the model on this computer, and the one thing to do next.
 *
 * The step is computed in the core and sent here rather than being worked out
 * on screen, so the UI cannot invent a fourth situation or disagree with the
 * command that acts on it.
 */
export const LocalModelStatus = z.object({
  endpoint: z.string(),
  hostReachable: z.boolean(),
  models: z.array(z.string()),
  wanted: z.string(),
  wantedPresent: z.boolean(),
  error: z.string().nullable(),
  nextStep: z.enum(["ready", "getTheHost", "pullModel"]),
  downloadPage: z.string(),
});
export type LocalModelStatus = z.infer<typeof LocalModelStatus>;
