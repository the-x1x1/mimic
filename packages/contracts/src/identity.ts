import { z } from "zod";

export const IdentifierKind = z.enum(["email", "phone", "handle", "display_name", "account_id"]);
export type IdentifierKind = z.infer<typeof IdentifierKind>;

export const IDENTIFIER_LABELS: Record<IdentifierKind, string> = {
  email: "Email address",
  phone: "Phone number",
  handle: "Username or handle",
  display_name: "Display name",
  account_id: "Account ID",
};

export const Identifier = z.object({
  id: z.string(),
  kind: z.string(),
  value: z.string(),
  normalizedValue: z.string(),
});
export type Identifier = z.infer<typeof Identifier>;

export const UserIdentity = z.object({
  id: z.string(),
  displayName: z.string(),
  identifiers: z.array(Identifier),
  createdAt: z.string(),
  updatedAt: z.string(),
});
export type UserIdentity = z.infer<typeof UserIdentity>;

export const Participant = z.object({
  id: z.string(),
  displayName: z.string(),
  isSelf: z.boolean(),
  relationship: z.string().nullable(),
  notes: z.string().nullable(),
  identifiers: z.array(Identifier),
  createdAt: z.string(),
  updatedAt: z.string(),
});
export type Participant = z.infer<typeof Participant>;

export const ParticipantSummary = z.object({
  participant: Participant,
  messageCount: z.number(),
  sentByUser: z.number(),
  conversationCount: z.number(),
  channels: z.array(z.string()),
  firstMessageAt: z.string().nullable(),
  lastMessageAt: z.string().nullable(),
  hasRelationshipProfile: z.boolean(),
});
export type ParticipantSummary = z.infer<typeof ParticipantSummary>;

/**
 * Relationships Mimic offers as suggestions. The field is free text: these are
 * a starting point, not a taxonomy the user has to fit into.
 */
export const SUGGESTED_RELATIONSHIPS = [
  "close friend",
  "friend",
  "family",
  "partner",
  "colleague",
  "manager",
  "report",
  "client",
  "acquaintance",
] as const;
