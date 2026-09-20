import { z } from "zod";

export const Channel = z.enum(["email", "sms", "chat", "forum", "other"]);
export type Channel = z.infer<typeof Channel>;

export const CHANNEL_LABELS: Record<Channel, string> = {
  email: "Email",
  sms: "Text messages",
  chat: "Chat",
  forum: "Forums and threads",
  other: "Other",
};

export const ConnectorInfo = z.object({
  connector: z.string(),
  displayName: z.string(),
  channel: z.string(),
  description: z.string(),
  locationKind: z.enum(["file", "folder"]),
  extensions: z.array(z.string()),
});
export type ConnectorInfo = z.infer<typeof ConnectorInfo>;

export const SourceStatus = z.enum(["new", "ready", "importing", "imported", "failed"]);
export type SourceStatus = z.infer<typeof SourceStatus>;

export const Source = z.object({
  id: z.string(),
  connector: z.string(),
  name: z.string(),
  channel: z.string(),
  location: z.string().nullable(),
  config: z.record(z.string(), z.unknown()),
  status: z.string(),
  createdAt: z.string(),
  lastImportedAt: z.string().nullable(),
  messageCount: z.number(),
  lastError: z.object({ message: z.string() }).passthrough().nullable(),
});
export type Source = z.infer<typeof Source>;

/** What `validate` found, before the user commits to importing. */
export const ValidationReport = z.object({
  ok: z.boolean(),
  blockers: z.array(z.string()),
  warnings: z.array(z.string()),
  conversations: z.number(),
  messages: z.number(),
  frequentIdentifiers: z.array(z.tuple([z.string(), z.number()])),
  earliest: z.string().nullable(),
  latest: z.string().nullable(),
});
export type ValidationReport = z.infer<typeof ValidationReport>;

export const ImportSummary = z.object({
  conversations: z.number(),
  inserted: z.number(),
  duplicates: z.number(),
  empty: z.number(),
  fromSelf: z.number(),
  unattributed: z.number(),
  participantsCreated: z.number(),
});
export type ImportSummary = z.infer<typeof ImportSummary>;

/**
 * One line summarising an import, for the job tray. Says what was skipped and
 * why, because "45 messages imported" hides the 1 that could not be attributed.
 */
export function describeImport(s: ImportSummary): string {
  const parts = [`${s.inserted} messages from ${s.conversations} conversations`];
  if (s.fromSelf > 0) parts.push(`${s.fromSelf} written by you`);
  if (s.duplicates > 0) parts.push(`${s.duplicates} already imported`);
  if (s.unattributed > 0) parts.push(`${s.unattributed} with no identifiable author`);
  if (s.empty > 0) parts.push(`${s.empty} with no text`);
  return parts.join(", ");
}
