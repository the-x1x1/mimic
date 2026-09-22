import { z } from "zod";
import { Participant } from "./identity";
import { LearnedPattern, ResolvedLayer, VoiceMetrics } from "./voice";

export const Adjustment = z.enum(["shorter", "longer", "moreCasual", "moreProfessional"]);
export type Adjustment = z.infer<typeof Adjustment>;

export const ADJUSTMENT_LABELS: Record<Adjustment, string> = {
  shorter: "Shorter",
  longer: "Longer",
  moreCasual: "More casual",
  moreProfessional: "More professional",
};

export const ComposeRequest = z.object({
  participantId: z.string().nullish(),
  conversationId: z.string().nullish(),
  channel: z.string(),
  incomingMessage: z.string().nullish(),
  intent: z.string().nullish(),
  situationId: z.string().nullish(),
  adjustment: Adjustment.nullish(),
});
export type ComposeRequest = z.infer<typeof ComposeRequest>;

export const Message = z.object({
  id: z.string(),
  conversationId: z.string(),
  sourceId: z.string(),
  participantId: z.string().nullable(),
  externalId: z.string(),
  direction: z.enum(["self", "other", "unknown"]),
  channel: z.string(),
  sentAt: z.string().nullable(),
  sequenceIndex: z.number(),
  body: z.string(),
  wordCount: z.number(),
  charCount: z.number(),
  replyToMessageId: z.string().nullable(),
  responseLatencySeconds: z.number().nullable(),
  metadata: z.record(z.string(), z.unknown()),
});
export type Message = z.infer<typeof Message>;

export const RetrievedExchange = z.object({
  replyMessageId: z.string(),
  reply: z.string(),
  incoming: z.string().nullable(),
  participantId: z.string().nullable(),
  channel: z.string(),
  sentAt: z.string().nullable(),
  score: z.number(),
  reason: z.string(),
});
export type RetrievedExchange = z.infer<typeof RetrievedExchange>;

export const ResolvedVoice = z.object({
  layers: z.array(ResolvedLayer),
  overrides: z.array(z.tuple([z.string(), z.unknown()])),
  examples: z.array(
    z.object({
      id: z.string(),
      messageId: z.string(),
      layer: z.string(),
      scopeKey: z.string(),
      participantId: z.string().nullable(),
      reason: z.string(),
      score: z.number(),
      body: z.string(),
      sentAt: z.string().nullable(),
    }),
  ),
});
export type ResolvedVoice = z.infer<typeof ResolvedVoice>;

/**
 * What a reply is doing, and how Mimic knows. `chosen` is something the user
 * said; `fromNote` is Mimic's reading of their note, and the UI must word it
 * as a reading ("your note reads like…"), never as a fact.
 */
export const SituationChoice = z.object({
  id: z.string(),
  label: z.string(),
  source: z.enum(["chosen", "fromNote"]),
  cue: z.string().nullable(),
});
export type SituationChoice = z.infer<typeof SituationChoice>;

/** How a draft's situation is described under it. */
export function describeSituation(s: SituationChoice | null | undefined): string | null {
  if (!s) return null;
  const what = s.label.toLowerCase();
  return s.source === "chosen"
    ? `Written as ${what}, because you said so.`
    : `Your note read like ${what}, so I wrote it the way you usually do that.`;
}

/**
 * What a draft will be based on, before one exists. `evidence` is written for
 * a person to read and is shown verbatim.
 */
export const GenerationContext = z.object({
  participant: Participant.nullable(),
  channel: z.string(),
  voice: ResolvedVoice,
  effective: VoiceMetrics,
  examples: z.array(RetrievedExchange),
  transcript: z.array(Message),
  situation: SituationChoice.nullable(),
  learned: z.array(LearnedPattern),
  evidence: z.array(z.string()),
});
export type GenerationContext = z.infer<typeof GenerationContext>;

export const DraftOutcome = z.enum(["sent_unedited", "sent_edited", "discarded", "regenerated"]);
export type DraftOutcome = z.infer<typeof DraftOutcome>;

export const Draft = z.object({
  id: z.string(),
  participantId: z.string().nullable(),
  conversationId: z.string().nullable(),
  channel: z.string(),
  situationId: z.string().nullable(),
  incomingMessage: z.string().nullable(),
  intent: z.string().nullable(),
  generatedText: z.string(),
  finalText: z.string().nullable(),
  provider: z.string(),
  model: z.string(),
  context: z.record(z.string(), z.unknown()),
  promptHash: z.string(),
  evidence: z.record(z.string(), z.unknown()),
  createdAt: z.string(),
  resolvedAt: z.string().nullable(),
  outcome: z.string().nullable(),
});
export type Draft = z.infer<typeof Draft>;

export const DraftFeedback = z.object({
  id: z.string(),
  draftId: z.string(),
  kind: z.enum(["edit", "preference", "rating"]),
  weight: z.number(),
  diff: z.unknown(),
  note: z.string().nullable(),
  createdAt: z.string(),
  appliedToAnalysisVersion: z.string().nullable(),
});
export type DraftFeedback = z.infer<typeof DraftFeedback>;

/** Measured outcomes. Nulls mean unmeasured and must be rendered as such. */
export const DraftOutcomes = z.object({
  total: z.number(),
  resolved: z.number(),
  sentUnedited: z.number(),
  sentEdited: z.number(),
  discarded: z.number(),
  uneditedRate: z.number().nullable(),
  meanLengthDelta: z.number().nullable(),
});
export type DraftOutcomes = z.infer<typeof DraftOutcomes>;

/**
 * Whether a draft can be asked for at all, and what to say if not. Keeping
 * this here rather than in a component means the Compose screen and the
 * onboarding cannot disagree about it.
 */
export function composeReadiness(
  ctx: GenerationContext | undefined,
  provider?: {
    displayName: string;
    local: boolean;
    reachable: boolean | null;
    error?: string | null;
  },
): {
  ready: boolean;
  reason: string | null;
} {
  if (!ctx) return { ready: false, reason: null };
  // A model that does not answer is the one thing that makes the button a lie,
  // so it outranks everything else this function has to say.
  if (provider && provider.reachable === false) {
    return {
      ready: false,
      reason: provider.local
        ? `${provider.displayName} is not answering. Start it, or point Mimic at a different endpoint in Settings.${provider.error ? ` (${provider.error})` : ""}`
        : `${provider.displayName} is not answering.${provider.error ? ` (${provider.error})` : ""}`,
    };
  }
  if (!ctx.effective.measurable) {
    return {
      ready: true,
      reason:
        "Mimic has not seen enough of your writing to describe your style yet, so this draft will be plain rather than yours.",
    };
  }
  if (ctx.voice.layers.some((l) => l.stale)) {
    return {
      ready: true,
      reason: "Messages have been imported since your profile was built. Re-analyze to use them.",
    };
  }
  return { ready: true, reason: null };
}
