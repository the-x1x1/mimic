/**
 * Wire schemas for the Lightroom plugin <-> desktop bridge. Kept in sync with
 * mimic-core::bridge::protocol and asserted against fixtures/bridge/*.json.
 */
import { z } from "zod";
import { CapabilityProbe } from "./lightroom";

export const BRIDGE_PROTOCOL_VERSION = 1;

export const CommandType = z.enum([
  "ping",
  "get_catalog_info",
  "get_selected_photos",
  "get_photo_metadata",
  "get_develop_settings",
  "create_before_snapshot",
  "apply_settings_as_plugin_preset",
  "read_back_develop_settings",
  "collect_correction_state",
  "get_capabilities",
]);
export type CommandType = z.infer<typeof CommandType>;

export const HandshakeRequest = z.object({
  protocolVersion: z.number(),
  pluginVersion: z.string(),
  lightroomVersion: z.string(),
  sdkVersion: z.string().nullish(),
  catalogFingerprint: z.string(),
  catalogName: z.string().nullish(),
  capabilities: CapabilityProbe,
});

export const HandshakeResponse = z.object({
  ok: z.boolean(),
  protocolVersion: z.number(),
  appVersion: z.string(),
  sessionId: z.string(),
  pollIntervalMs: z.number(),
  maxBatchSize: z.number(),
  accepted: z.boolean(),
  reason: z.string().optional(),
});

export const CommandEnvelope = z.object({
  commandId: z.string(),
  commandType: CommandType,
  payload: z.unknown(),
});

export const BridgeErrorBody = z.object({
  code: z.string(),
  message: z.string(),
  details: z.unknown().optional(),
});

export const CommandResultBody = z.object({
  commandId: z.string(),
  ok: z.boolean(),
  result: z.unknown().optional(),
  error: BridgeErrorBody.optional(),
});

export const ApplyItemResult = z.object({
  photoId: z.number(),
  predictionId: z.string(),
  status: z.enum(["applied", "failed", "skipped"]),
  snapshotName: z.string().nullish(),
  before: z.record(z.string(), z.unknown()).nullish(),
  readBack: z.record(z.string(), z.unknown()).nullish(),
  error: BridgeErrorBody.nullish(),
});

export const ApplyBatchResult = z.object({
  items: z.array(ApplyItemResult),
  canceled: z.boolean().default(false),
});

export const EventsBody = z.object({
  events: z.array(z.object({ type: z.string(), at: z.string().nullish(), payload: z.unknown() })),
});
