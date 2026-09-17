import { z } from "zod";

export const LibrarySourceType = z.enum(["lightroom_catalog", "folder_sidecars", "demo"]);
export type LibrarySourceType = z.infer<typeof LibrarySourceType>;

export const Library = z.object({
  id: z.string(),
  name: z.string(),
  sourceType: LibrarySourceType,
  rootPath: z.string().nullable(),
  lightroomCatalogFingerprint: z.string().nullable(),
  createdAt: z.string(),
  lastScannedAt: z.string().nullable(),
  status: z.string(),
});
export type Library = z.infer<typeof Library>;

export const LibrarySummary = Library.extend({
  assetCount: z.number(),
  validPairCount: z.number(),
});
export type LibrarySummary = z.infer<typeof LibrarySummary>;

export const CountRow = z.object({ label: z.string(), count: z.number() });

export const Recommendation = z.object({
  level: z.enum(["insufficient", "minimal", "good", "strong"]),
  headline: z.string(),
  detail: z.string(),
});

export const DataQualityReport = z.object({
  libraryId: z.string(),
  assetsFound: z.number(),
  validPairs: z.number(),
  missingEdits: z.number(),
  featuresComputed: z.number(),
  lightroomConnectedPairs: z.number(),
  sidecarOnlyPairs: z.number(),
  acrHeavyEditCount: z.number(),
  localEditCount: z.number(),
  failedSidecars: z.number(),
  cameras: z.array(CountRow),
  captureDays: z.array(CountRow),
  recommendation: Recommendation,
  warnings: z.array(z.string()),
});
export type DataQualityReport = z.infer<typeof DataQualityReport>;

export const Asset = z.object({
  id: z.string(),
  libraryId: z.string().nullable(),
  sourcePath: z.string(),
  normalizedPath: z.string(),
  fileName: z.string(),
  extension: z.string(),
  mimeType: z.string().nullable(),
  sizeBytes: z.number(),
  modifiedTime: z.string().nullable(),
  fastHash: z.string(),
  fullHash: z.string().nullable(),
  cameraMake: z.string().nullable(),
  cameraModel: z.string().nullable(),
  lens: z.string().nullable(),
  focalLength: z.number().nullable(),
  iso: z.number().nullable(),
  aperture: z.number().nullable(),
  shutterSpeed: z.number().nullable(),
  capturedAt: z.string().nullable(),
  width: z.number().nullable(),
  height: z.number().nullable(),
  orientation: z.number().nullable(),
  lightroomLocalId: z.number().nullable(),
  createdAt: z.string(),
  updatedAt: z.string(),
});
export type Asset = z.infer<typeof Asset>;

export const AssetRow = Asset.extend({
  hasEdits: z.boolean(),
  hasFeatures: z.boolean(),
  previewPath: z.string().nullable(),
  editSource: z.string().nullable(),
});
export type AssetRow = z.infer<typeof AssetRow>;

export const AssetDetail = z.object({
  asset: Asset,
  sidecars: z.array(z.unknown()),
  snapshots: z.array(z.unknown()),
  features: z.unknown().nullable(),
  editDna: z.unknown().nullable(),
});
export type AssetDetail = z.infer<typeof AssetDetail>;
