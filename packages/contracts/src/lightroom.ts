import { z } from "zod";

export const SupportFlags = z.object({
  getDevelopSettings: z.boolean(),
  applyDevelopPreset: z.boolean(),
  addDevelopPresetForPlugin: z.boolean(),
  createDevelopSnapshot: z.boolean(),
  developController: z.boolean(),
  catalogWriteAccess: z.boolean(),
  lrHttp: z.boolean(),
});

export const CapabilityProbe = z.object({
  lightroomVersion: z.string(),
  sdkVersion: z.string().nullish(),
  pluginVersion: z.string(),
  developSettingKeys: z.array(z.string()).default([]),
  supports: SupportFlags.partial().default({}),
  notes: z.array(z.string()).default([]),
});
export type CapabilityProbe = z.infer<typeof CapabilityProbe>;

export const ControlStatus = z.enum(["supported", "observed_not_writable", "unsupported"]);

export const ControlCapability = z.object({
  canonical: z.string(),
  family: z.string(),
  status: ControlStatus,
  lightroomKey: z.string().nullable(),
  reason: z.string(),
});

export const FamilySummary = z.object({
  label: z.string(),
  supported: z.number(),
  observedNotWritable: z.number(),
  unsupported: z.number(),
});

export const CapabilityMatrix = z.object({
  schemaVersion: z.string(),
  lightroomVersion: z.string(),
  pluginVersion: z.string(),
  probeHadPhoto: z.boolean(),
  canApply: z.boolean(),
  canSnapshot: z.boolean(),
  canRead: z.boolean(),
  controls: z.array(ControlCapability),
  familySummary: z.record(z.string(), FamilySummary),
  localEdits: z.string(),
  masks: z.string(),
});
export type CapabilityMatrix = z.infer<typeof CapabilityMatrix>;

export const ConnectionInfo = z.object({
  sessionId: z.string(),
  pluginVersion: z.string(),
  lightroomVersion: z.string(),
  sdkVersion: z.string().nullable(),
  catalogFingerprint: z.string(),
  catalogName: z.string().nullable(),
  probe: CapabilityProbe,
  capabilities: CapabilityMatrix,
  connectedAt: z.string(),
});
export type ConnectionInfo = z.infer<typeof ConnectionInfo>;

export const BridgeStatus = z.object({
  listening: z.boolean(),
  baseUrl: z.string(),
  connected: z.boolean(),
  lastSeenMsAgo: z.number().nullable(),
  connection: ConnectionInfo.nullable(),
  queuedCommands: z.number(),
  inFlightCommands: z.number(),
  totalHandshakes: z.number(),
  totalCommandsCompleted: z.number(),
});
export type BridgeStatus = z.infer<typeof BridgeStatus>;

export const LightroomConnectionRecord = z.object({
  id: z.string(),
  catalogFingerprint: z.string(),
  lightroomVersion: z.string().nullable(),
  sdkVersion: z.string().nullable(),
  pluginVersion: z.string().nullable(),
  capabilities: z.unknown(),
  firstSeenAt: z.string(),
  lastSeenAt: z.string(),
  status: z.string(),
});

export const LightroomStatus = z.object({
  bridge: BridgeStatus,
  lastKnown: LightroomConnectionRecord.nullable(),
  pluginInstalledPath: z.string(),
  pluginInstalled: z.boolean(),
  pluginSourceAvailable: z.boolean(),
});
export type LightroomStatus = z.infer<typeof LightroomStatus>;

export const PluginSetup = z.object({
  pluginPath: z.string(),
  bridgeFile: z.string(),
  steps: z.array(z.string()),
  pluginVersion: z.string(),
});
export type PluginSetup = z.infer<typeof PluginSetup>;

export const ConnectionTest = z.object({
  connected: z.boolean(),
  roundTripMs: z.number().nullable(),
  message: z.string(),
  connection: ConnectionInfo.nullable(),
});
export type ConnectionTest = z.infer<typeof ConnectionTest>;

export const CapabilityMatrixResponse = z.object({
  live: z.boolean(),
  matrix: CapabilityMatrix.nullable(),
  lastSeenAt: z.string().optional(),
});

export const PluginEvent = z.object({
  type: z.string(),
  at: z.string().nullish(),
  payload: z.unknown(),
});

export const BridgeEvent = z.discriminatedUnion("type", [
  z.object({ type: z.literal("connected"), connection: ConnectionInfo }),
  z.object({ type: z.literal("disconnected"), reason: z.string() }),
  z.object({ type: z.literal("plugin_event"), event: PluginEvent }),
  z.object({
    type: z.literal("command_completed"),
    commandId: z.string(),
    commandType: z.string(),
    ok: z.boolean(),
  }),
]);
export type BridgeEvent = z.infer<typeof BridgeEvent>;
