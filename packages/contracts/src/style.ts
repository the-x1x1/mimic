import { z } from "zod";
import { DataQualityReport } from "./library";

export const ModelVersion = z.object({
  id: z.string(),
  styleProfileId: z.string(),
  semanticVersion: z.string(),
  modelType: z.string(),
  featureSchemaVersion: z.string(),
  editSchemaVersion: z.string(),
  trainingSetId: z.string().nullable(),
  trainingConfig: z.unknown(),
  metrics: z.unknown(),
  artifactManifest: z.unknown(),
  createdAt: z.string(),
  status: z.enum(["training", "ready", "failed", "archived"]),
  isActive: z.boolean(),
});
export type ModelVersion = z.infer<typeof ModelVersion>;

export const StyleProfile = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string().nullable(),
  createdAt: z.string(),
  updatedAt: z.string(),
  activeModelVersionId: z.string().nullable(),
  status: z.string(),
  libraryIds: z.array(z.string()),
});

export const StyleSummary = StyleProfile.extend({
  trainingExamples: z.number(),
  cameras: z.array(z.string()),
  activeVersion: ModelVersion.nullable(),
  versionCount: z.number(),
});
export type StyleSummary = z.infer<typeof StyleSummary>;

export const StyleDetail = z.object({
  style: StyleSummary,
  libraries: z.array(DataQualityReport),
  versions: z.array(ModelVersion),
  training: z.object({
    available: z.boolean(),
    reason: z.string(),
    pairs: z.number(),
    minPairs: z.number(),
    inProgress: z.boolean(),
  }),
});
export type StyleDetail = z.infer<typeof StyleDetail>;

/** Metrics document written by the trainer (loosely typed; see docs/ML_PIPELINE.md). */
export const MetricSet = z.object({
  n: z.number(),
  overall: z
    .object({ n: z.number(), nMae: z.number().nullable(), p90: z.number().nullable() })
    .optional(),
  perFamily: z
    .record(z.string(), z.object({ n: z.number(), nMae: z.number(), p90: z.number() }))
    .optional(),
  perControl: z.record(z.string(), z.record(z.string(), z.unknown())).optional(),
});

/** Holdout (or validation) hybrid nMAE, mirroring mimic_core::training::primary_error. */
export function primaryError(metrics: unknown): number | null {
  if (!metrics || typeof metrics !== "object") return null;
  const m = metrics as Record<
    string,
    { n?: number; hybrid?: { overall?: { nMae?: number | null } } } | undefined
  >;
  for (const set of ["holdout", "validation"] as const) {
    const s = m[set];
    if (s && (s.n ?? 0) > 0) {
      const v = s.hybrid?.overall?.nMae;
      if (typeof v === "number") return v;
    }
  }
  return null;
}

export function evaluationSet(metrics: unknown): "holdout" | "validation" | null {
  if (!metrics || typeof metrics !== "object") return null;
  const m = metrics as Record<string, { n?: number } | undefined>;
  if ((m["holdout"]?.n ?? 0) > 0) return "holdout";
  if ((m["validation"]?.n ?? 0) > 0) return "validation";
  return null;
}

/** Holdout MAE of one control in raw units, e.g. tone.exposure in stops. */
export function controlMae(metrics: unknown, model: string, control: string): number | null {
  const set = evaluationSet(metrics);
  if (!set) return null;
  const m = metrics as Record<
    string,
    Record<string, { perControl?: Record<string, { mae?: number }> }>
  >;
  const v = m[set]?.[model]?.perControl?.[control]?.mae;
  return typeof v === "number" ? v : null;
}
