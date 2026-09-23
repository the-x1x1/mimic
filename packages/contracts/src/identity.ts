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
  /**
   * Everything they sent looks automated from its headers, and the user has
   * neither written to them nor said anything about them. A reading, not a fact.
   */
  automated: z.boolean(),
});
export type ParticipantSummary = z.infer<typeof ParticipantSummary>;

/**
 * The People screen: people (possibly fewer than `peopleTotal`), and how many
 * senders were left out because everything they sent looks automated — the
 * senders themselves only when asked for.
 */
export const PeopleView = z.object({
  people: z.array(ParticipantSummary),
  peopleTotal: z.number(),
  automatedSendersTotal: z.number(),
  automatedSenders: z.array(ParticipantSummary),
  showingAutomated: z.boolean(),
});
export type PeopleView = z.infer<typeof PeopleView>;

/** The line under the People heading, or null when nothing needs saying. */
export function describePeopleList(v: PeopleView): string | null {
  const parts: string[] = [];
  if (v.people.length < v.peopleTotal) {
    parts.push(
      `Here are the ${v.people.length.toLocaleString()} most recent of ${v.peopleTotal.toLocaleString()}.`,
    );
  }
  const n = v.automatedSendersTotal;
  if (n === 1) {
    parts.push("I left out one sender whose mail all looks automated.");
  } else if (n > 1) {
    parts.push(
      `I left out ${n.toLocaleString()} senders whose mail all looks automated (newsletters, notifications, no-reply addresses).`,
    );
  }
  return parts.length > 0 ? parts.join(" ") : null;
}

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
