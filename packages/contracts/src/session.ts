/**
 * Sessions, predictions, apply batches and review (mimic-core::sessions).
 */
import { z } from "zod";
import { Asset } from "./library";
import { ModelVersion } from "./style";

export const SessionSource = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("folder"), path: z.string() }),
  z.object({
    kind: z.literal("lightroom"),
    scope: z.enum(["selection", "folder", "collection", "catalog"]),
  }),
]);
export type SessionSource = z.infer<typeof SessionSource>;

export const SessionStatus = z.enum([
  "new",
  "ingesting",
  "ingest_failed",
  "empty",
  "ingested",
  "grouped",
  "predicted",
  "applying",
  "applied",
]);
export type SessionStatus = z.infer<typeof SessionStatus>;

export const Session = z.object({
  id: z.string(),
  name: z.string(),
  sourcePath: z.string().nullable(),
  sourceLibraryId: z.string().nullable(),
  capturedStart: z.string().nullable(),
  capturedEnd: z.string().nullable(),
  status: z.string(),
  activeStyleProfileId: z.string().nullable(),
  createdAt: z.string(),
  assetCount: z.number(),
});
export type Session = z.infer<typeof Session>;

export const SceneCluster = z.object({
  id: z.string(),
  sessionId: z.string(),
  label: z.string(),
  centroidArtifactId: z.string().nullable(),
  featureSummary: z.record(z.string(), z.unknown()),
  createdAt: z.string(),
  assetCount: z.number(),
});
export type SceneCluster = z.infer<typeof SceneCluster>;

export const PredictionStatus = z.enum([
  "pending",
  "reviewed",
  "applied",
  "rejected",
  "superseded",
]);
export type PredictionStatus = z.infer<typeof PredictionStatus>;

/** One predicted control: normalized value + raw Lightroom value. */
export const PredictedControl = z.object({
  raw: z.unknown(),
  value: z.number().nullish(),
  sourceKey: z.string(),
});

export const PredictedSettings = z.object({
  schemaVersion: z.string().optional(),
  mappingVersion: z.string().optional(),
  global: z.record(z.string(), z.record(z.string(), PredictedControl)).default({}),
});
export type PredictedSettings = z.infer<typeof PredictedSettings>;

/** Emitted by engine/confidence/score.py; each 0..1 unless noted. */
export const ConfidenceComponents = z
  .object({
    similarity: z.number().nullish(),
    density: z.number().nullish(),
    agreement: z.number().nullish(),
    validation: z.number().nullish(),
    coverage: z.number().nullish(),
    meanNeighbourDistance: z.number().nullish(),
    oodZ: z.number().nullish(),
    cameraKnown: z.boolean().nullish(),
    lensKnown: z.boolean().nullish(),
  })
  .passthrough();

export const CONFIDENCE_COMPONENT_LABELS: Record<string, string> = {
  similarity: "Similarity to training photos",
  density: "Neighbourhood density",
  agreement: "Neighbour agreement",
  validation: "Model accuracy on this family",
  coverage: "Camera / ISO / lens coverage",
};
export type ConfidenceComponents = z.infer<typeof ConfidenceComponents>;

export const Prediction = z.object({
  id: z.string(),
  sessionId: z.string(),
  assetId: z.string(),
  modelVersionId: z.string(),
  predictedSettings: PredictedSettings,
  rawModelOutput: z
    .object({
      reasons: z.array(z.string()).optional(),
      ood: z.boolean().optional(),
      consistencyShift: z.number().optional(),
    })
    .passthrough(),
  confidence: z.number(),
  confidenceComponents: ConfidenceComponents,
  nearestExamples: z.array(z.unknown()),
  createdAt: z.string(),
  status: PredictionStatus,
  capabilitySchemaVersion: z.string().nullable(),
  clusterId: z.string().nullable(),
});
export type Prediction = z.infer<typeof Prediction>;

export const ApplyBatchStatus = z.enum([
  "running",
  "completed",
  "completed_with_failures",
  "failed",
  "canceled",
]);

export const ApplyBatch = z.object({
  id: z.string(),
  sessionId: z.string(),
  lightroomCatalogFingerprint: z.string().nullable(),
  startedAt: z.string(),
  completedAt: z.string().nullable(),
  status: z.string(),
  appliedCount: z.number(),
  failedCount: z.number(),
  rollbackAvailable: z.boolean(),
  error: z.unknown().nullable(),
});
export type ApplyBatch = z.infer<typeof ApplyBatch>;

export const AppliedEditResult = z.enum(["applied", "verify_failed", "failed", "skipped"]);
export const RestoreResult = z.enum(["restored", "verify_failed", "failed", "skipped"]);

export const AppliedEdit = z.object({
  id: z.string(),
  applyBatchId: z.string(),
  predictionId: z.string(),
  assetId: z.string(),
  beforeSettings: z.record(z.string(), z.unknown()).nullable(),
  appliedSettings: z.record(z.string(), z.unknown()),
  lightroomSnapshotName: z.string().nullable(),
  result: AppliedEditResult,
  error: z.unknown().nullable(),
  appliedAt: z.string(),
  restoredAt: z.string().nullable(),
  restoreResult: RestoreResult.nullable(),
  restoreError: z.unknown().nullable(),
});
export type AppliedEdit = z.infer<typeof AppliedEdit>;

export const CorrectionSync = z.object({
  id: z.string(),
  sessionId: z.string(),
  lightroomCatalogFingerprint: z.string().nullable(),
  syncedAt: z.string(),
  checkedCount: z.number(),
  untouchedCount: z.number(),
  correctedCount: z.number(),
  unresolvedCount: z.number(),
});
export type CorrectionSync = z.infer<typeof CorrectionSync>;

export const SessionDetail = z.object({
  session: Session,
  source: SessionSource.nullable(),
  clusters: z.array(SceneCluster),
  predictionCounts: z.record(z.string(), z.number()),
  batches: z.array(ApplyBatch),
  correctionSyncs: z.array(CorrectionSync),
  photoCount: z.number(),
  photosWithFeatures: z.number(),
  grouped: z.boolean(),
});
export type SessionDetail = z.infer<typeof SessionDetail>;

export const SessionPhoto = z.object({
  asset: Asset,
  sequenceIndex: z.number(),
  clusterId: z.string().nullable(),
  burstId: z.string().nullable(),
  previewPath: z.string().nullable(),
  prediction: Prediction.nullable(),
  lastApply: AppliedEdit.nullable(),
});
export type SessionPhoto = z.infer<typeof SessionPhoto>;

export const ApplyPreflight = z.object({
  ok: z.boolean(),
  blockers: z.array(z.string()),
  warnings: z.array(z.string()),
  lightroomConnected: z.boolean(),
  catalogFingerprint: z.string().nullable(),
  capabilitySchemaVersion: z.string().nullable(),
  candidateCount: z.number(),
  staleCount: z.number(),
  unresolvedCount: z.number(),
  writableControls: z.number(),
  batchSize: z.number(),
});
export type ApplyPreflight = z.infer<typeof ApplyPreflight>;

export const PredictionDetail = z.object({
  prediction: Prediction,
  asset: Asset.nullable(),
  beforeSnapshot: z.unknown().nullable(),
  modelVersion: ModelVersion.nullable(),
});
export type PredictionDetail = z.infer<typeof PredictionDetail>;

/** Default review threshold (settings key `review.mediumThreshold`): below this a prediction "needs attention". */
export const DEFAULT_REVIEW_THRESHOLD = 0.6;
/** Default high-confidence threshold (settings key `review.highThreshold`). */
export const DEFAULT_HIGH_THRESHOLD = 0.8;

export type ConfidenceBand = "high" | "medium" | "low";
export function confidenceBand(
  c: number,
  threshold = DEFAULT_REVIEW_THRESHOLD,
  high = DEFAULT_HIGH_THRESHOLD,
): ConfidenceBand {
  if (c >= Math.max(threshold, high)) return "high";
  if (c >= threshold) return "medium";
  return "low";
}

/** True when a photo should surface in the attention-only review queue. */
export function needsAttention(photo: SessionPhoto, threshold = DEFAULT_REVIEW_THRESHOLD): boolean {
  const p = photo.prediction;
  if (!p) return false;
  if (p.status === "rejected" || p.status === "superseded") return false;
  if (
    photo.lastApply &&
    (photo.lastApply.result === "verify_failed" || photo.lastApply.result === "failed")
  )
    return true;
  if (p.status === "applied") return false;
  return p.confidence < threshold || p.rawModelOutput.ood === true;
}

/** Flat list of predicted controls with labels, sorted by family then name. */
export function predictedControlRows(
  settings: PredictedSettings,
): { canonical: string; family: string; short: string; raw: unknown; value: number | null }[] {
  const rows: {
    canonical: string;
    family: string;
    short: string;
    raw: unknown;
    value: number | null;
  }[] = [];
  for (const [family, controls] of Object.entries(settings.global ?? {})) {
    for (const [short, cv] of Object.entries(controls)) {
      rows.push({
        canonical: `${family}.${short}`,
        family,
        short,
        raw: cv.raw,
        value: cv.value ?? null,
      });
    }
  }
  return rows.sort((a, b) => a.canonical.localeCompare(b.canonical));
}
