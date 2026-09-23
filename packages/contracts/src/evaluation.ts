import { z } from "zod";

/**
 * Measuring the drafts (mimic-core `evaluation`): how close Mimic's replies
 * come to what the user actually wrote, next to two baselines, on
 * conversations it was not shown. There is no headline number, by design —
 * the measures are not commensurable — so nothing here produces one.
 */

/** The three ways each held-out exchange is answered. */
export const EvaluationSystem = z.enum(["mimic", "generic", "common_reply"]);
export type EvaluationSystem = z.infer<typeof EvaluationSystem>;

/** A measure's mean over the cases still here, and its 10th percentile. */
export const Measure = z.object({ mean: z.number(), p10: z.number() });
export type Measure = z.infer<typeof Measure>;

export const SystemResult = z.object({
  system: EvaluationSystem,
  cases: z.number(),
  length: Measure.nullable(),
  vocabulary: Measure.nullable(),
  punctuation: Measure.nullable(),
  /** Absent when the engine had no encoder to measure it with. */
  embedding: Measure.nullable(),
});
export type SystemResult = z.infer<typeof SystemResult>;

export const EvaluationCase = z.object({
  incoming: z.string(),
  from: z.string().nullable(),
  /** What the user actually wrote. */
  reply: z.string(),
  answers: z.array(z.object({ system: EvaluationSystem, text: z.string() })),
});
export type EvaluationCase = z.infer<typeof EvaluationCase>;

export const EvaluationView = z.object({
  id: z.string(),
  createdAt: z.string(),
  provider: z.string(),
  model: z.string().nullable(),
  /** Exchanges answered when it ran. */
  measured: z.number(),
  /** Of those, how many are still here: the rest were deleted since. */
  remaining: z.number(),
  strategy: z.string(),
  /** Conversations with an exchange, and how many the split held back. */
  conversations: z.number(),
  heldOutConversations: z.number(),
  /** Conversations the remaining cases come from. */
  measuredConversations: z.number(),
  warnings: z.array(z.string()),
  embeddingProvider: z.string().nullable(),
  commonReply: z.object({ text: z.string(), times: z.number().nullable() }).nullable(),
  systems: z.array(SystemResult),
  cases: z.array(EvaluationCase),
});
export type EvaluationView = z.infer<typeof EvaluationView>;

export const SYSTEM_LABELS: Record<EvaluationSystem, string> = {
  mimic: "My drafts",
  generic: "A generic reply",
  common_reply: "Your most common reply",
};

export const MEASURES = ["length", "vocabulary", "punctuation", "embedding"] as const;
export type MeasureKey = (typeof MEASURES)[number];

/** What each measure is, in the words the screen uses. */
export const MEASURE_LABELS: Record<MeasureKey, { label: string; explains: string }> = {
  length: {
    label: "Length",
    explains: "The shorter of the two word counts over the longer.",
  },
  vocabulary: {
    label: "Words in common",
    explains: "Words both used, out of every word either used.",
  },
  punctuation: {
    label: "Punctuation habits",
    explains:
      "Five habits matched or not: a full stop at the end, a question mark, an exclamation mark, a lowercase start, a paragraph break.",
  },
  embedding: {
    label: "Overall wording",
    explains: "How alike the two read overall, by the engine's own measure.",
  },
};

/** A 0–1 resemblance as a percentage. */
export function formatShare(x: number): string {
  return `${Math.round(x * 100)}%`;
}

/**
 * What the "overall wording" measure used, said plainly: the lexical
 * encoder compares wording, not meaning, and a reader should not take it for
 * more.
 */
export function describeEncoder(provider: string | null): string | null {
  if (provider === null) return null;
  return provider.startsWith("lexical")
    ? `"Overall wording" was measured by ${provider}, which compares the words and letters used, not what they mean.`
    : `"Overall wording" was measured by ${provider}.`;
}

/** What a measurement was taken on, in one line. */
export function describeEvaluation(v: EvaluationView): string {
  const replies = v.remaining === 1 ? "1 of your replies" : `${v.remaining} of your replies`;
  const threads =
    v.measuredConversations === 1 ? "1 conversation" : `${v.measuredConversations} conversations`;
  const model = v.model ? `, and the replies were written by ${v.model}` : "";
  return `Measured on ${replies}, from ${threads} I held back and didn't learn from${model}.`;
}

/**
 * Said when replies it was measured on no longer count: someone was found
 * to be you since, so what they "sent" is yours and answers nothing.
 */
export function describeGone(v: EvaluationView): string | null {
  const gone = v.measured - v.remaining;
  if (gone <= 0) return null;
  if (v.remaining === 0)
    return "None of the replies this was measured on count any more: the mail has changed since. Measure again to see where things stand.";
  return `${gone === 1 ? "1 reply" : `${gone} replies`} it was measured on ${gone === 1 ? "doesn't" : "don't"} count any more: the mail has changed since. The figures are from the ${v.remaining} left.`;
}

/**
 * The engine's notes on how the split came out, in the screen's words. The
 * engine writes them for developers; these say what they mean for the
 * figures. Anything unrecognised is shown as the engine wrote it.
 */
export function describeSplitWarning(w: string): string {
  if (w.startsWith("only two conversations"))
    return "Only two of your conversations had replies to measure, so everything here rests on one of them.";
  if (w.startsWith("the holdout fraction would have consumed every conversation"))
    return "I kept your largest conversation to learn from, so fewer were held back than I'd have liked.";
  if (w.startsWith("every message comes from one conversation"))
    return "Every reply came from one conversation, so the ones held back weren't independent of the rest, and the figures flatter me.";
  return w;
}
