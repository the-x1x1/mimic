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
