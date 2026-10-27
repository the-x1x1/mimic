import { z } from "zod";

/**
 * Sending replies through the user's own mailbox (0.10.0-alpha.23): how a
 * mailbox sends (`sources::smtp::SmtpAccount`), who a reply goes to before
 * it goes (`sending::ReplyEnvelope`), and what was sent (`db::OutboxEntry`).
 */

/** `tls` is encrypted from the start (465); `startTls` encrypts first thing (587); `plain` is this computer only. */
export const SmtpSecurity = z.enum(["tls", "startTls", "plain"]);
export type SmtpSecurity = z.infer<typeof SmtpSecurity>;

export const SmtpAccount = z.object({
  host: z.string(),
  port: z.number().int(),
  security: SmtpSecurity,
  /** Who it signs in as: the mailbox's own username. */
  username: z.string(),
  /** The address replies go out from. */
  fromAddress: z.string(),
  /** A copy of each reply in the Sent folder: null for what the provider needs. */
  copyToSent: z.boolean().nullable().optional(),
});
export type SmtpAccount = z.infer<typeof SmtpAccount>;

/** How a mailbox sends: as the user set it, or its provider's known server. */
export const MailboxSending = z.object({
  sending: SmtpAccount.nullable(),
  setByUser: z.boolean(),
  /** Its provider keeps what is sent in the Sent folder itself. */
  keepsSent: z.boolean(),
  /** What goes under each reply sent from it (0.10.0-alpha.36). */
  signature: z.string().nullable().optional(),
  /**
   * Its organization turned off sending from apps by SMTP, so replies go
   * through Microsoft 365 itself (0.10.0-alpha.94).
   */
  throughGraph: z.boolean().optional(),
  /** Replies from it quote the message they answer (0.10.0-alpha.96). */
  quotes: z.boolean().optional(),
});
export type MailboxSending = z.infer<typeof MailboxSending>;

export const SendingMailbox = z.object({
  sourceId: z.string(),
  name: z.string(),
  /** Null when it can't send; `whyNot` says why. */
  fromAddress: z.string().nullable(),
  server: z.string().nullable(),
  whyNot: z.string().nullable(),
  /** What goes under a reply sent through it, after a `-- ` line (0.10.0-alpha.36). */
  signature: z.string().nullable().optional(),
  /** Whether a reply through it quotes what it answers, unless changed for that reply (0.10.0-alpha.96). */
  quotes: z.boolean().optional(),
});
export type SendingMailbox = z.infer<typeof SendingMailbox>;

export const SendHow = z.enum(["approved", "autonomous"]);
export type SendHow = z.infer<typeof SendHow>;
export const SendStatus = z.enum(["held", "sending", "sent", "unsure", "failed", "canceled"]);
export type SendStatus = z.infer<typeof SendStatus>;

/** One reply Mimic sent, is sending, or holds to send. */
export const OutboxEntry = z.object({
  id: z.string(),
  draftId: z.string().nullable(),
  sourceId: z.string(),
  conversationId: z.string().nullable(),
  answersMessageId: z.string().nullable(),
  participantId: z.string().nullable(),
  fromAddress: z.string(),
  to: z.array(z.string()),
  cc: z.array(z.string()),
  subject: z.string(),
  body: z.string(),
  messageId: z.string(),
  how: SendHow,
  status: SendStatus,
  sendAfter: z.string(),
  error: z.string().nullable(),
  createdAt: z.string(),
  updatedAt: z.string(),
  sentAt: z.string().nullable(),
  /**
   * How it went, when not the usual way: `graph`, through Microsoft 365
   * itself, for a mailbox whose organization turned off sending by SMTP
   * (0.10.0-alpha.94).
   */
  via: z.string().nullable().optional(),
});
export type OutboxEntry = z.infer<typeof OutboxEntry>;

/** Everything the Send panel shows before anything goes. */
export const ReplyEnvelope = z.object({
  draftId: z.string(),
  /** Every connected mailbox, the one the message came in to first. */
  mailboxes: z.array(SendingMailbox),
  /** The one it goes through unless the user picks another. */
  mailbox: z.string().nullable(),
  to: z.array(z.string()),
  /** The person's other addresses, to send to instead. */
  otherAddresses: z.array(z.string()),
  /** Everyone else the message was to or copied to, not the user: the ones that can be copied in. */
  othersOnIt: z.array(z.string()),
  subject: z.string(),
  /** Why it can't be sent from here at all. */
  whyNot: z.string().nullable(),
  /** A reply to it held to go by itself, which can be stopped or sent now. */
  held: OutboxEntry.nullable(),
  /**
   * Out of a mailbox, or into Slack (0.10.0-alpha.65) or Teams (.68): for
   * those the one "mailbox" is the workspace or account, `to` is the
   * conversation, and there is no subject. Absent is mail.
   */
  kind: z.enum(["mail", "slack", "teams", "telegram", "googleChat"]).optional(),
  /** The message it answers, quoted as it would go under the reply (0.10.0-alpha.96). */
  quoted: z.string().nullable().optional(),
});
export type ReplyEnvelope = z.infer<typeof ReplyEnvelope>;

/** What the user approved, sent exactly as given. */
export interface SendRequest {
  draftId: string;
  text: string;
  sourceId: string;
  to: string[];
  /** The others on the message the user ticked; only ever from `ReplyEnvelope.othersOnIt`. */
  cc: string[];
  subject: string;
  /** Quote the message it answers under it, or not; unset is as its mailbox does (0.10.0-alpha.96). */
  quote?: boolean;
  /** The quote as the Send panel showed it: a reply that quotes goes only with that one (0.10.0-alpha.100). */
  shownQuote?: string | null;
}

/** The address list as one would write it on an envelope. */
export function listAddresses(addresses: readonly string[]): string {
  if (addresses.length <= 2) return addresses.join(" and ");
  return `${addresses.slice(0, -1).join(", ")} and ${addresses.at(-1) ?? ""}`;
}

/**
 * Where a reply stands, in one sentence: sent, going, waiting to go, stopped,
 * or why it didn't go. A reply that may have gone says to look before sending
 * it again.
 */
export function describeOutboxEntry(e: OutboxEntry): string {
  const to =
    e.cc.length > 0
      ? `${listAddresses(e.to)}, copying in ${listAddresses(e.cc)}`
      : listAddresses(e.to);
  switch (e.status) {
    case "sent": {
      const through = e.via === "graph" ? ", through Microsoft 365" : "";
      const said =
        e.how === "autonomous"
          ? `I sent this to ${to} by myself${through}.`
          : `Sent to ${to}${through}.`;
      // A note on a reply that went: its copy in the Sent folder couldn't be made.
      return e.error ? `${said} ${e.error}` : said;
    }
    case "sending":
      return `Sending to ${to}…`;
    case "held":
      return `Waiting to go to ${to}. You can stop it until then.`;
    case "canceled":
      // Why, when Mimic stopped it: the rules, the user, or a rehearsal.
      return e.error
        ? `Stopped before it went to ${to}. ${e.error}`
        : `Stopped before it went to ${to}.`;
    case "unsure":
      return e.fromAddress.startsWith("slack:")
        ? `May have gone to ${to} — I couldn't tell. Look in the conversation in Slack.`
        : e.fromAddress.startsWith("teams:")
          ? `May have gone to ${to} — I couldn't tell. Look in the chat in Teams.`
          : e.fromAddress.startsWith("telegram:")
            ? `May have gone to ${to} — I couldn't tell. Look in the chat in Telegram.`
            : e.fromAddress.startsWith("google_chat:")
              ? `May have gone to ${to} — I couldn't tell. Look in the chat in Google Chat.`
              : `May have gone to ${to} — I couldn't tell. Look in your Sent folder.`;
    case "failed":
      return e.error ? `Didn't go to ${to}: ${e.error}` : `Didn't go to ${to}.`;
  }
}

/** Codes a send can fail with, and what each means for the Send button. */
export function sendMayHaveGone(code: string | undefined): boolean {
  return code === "send_unsure";
}
