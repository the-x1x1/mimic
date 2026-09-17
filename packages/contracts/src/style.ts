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
  training: z.object({ available: z.boolean(), reason: z.string() }),
});
export type StyleDetail = z.infer<typeof StyleDetail>;
