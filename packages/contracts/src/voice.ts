import { z } from "zod";

export const VoiceLayer = z.enum(["global", "channel", "relationship", "situational"]);
export type VoiceLayer = z.infer<typeof VoiceLayer>;

/**
 * Every rate is nullable on purpose. `null` means "not measured"; `0` means
 * "measured, and it is zero". The UI must not render the two the same way.
 */
export const VoiceMetrics = z.object({
  sampleSize: z.number(),
  measurable: z.boolean(),
  avgWordsPerMessage: z.number().nullable(),
  medianWordsPerMessage: z.number().nullable(),
  p90WordsPerMessage: z.number().nullable(),
  avgSentencesPerMessage: z.number().nullable(),
  multiParagraphRate: z.number().nullable(),
  terminalPeriodRate: z.number().nullable(),
  questionRate: z.number().nullable(),
  exclamationRate: z.number().nullable(),
  ellipsisRate: z.number().nullable(),
  emojiRate: z.number().nullable(),
  lowercaseStartRate: z.number().nullable(),
  allLowercaseRate: z.number().nullable(),
  contractionsPer100Words: z.number().nullable(),
  greetingRate: z.number().nullable(),
  signOffRate: z.number().nullable(),
  topGreetings: z.array(z.tuple([z.string(), z.number()])),
  topSignOffs: z.array(z.tuple([z.string(), z.number()])),
  topPhrases: z.array(z.tuple([z.string(), z.number()])),
  medianResponseSeconds: z.number().nullable(),
});
export type VoiceMetrics = z.infer<typeof VoiceMetrics>;

/**
 * A model's reading of a layer's numbers, in words — written from the
 * numbers alone, and shown beside them as a reading, never as something the
 * user said. `current` is whether it was written from the numbers the layer
 * has now; a reading of older numbers is shown as that and given to no draft.
 */
export const VoiceReading = z.object({
  text: z.string(),
  model: z.string(),
  provider: z.string(),
  writtenAt: z.string(),
  current: z.boolean(),
});
export type VoiceReading = z.infer<typeof VoiceReading>;

export const ResolvedLayer = z.object({
  layer: z.string(),
  scopeKey: z.string(),
  label: z.string(),
  sampleSize: z.number(),
  measurable: z.boolean(),
  metrics: VoiceMetrics,
  stale: z.boolean(),
  reading: VoiceReading.nullable(),
});
export type ResolvedLayer = z.infer<typeof ResolvedLayer>;

export const VoiceOverview = z.object({
  analysisVersion: z.string(),
  ownMessages: z.number(),
  channels: z.array(z.tuple([z.string(), z.number()])),
  profiles: z.array(ResolvedLayer),
  peopleWithProfiles: z.number(),
  stale: z.boolean(),
  lastAnalyzedAt: z.string().nullable(),
  messagesUntilMeasurable: z.number(),
});
export type VoiceOverview = z.infer<typeof VoiceOverview>;

export const RepresentativeExample = z.object({
  id: z.string(),
  messageId: z.string(),
  layer: z.string(),
  scopeKey: z.string(),
  participantId: z.string().nullable(),
  reason: z.string(),
  score: z.number(),
  body: z.string(),
  sentAt: z.string().nullable(),
});
export type RepresentativeExample = z.infer<typeof RepresentativeExample>;

export const VoicePreference = z.object({
  id: z.string(),
  layer: z.string(),
  scopeKey: z.string(),
  key: z.string(),
  value: z.unknown(),
  note: z.string().nullable(),
  updatedAt: z.string(),
});
export type VoicePreference = z.infer<typeof VoicePreference>;

/**
 * One situation in the vocabulary — what a reply is doing — with how many of
 * the user's own messages are filed under it. The six ids are fixed; they are
 * seeded by migration 0007 and named in `crates/mimic-core/src/situations.rs`.
 */
export const SituationSummary = z.object({
  id: z.string(),
  label: z.string(),
  layerLabel: z.string(),
  ownMessages: z.number(),
  measurable: z.boolean(),
});
export type SituationSummary = z.infer<typeof SituationSummary>;

/**
 * How the user's own messages came to be filed by what they are doing — by
 * the rules, by a model on this computer, by the user — and the model on
 * this computer that could read them, if there is one. Only a local model
 * is ever sent them.
 */
export const SituationFiling = z.object({
  byRules: z.number(),
  byModel: z.number(),
  byYou: z.number(),
  /** The provider on this computer that would read them, to check it answers first. */
  localProvider: z.string().nullable(),
  localModel: z.string().nullable(),
});
export type SituationFiling = z.infer<typeof SituationFiling>;

export const Habit = z.enum(["greeting", "signOff", "emoji", "terminalPeriod", "length"]);
export type Habit = z.infer<typeof Habit>;

/**
 * A habit the user pushed one way in drafts they sent. `holds` is whether it
 * passed the threshold (three agreeing drafts carrying most of the weight)
 * and so changes the next draft; the rest are still forming. `summary` is
 * written for a person and shown verbatim.
 */
export const LearnedPattern = z.object({
  habit: Habit,
  direction: z.enum(["less", "more"]),
  participantId: z.string().nullable(),
  participantName: z.string().nullable(),
  agreeing: z.number(),
  observations: z.number(),
  share: z.number(),
  holds: z.boolean(),
  meanChange: z.number().nullable(),
  summary: z.string(),
});
export type LearnedPattern = z.infer<typeof LearnedPattern>;

/** Something the user told Mimic, in their words, and who it is about. */
export const StatedNote = z.object({
  id: z.string(),
  text: z.string(),
  participantId: z.string().nullable(),
  participantName: z.string().nullable(),
  updatedAt: z.string(),
});
export type StatedNote = z.infer<typeof StatedNote>;

export const LearningOverview = z.object({
  draftsConsidered: z.number(),
  patterns: z.array(LearnedPattern),
  minAgreeing: z.number(),
  notes: z.array(StatedNote),
});
export type LearningOverview = z.infer<typeof LearningOverview>;

/** Below this many of the user's own messages nothing is measured. */
export const MIN_SAMPLE = 20;

/** A percentage, or the honest absence of one. Never "0%" for unknown. */
export function formatRate(rate: number | null): string {
  if (rate === null) return "not measured yet";
  return `${Math.round(rate * 100)}%`;
}

/** A measured metric in one readable sentence, or null when unmeasured. */
export function describeMetric(key: keyof VoiceMetrics, m: VoiceMetrics): string | null {
  if (!m.measurable) return null;
  switch (key) {
    case "medianWordsPerMessage":
      return m.medianWordsPerMessage === null
        ? null
        : `${Math.round(m.medianWordsPerMessage)} words in a typical message`;
    case "terminalPeriodRate":
      return m.terminalPeriodRate === null
        ? null
        : `${formatRate(m.terminalPeriodRate)} of messages end with a full stop`;
    case "emojiRate":
      return m.emojiRate === null
        ? null
        : `${formatRate(m.emojiRate)} of messages contain an emoji`;
    case "greetingRate":
      return m.greetingRate === null ? null : `${formatRate(m.greetingRate)} open with a greeting`;
    case "signOffRate":
      return m.signOffRate === null ? null : `${formatRate(m.signOffRate)} end with a sign-off`;
    case "lowercaseStartRate":
      return m.lowercaseStartRate === null
        ? null
        : `${formatRate(m.lowercaseStartRate)} start in lowercase`;
    default:
      return null;
  }
}

/** Seconds as something a person would say. */
export function formatDuration(seconds: number | null): string {
  if (seconds === null) return "not measured yet";
  if (seconds < 90) return `${Math.round(seconds)} seconds`;
  if (seconds < 5400) return `${Math.round(seconds / 60)} minutes`;
  if (seconds < 172800) return `${Math.round(seconds / 3600)} hours`;
  return `${Math.round(seconds / 86400)} days`;
}

/** The sentence encoder this build offers, as its manifest names it. */
export const OfferedEncoder = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string(),
  bytes: z.number(),
  license: z.string(),
  homepage: z.string(),
});
export type OfferedEncoder = z.infer<typeof OfferedEncoder>;

/**
 * Reading messages for meaning: the encoder offered (none in a build without
 * one), whether it is downloaded and in use — the engine runs it only while
 * every file matches its pinned SHA-256 — the engine's own word on why, and
 * how many of the messages drafts compare by meaning it has read.
 */
export const EncoderView = z.object({
  offered: OfferedEncoder.nullable(),
  downloaded: z.boolean(),
  inUse: z.boolean(),
  reason: z.string().nullable(),
  wanted: z.number(),
  done: z.number(),
});
export type EncoderView = z.infer<typeof EncoderView>;
