/**
 * Typed loader for edit_mapping_v1.json (the canonical EditDNA <-> Lightroom
 * mapping). The frontend uses it for labels and ranges only; normalization
 * happens in mimic-core.
 */
import { z } from "zod";
import mappingJson from "../edit_mapping_v1.json";

export const MappingControl = z.object({
  canonical: z.string(),
  family: z.string(),
  lightroomKeys: z.array(z.string()).min(1),
  valueType: z.enum(["float", "int", "bool", "string", "enum", "curve"]),
  range: z.object({ min: z.number(), max: z.number() }).optional(),
  default: z.unknown().optional(),
  normalize: z.enum(["linear", "log", "none"]),
  processVersions: z.array(z.string()),
  writable: z.string(),
  note: z.string().optional(),
});
export type MappingControl = z.infer<typeof MappingControl>;

export const EditMapping = z.object({
  mappingVersion: z.string(),
  editDnaSchemaVersion: z.string(),
  families: z.record(
    z.string(),
    z.object({ label: z.string(), predictable: z.boolean(), note: z.string().optional() }),
  ),
  lightroomMetadataKeys: z.record(z.string(), z.array(z.string())),
  localCorrectionKeyPrefixes: z.array(z.string()),
  heavyEditKeyPrefixes: z.array(z.string()),
  controls: z.array(MappingControl),
});
export type EditMapping = z.infer<typeof EditMapping>;

export const editMapping: EditMapping = EditMapping.parse(mappingJson);

export const controlByCanonical = new Map(editMapping.controls.map((c) => [c.canonical, c]));
export const controlByLightroomKey = new Map(
  editMapping.controls.flatMap((c) => c.lightroomKeys.map((k) => [k, c] as const)),
);

export function familyLabel(family: string): string {
  return editMapping.families[family]?.label ?? family;
}

/** Human label for a canonical control, e.g. `tone.exposure` → "Exposure". */
export function controlLabel(canonical: string): string {
  const short = canonical.split(".").slice(1).join(" ");
  return short.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/\b\w/g, (m) => m.toUpperCase());
}
