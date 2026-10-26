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
  /** People it never answers by itself, whatever `who` says (0.10.0-alpha.37). */
  never: z.array(z.string()),
  /** How long a reply is held before it goes, in minutes. */
  holdMinutes: z.number().int(),
  /** Replies it may send by itself in a day. */
  dailyLimit: z.number().int(),
  /** When it was last turned on: only what arrived after is answered by it. */
  enabledAt: z.string().nullable(),
  /** The hours of the day replies may go by themselves, on this computer's clock (0.10.0-alpha.39). */
  hours: z.object({ from: z.number().int(), to: z.number().int() }).nullable().optional(),
  /**
   * Rehearsing (0.10.0-alpha.45): held, shown and checked as for real, but
   * stopped when it would go. Nothing is sent.
   */
  rehearse: z.boolean().optional(),
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
  /** Gone or not, the connection lost as it went (0.10.0-alpha.44). */
  unsure: z.number().int().optional(),
  /** Stopped when it would have gone, rehearsing (0.10.0-alpha.45). */
  rehearsed: z.number().int().optional(),
});
export type AutoReplyRun = z.infer<typeof AutoReplyRun>;

/** The hold lengths Settings offers, in minutes. */
export const HOLD_CHOICES = [2, 5, 10, 30, 60] as const;

/** A held reply, as the line under its message says it — as a rehearsal while rehearsing. */
export function describeHeld(
  entry: OutboxEntry,
  now: Date = new Date(),
  rehearsing = false,
): string {
  const at = new Date(entry.sendAfter);
  const minutes = Math.max(0, Math.ceil((at.getTime() - now.getTime()) / 60_000));
  // An hour or more away — the hours allowed haven't started — by the clock.
  const clock = at.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  const when =
    minutes <= 0
      ? "in a moment"
      : minutes === 1
        ? "in a minute"
        : minutes < 60
          ? `in ${minutes.toLocaleString()} minutes`
          : at.toDateString() === now.toDateString()
            ? `at ${clock}`
            : `tomorrow at ${clock}`;
  if (rehearsing)
    return `Rehearsing: I'd send this to ${listAddresses(entry.to)} by myself ${when}. Nothing will go; then it's yours.`;
  return `I'll send this to ${listAddresses(entry.to)} by myself ${when}, unless you stop it.`;
}

/** What the rules allow, in one sentence. */
export function describeAutoReply(state: AutoReplyState): string {
  const r = state.rules;
  if (!r.enabled) return "Off. Every reply waits for you.";
  if (!state.running)
    return "On, but replies aren't being prepared in advance, so there's nothing to send.";
  if (r.mailboxes.length === 0) return "On, but for no mailbox yet, so nothing is sent.";
  const chosen = r.people.filter((p) => !r.never.includes(p)).length;
  const who =
    r.who === "known"
      ? r.never.length === 0
        ? "people you've written to yourself"
        : `people you've written to yourself, except ${r.never.length === 1 ? "the one you named" : `the ${r.never.length.toLocaleString()} you named`}`
      : chosen === 1
        ? "the one person you chose"
        : `the ${chosen.toLocaleString()} people you chose`;
  const when = r.hours ? `, only between ${hourOf(r.hours.from)} and ${hourOf(r.hours.to)}` : "";
  if (r.rehearse)
    return `Rehearsing: I'd answer ${who}, holding each reply ${r.holdMinutes.toLocaleString()} minutes first${when}, ${r.dailyLimit.toLocaleString()} a day at most — and send none of them.`;
  return `On: I answer ${who}, holding each reply ${r.holdMinutes.toLocaleString()} minutes first${when}, ${r.dailyLimit.toLocaleString()} a day at most.`;
}

/** An hour of the day as a clock shows it: 8:00, 20:00. */
export function hourOf(hour: number): string {
  return `${hour}:00`;
}
