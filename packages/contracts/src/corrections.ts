/**
 * Continuous learning (mimic-core::corrections): what the photographer
 * changed after an apply, and the No-Touch Rate derived from it.
 */
import { z } from "zod";

export const ControlDelta = z.object({
  canonical: z.string(),
  predictedRaw: z.unknown(),
  correctedRaw: z.unknown(),
  delta: z.number().nullable(),
});
export type ControlDelta = z.infer<typeof ControlDelta>;

export const CorrectionRow = z.object({
  id: z.string(),
  assetId: z.string(),
  predictionId: z.string(),
  modelVersionId: z.string(),
  predictedSettings: z.unknown(),
  correctedSettings: z.unknown(),
  delta: z.array(ControlDelta),
  correctionMagnitude: z.number(),
  observedAt: z.string(),
  includedInTrainingVersion: z.string().nullable(),
  sessionId: z.string(),
  fileName: z.string(),
  semanticVersion: z.string(),
});
export type CorrectionRow = z.infer<typeof CorrectionRow>;

export const NoTouchStats = z.object({
  modelVersionId: z.string(),
  semanticVersion: z.string(),
  appliedChecked: z.number(),
  corrected: z.number(),
  untouched: z.number(),
  rate: z.number().nullable(),
});
export type NoTouchStats = z.infer<typeof NoTouchStats>;

export const ControlInsight = z.object({
  canonical: z.string(),
  corrections: z.number(),
  meanAbsDelta: z.number(),
  meanDelta: z.number(),
});

export const StyleHealth = z.object({
  styleId: z.string(),
  noTouch: z.array(NoTouchStats),
  activeNoTouchRate: z.number().nullable(),
  correctionsTotal: z.number(),
  correctionsPendingTraining: z.number(),
  mostCorrected: z.array(ControlInsight),
  insights: z.array(z.string()),
});
export type StyleHealth = z.infer<typeof StyleHealth>;

export function formatNoTouch(rate: number | null | undefined): string {
  return rate === null || rate === undefined ? "—" : `${Math.round(rate * 100)}%`;
}
