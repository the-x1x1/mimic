import { z } from "zod";
import { listAddresses, type OutboxEntry } from "./sending";

/**
 * Replies Mimic sends by itself — Trusted mode, 0.10.0-alpha.24
 * (`mimic_core::autoreply`). Off until the user turns it on.
 */

/** `known`: anyone the user has written to themselves; `listed`: only the people they chose. */
export const AutoReplyWho = z.enum(["known", "listed"]);
export type AutoReplyWho = z.infer<typeof AutoReplyWho>;

export const AutoReplyRules = z.object({
  enabled: z.boolean(),
  /** The mailboxes it may answer through (source ids). */
  mailboxes: z.array(z.string()),
  who: AutoReplyWho,
  /** With `listed`, the people it may answer (participant ids). */
  people: z.array(z.string()),
  /** How long a reply is held before it goes, in minutes. */
  holdMinutes: z.number().int(),
  /** Replies it may send by itself in a day. */
  dailyLimit: z.number().int(),
  /** When it was last turned on: only what arrived after is answered by it. */
  enabledAt: z.string().nullable(),
});
export type AutoReplyRules = z.infer<typeof AutoReplyRules>;

export const AutoReplyState = z.object({
  rules: AutoReplyRules,
  /** On, and replies are being prepared in advance. */
  running: z.boolean(),
  /** Replies sent or held by themselves in the last day, against the limit. */
  lastDay: z.number().int(),
});
export type AutoReplyState = z.infer<typeof AutoReplyState>;

/** What a look at held replies did, as `sending://changed` says it. */
export const AutoReplyRun = z.object({
  held: z.number().int(),
  sent: z.number().int(),
  stopped: z.number().int(),
  failed: z.number().int(),
});
export type AutoReplyRun = z.infer<typeof AutoReplyRun>;

/** The hold lengths Settings offers, in minutes. */
export const HOLD_CHOICES = [2, 5, 10, 30, 60] as const;

/** A held reply, as the line under its message says it. */
export function describeHeld(entry: OutboxEntry, now: Date = new Date()): string {
  const at = new Date(entry.sendAfter);
  const minutes = Math.max(0, Math.ceil((at.getTime() - now.getTime()) / 60_000));
  const when =
    minutes <= 0
      ? "in a moment"
      : minutes === 1
        ? "in a minute"
        : `in ${minutes.toLocaleString()} minutes`;
  return `I'll send this to ${listAddresses(entry.to)} by myself ${when}, unless you stop it.`;
}

/** What the rules allow, in one sentence. */
export function describeAutoReply(state: AutoReplyState): string {
  const r = state.rules;
  if (!r.enabled) return "Off. Every reply waits for you.";
  if (!state.running)
    return "On, but replies aren't being prepared in advance, so there's nothing to send.";
  if (r.mailboxes.length === 0) return "On, but for no mailbox yet, so nothing is sent.";
  const who =
    r.who === "known"
      ? "people you've written to yourself"
      : r.people.length === 1
        ? "the one person you chose"
        : `the ${r.people.length.toLocaleString()} people you chose`;
  return `On: I answer ${who}, holding each reply ${r.holdMinutes.toLocaleString()} minutes first, ${r.dailyLimit.toLocaleString()} a day at most.`;
}
