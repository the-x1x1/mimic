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

/**
 * What folding people back into the user changed. Counts of rows changed,
 * never estimates; zero means nothing was there to fold.
 */
export const Claimed = z.object({
  /** Messages that were counted as someone else's and are now the user's. */
  messages: z.number(),
  /** People who were the user under an address, folded back in. */
  people: z.number(),
});
export type Claimed = z.infer<typeof Claimed>;

/** What adding an address did: the identity as it now stands, and what moved. */
export const AddressAdded = z.object({
  identity: UserIdentity,
  claimed: Claimed,
});
export type AddressAdded = z.infer<typeof AddressAdded>;

/**
 * The person mail from an address is filed under, and everything that would
 * change if the user says the address is theirs — computed by the same query
 * the fold runs, which refuses unless this person is the one confirmed.
 */
export const AddressOwner = z.object({
  participantId: z.string(),
  displayName: z.string(),
  /** Messages filed under them. Every one would become the user's. */
  messages: z.number(),
  /** Their other addresses, not yet the user's, which would become the user's. */
  otherAddresses: z.array(z.string()),
  /** What the user said they are to them, which would go. */
  relationship: z.string().nullable(),
  hasNotes: z.boolean(),
  /** Preferences the user set for writing to them, which would go. */
  preferences: z.number(),
});
export type AddressOwner = z.infer<typeof AddressOwner>;

/** What adding an address would do, before anything is changed. */
export const AddressPreview = z.object({
  alreadyYours: z.boolean(),
  owner: AddressOwner.nullable(),
});
export type AddressPreview = z.infer<typeof AddressPreview>;

/** One of the user's addresses that mail already read is still filed under someone else by. */
export const HeldAddress = z.object({
  identifier: Identifier,
  owner: AddressOwner,
  /** The user already said this person is not them; they are never folded in unasked. */
  keptApart: z.boolean(),
});
export type HeldAddress = z.infer<typeof HeldAddress>;

function listed(items: string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/**
 * The question to ask before an address filed under someone becomes the
 * user's, and what saying yes does, in sentences from the preview's counts.
 * The question is about the person, because that is what is decided: every
 * message filed under them becomes the user's, whichever address it came from.
 */
export function describeFold(
  owner: AddressOwner,
  address: string,
): { question: string; lines: string[] } {
  const name = owner.displayName;
  const lines = [`I have ${address} down as ${name}'s.`];
  const n = owner.messages;
  if (n === 0) {
    lines.push(`If ${name} is you, they're no longer among your people.`);
  } else {
    const mail =
      n === 1
        ? "the one message filed under them becomes yours"
        : `the ${n.toLocaleString()} messages filed under them become yours`;
    lines.push(`If ${name} is you, ${mail}, and ${name} is no longer among your people.`);
  }
  const others = owner.otherAddresses;
  if (others.length === 1) {
    lines.push(`Their other address, ${others[0]}, becomes yours too.`);
  } else if (others.length > 1) {
    lines.push(`Their other addresses, ${listed(others)}, become yours too.`);
  }
  const said: string[] = [];
  if (owner.relationship) said.push(`what they are to you ("${owner.relationship}")`);
  if (owner.hasNotes) said.push("your notes");
  if (owner.preferences === 1) said.push("the preference you set for writing to them");
  if (owner.preferences > 1) {
    said.push(`the ${owner.preferences} preferences you set for writing to them`);
  }
  if (said.length > 0) lines.push(`What you told me about them goes: ${listed(said)}.`);
  lines.push(
    n === 0
      ? "This can't be undone."
      : `This can't be undone: removing the address later won't make ${n === 1 ? "that message" : "those messages"} ${name}'s again.`,
  );
  return { question: `Is ${name} you?`, lines };
}

/**
 * What to tell the user after an address was added, or null when no message
 * moved — the question they answered already said who went.
 */
export function describeClaimed(c: Claimed): string | null {
  if (c.messages === 0) return null;
  const mail =
    c.messages === 1
      ? "One message I'd already read is yours now"
      : `${c.messages.toLocaleString()} messages I'd already read are yours now`;
  return `${mail}. I'll look at how you write again so ${c.messages === 1 ? "it counts" : "they count"}.`;
}

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
