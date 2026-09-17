# EditDNA — canonical edit representation

EditDNA is Mimic's Lightroom-version-neutral description of a photograph's develop state. The mapping below is **data**: `packages/contracts/edit_mapping_v1.json` is loaded by mimic-core (Rust, `include_str!`), by the TypeScript contracts, and read by the Python engine. Golden fixtures (`fixtures/expected/*.raw.json` → `*.normalized.json`) pin the behaviour; a mapping change that alters any golden fails CI in all three languages.

Mapping version: `edit_mapping_v1` · schema version: `1.0` · controls: 104

## Document shape

```json
{
  "schemaVersion": "1.0",
  "asset": {
    "id": "\u2026",
    "camera": {
      "make": "\u2026",
      "model": "\u2026",
      "lens": "\u2026"
    },
    "capture": {
      "focalLength": 50,
      "iso": 400,
      "aperture": 2.8,
      "shutterSpeed": 0.005,
      "capturedAt": "\u2026"
    },
    "dimensions": {
      "width": 6000,
      "height": 4000,
      "orientation": 1
    }
  },
  "input": {
    "sceneFeaturesVersion": "features_v1",
    "visualEmbeddingRef": "emb_\u2026.npy",
    "histogram": {},
    "luminance": {},
    "color": {},
    "noise": {},
    "sharpness": {},
    "clipping": {},
    "scene": {},
    "sessionContext": {}
  },
  "lightroom": {
    "processVersion": "15.4",
    "crsVersion": "17.0",
    "source": "lightroom_sdk|xmp",
    "capabilitySchemaVersion": "cap_\u2026",
    "heavyEditKeys": []
  },
  "target": {
    "global": {
      "tone": {
        "exposure": {
          "raw": 0.35,
          "value": 0.535,
          "sourceKey": "Exposure2012"
        }
      }
    },
    "local": {
      "status": "unsupported|observed",
      "observedKeys": [],
      "operations": []
    }
  },
  "unknownLightroomSettings": {},
  "provenance": {
    "sourceSnapshotId": "\u2026",
    "capturedAt": "\u2026",
    "mappingVersion": "edit_mapping_v1",
    "rawSettingsHash": "sha256\u2026"
  }
}
```

## Rules

- Every numeric control stores `raw` (as parsed) **and** `value` (normalized 0..1 over the control's range; `log` scale for Kelvin temperature). `sourceKey` records which Lightroom key supplied it (`Exposure2012` vs legacy `Exposure`).
- **Not present** = the control key is absent from `target.global`. **Zero** = present with `raw: 0`. **Unsupported** is a capability-matrix state, never encoded in the document.
- Unknown or unparsable keys are never dropped: they land in `unknownLightroomSettings` with a warning. Local/mask payloads (prefixes below) are additionally listed in `target.local.observedKeys` and set `status: observed`. Heavy-edit keys (Look tables, LensBlur, PointColors, AI denoise…) are listed in `lightroom.heavyEditKeys`.
- First matching key wins per control (modern key listed first), so PV 2012+ files use `*2012` keys and PV 2010 files fall back to legacy names (`FillLight` → `tone.shadows`, `Shadows` → `tone.blacks`, `HighlightRecovery` → `tone.highlights`).
- Canonical names are Lightroom-version-neutral. Adding a control means a new mapping version; existing controls are never changed in place after a release.
- Writing back (`to_lightroom_settings`) only emits keys the capability matrix marks writable; skipped controls are reported with a reason. Read-back verification uses 0.5 % of range for floats, exact for ints/bools/enums, structural for curves.

## Families

| Family         | Label             | Predictable in 0.x | Note                                          |
| -------------- | ----------------- | ------------------ | --------------------------------------------- |
| `whiteBalance` | White Balance     | yes                |                                               |
| `tone`         | Basic Tone        | yes                |                                               |
| `presence`     | Presence          | yes                |                                               |
| `hsl`          | HSL / Color Mixer | yes                |                                               |
| `toneCurve`    | Tone Curve        | yes                | point curves are stored, not predicted in 0.x |
| `colorGrading` | Color Grading     | yes                |                                               |
| `detail`       | Detail            | yes                |                                               |
| `lens`         | Lens Corrections  | no                 |                                               |
| `effects`      | Effects           | yes                |                                               |
| `calibration`  | Calibration       | yes                |                                               |
| `crop`         | Crop              | no                 | stored only; crop prediction is roadmap       |

## Lightroom metadata keys (not controls)

`processVersion` → ProcessVersion, `crsVersion` → Version, `hasSettings` → HasSettings, `alreadyApplied` → AlreadyApplied, `rawFileName` → RawFileName

## Local-correction prefixes (observed, never written)

`MaskGroupBasedCorrections`, `GradientBasedCorrections`, `CircularGradientBasedCorrections`, `PaintBasedCorrections`, `RetouchAreas`, `RetouchInfo`, `RedEyeInfo`

## Heavy-edit prefixes (preserved, flagged)

`Look`, `LookTable`, `RGBTable`, `RGBTableAmount`, `LensBlur`, `DepthBasedCorrections`, `NoiseReductionAI`, `DenoiseAI`, `AutoToneDigest`, `LensBlurRaw`, `PointColors`

## Controls

| Canonical                                   | Lightroom keys                           | Type   | Range       | Default | Normalize | Process versions       |
| ------------------------------------------- | ---------------------------------------- | ------ | ----------- | ------- | --------- | ---------------------- |
| `whiteBalance.mode`                         | `WhiteBalance`                           | enum   | —           | —       | none      | all                    |
| `whiteBalance.temperature`                  | `Temperature`                            | int    | 2000..50000 | —       | log       | all                    |
| `whiteBalance.tint`                         | `Tint`                                   | int    | -150..150   | 0       | linear    | all                    |
| `tone.exposure`                             | `Exposure2012`, `Exposure`               | float  | -5.0..5.0   | 0.0     | linear    | 6.7, 11.0, 15.4, 2012+ |
| `tone.contrast`                             | `Contrast2012`, `Contrast`               | int    | -100..100   | 0       | linear    | 6.7, 11.0, 15.4, 2012+ |
| `tone.highlights`                           | `Highlights2012`, `HighlightRecovery`    | int    | -100..100   | 0       | linear    | 6.7, 11.0, 15.4, 2012+ |
| `tone.shadows`                              | `Shadows2012`, `FillLight`               | int    | -100..100   | 0       | linear    | 6.7, 11.0, 15.4, 2012+ |
| `tone.whites`                               | `Whites2012`                             | int    | -100..100   | 0       | linear    | 6.7, 11.0, 15.4, 2012+ |
| `tone.blacks`                               | `Blacks2012`, `Shadows`                  | int    | -100..100   | 0       | linear    | 6.7, 11.0, 15.4, 2012+ |
| `presence.texture`                          | `Texture`                                | int    | -100..100   | 0       | linear    | all                    |
| `presence.clarity`                          | `Clarity2012`, `Clarity`                 | int    | -100..100   | 0       | linear    | 6.7, 11.0, 15.4, 2012+ |
| `presence.dehaze`                           | `Dehaze`                                 | int    | -100..100   | 0       | linear    | all                    |
| `presence.vibrance`                         | `Vibrance`                               | int    | -100..100   | 0       | linear    | all                    |
| `presence.saturation`                       | `Saturation`                             | int    | -100..100   | 0       | linear    | all                    |
| `hsl.hue.red`                               | `HueAdjustmentRed`                       | int    | -100..100   | 0       | linear    | all                    |
| `hsl.saturation.red`                        | `SaturationAdjustmentRed`                | int    | -100..100   | 0       | linear    | all                    |
| `hsl.luminance.red`                         | `LuminanceAdjustmentRed`                 | int    | -100..100   | 0       | linear    | all                    |
| `hsl.hue.orange`                            | `HueAdjustmentOrange`                    | int    | -100..100   | 0       | linear    | all                    |
| `hsl.saturation.orange`                     | `SaturationAdjustmentOrange`             | int    | -100..100   | 0       | linear    | all                    |
| `hsl.luminance.orange`                      | `LuminanceAdjustmentOrange`              | int    | -100..100   | 0       | linear    | all                    |
| `hsl.hue.yellow`                            | `HueAdjustmentYellow`                    | int    | -100..100   | 0       | linear    | all                    |
| `hsl.saturation.yellow`                     | `SaturationAdjustmentYellow`             | int    | -100..100   | 0       | linear    | all                    |
| `hsl.luminance.yellow`                      | `LuminanceAdjustmentYellow`              | int    | -100..100   | 0       | linear    | all                    |
| `hsl.hue.green`                             | `HueAdjustmentGreen`                     | int    | -100..100   | 0       | linear    | all                    |
| `hsl.saturation.green`                      | `SaturationAdjustmentGreen`              | int    | -100..100   | 0       | linear    | all                    |
| `hsl.luminance.green`                       | `LuminanceAdjustmentGreen`               | int    | -100..100   | 0       | linear    | all                    |
| `hsl.hue.aqua`                              | `HueAdjustmentAqua`                      | int    | -100..100   | 0       | linear    | all                    |
| `hsl.saturation.aqua`                       | `SaturationAdjustmentAqua`               | int    | -100..100   | 0       | linear    | all                    |
| `hsl.luminance.aqua`                        | `LuminanceAdjustmentAqua`                | int    | -100..100   | 0       | linear    | all                    |
| `hsl.hue.blue`                              | `HueAdjustmentBlue`                      | int    | -100..100   | 0       | linear    | all                    |
| `hsl.saturation.blue`                       | `SaturationAdjustmentBlue`               | int    | -100..100   | 0       | linear    | all                    |
| `hsl.luminance.blue`                        | `LuminanceAdjustmentBlue`                | int    | -100..100   | 0       | linear    | all                    |
| `hsl.hue.purple`                            | `HueAdjustmentPurple`                    | int    | -100..100   | 0       | linear    | all                    |
| `hsl.saturation.purple`                     | `SaturationAdjustmentPurple`             | int    | -100..100   | 0       | linear    | all                    |
| `hsl.luminance.purple`                      | `LuminanceAdjustmentPurple`              | int    | -100..100   | 0       | linear    | all                    |
| `hsl.hue.magenta`                           | `HueAdjustmentMagenta`                   | int    | -100..100   | 0       | linear    | all                    |
| `hsl.saturation.magenta`                    | `SaturationAdjustmentMagenta`            | int    | -100..100   | 0       | linear    | all                    |
| `hsl.luminance.magenta`                     | `LuminanceAdjustmentMagenta`             | int    | -100..100   | 0       | linear    | all                    |
| `toneCurve.parametric.shadows`              | `ParametricShadows`                      | int    | -100..100   | 0       | linear    | all                    |
| `toneCurve.parametric.darks`                | `ParametricDarks`                        | int    | -100..100   | 0       | linear    | all                    |
| `toneCurve.parametric.lights`               | `ParametricLights`                       | int    | -100..100   | 0       | linear    | all                    |
| `toneCurve.parametric.highlights`           | `ParametricHighlights`                   | int    | -100..100   | 0       | linear    | all                    |
| `toneCurve.parametric.shadowSplit`          | `ParametricShadowSplit`                  | int    | 0..100      | 25      | linear    | all                    |
| `toneCurve.parametric.midtoneSplit`         | `ParametricMidtoneSplit`                 | int    | 0..100      | 50      | linear    | all                    |
| `toneCurve.parametric.highlightSplit`       | `ParametricHighlightSplit`               | int    | 0..100      | 75      | linear    | all                    |
| `toneCurve.point.rgb`                       | `ToneCurvePV2012`, `ToneCurve`           | curve  | —           | —       | none      | 6.7, 11.0, 15.4, 2012+ |
| `toneCurve.point.red`                       | `ToneCurvePV2012Red`, `ToneCurveRed`     | curve  | —           | —       | none      | 6.7, 11.0, 15.4, 2012+ |
| `toneCurve.point.green`                     | `ToneCurvePV2012Green`, `ToneCurveGreen` | curve  | —           | —       | none      | 6.7, 11.0, 15.4, 2012+ |
| `toneCurve.point.blue`                      | `ToneCurvePV2012Blue`, `ToneCurveBlue`   | curve  | —           | —       | none      | 6.7, 11.0, 15.4, 2012+ |
| `toneCurve.name`                            | `ToneCurveName2012`, `ToneCurveName`     | string | —           | —       | none      | 6.7, 11.0, 15.4, 2012+ |
| `colorGrading.shadow.hue`                   | `SplitToningShadowHue`                   | int    | 0..360      | 0       | linear    | all                    |
| `colorGrading.shadow.saturation`            | `SplitToningShadowSaturation`            | int    | 0..100      | 0       | linear    | all                    |
| `colorGrading.shadow.luminance`             | `ColorGradeShadowLum`                    | int    | -100..100   | 0       | linear    | all                    |
| `colorGrading.midtone.hue`                  | `ColorGradeMidtoneHue`                   | int    | 0..360      | 0       | linear    | all                    |
| `colorGrading.midtone.saturation`           | `ColorGradeMidtoneSat`                   | int    | 0..100      | 0       | linear    | all                    |
| `colorGrading.midtone.luminance`            | `ColorGradeMidtoneLum`                   | int    | -100..100   | 0       | linear    | all                    |
| `colorGrading.highlight.hue`                | `SplitToningHighlightHue`                | int    | 0..360      | 0       | linear    | all                    |
| `colorGrading.highlight.saturation`         | `SplitToningHighlightSaturation`         | int    | 0..100      | 0       | linear    | all                    |
| `colorGrading.highlight.luminance`          | `ColorGradeHighlightLum`                 | int    | -100..100   | 0       | linear    | all                    |
| `colorGrading.global.hue`                   | `ColorGradeGlobalHue`                    | int    | 0..360      | 0       | linear    | all                    |
| `colorGrading.global.saturation`            | `ColorGradeGlobalSat`                    | int    | 0..100      | 0       | linear    | all                    |
| `colorGrading.global.luminance`             | `ColorGradeGlobalLum`                    | int    | -100..100   | 0       | linear    | all                    |
| `colorGrading.blending`                     | `ColorGradeBlending`                     | int    | 0..100      | 50      | linear    | all                    |
| `colorGrading.balance`                      | `SplitToningBalance`                     | int    | -100..100   | 0       | linear    | all                    |
| `detail.sharpness`                          | `Sharpness`                              | int    | 0..150      | 40      | linear    | all                    |
| `detail.sharpenRadius`                      | `SharpenRadius`                          | float  | 0.5..3.0    | 1.0     | linear    | all                    |
| `detail.sharpenDetail`                      | `SharpenDetail`                          | int    | 0..100      | 25      | linear    | all                    |
| `detail.sharpenEdgeMasking`                 | `SharpenEdgeMasking`                     | int    | 0..100      | 0       | linear    | all                    |
| `detail.luminanceNoise`                     | `LuminanceSmoothing`                     | int    | 0..100      | 0       | linear    | all                    |
| `detail.luminanceNoiseDetail`               | `LuminanceNoiseReductionDetail`          | int    | 0..100      | 50      | linear    | all                    |
| `detail.luminanceNoiseContrast`             | `LuminanceNoiseReductionContrast`        | int    | 0..100      | 0       | linear    | all                    |
| `detail.colorNoise`                         | `ColorNoiseReduction`                    | int    | 0..100      | 25      | linear    | all                    |
| `detail.colorNoiseDetail`                   | `ColorNoiseReductionDetail`              | int    | 0..100      | 50      | linear    | all                    |
| `detail.colorNoiseSmoothness`               | `ColorNoiseReductionSmoothness`          | int    | 0..100      | 50      | linear    | all                    |
| `lens.profileEnabled`                       | `LensProfileEnable`                      | bool   | —           | False   | none      | all                    |
| `lens.profileName`                          | `LensProfileName`                        | string | —           | —       | none      | all                    |
| `lens.autoLateralCA`                        | `AutoLateralCA`                          | bool   | —           | False   | none      | all                    |
| `lens.manualDistortion`                     | `LensManualDistortionAmount`             | int    | -100..100   | 0       | linear    | all                    |
| `lens.vignetteAmount`                       | `VignetteAmount`                         | int    | -100..100   | 0       | linear    | all                    |
| `lens.vignetteMidpoint`                     | `VignetteMidpoint`                       | int    | 0..100      | 50      | linear    | all                    |
| `effects.postCropVignetteAmount`            | `PostCropVignetteAmount`                 | int    | -100..100   | 0       | linear    | all                    |
| `effects.postCropVignetteMidpoint`          | `PostCropVignetteMidpoint`               | int    | 0..100      | 50      | linear    | all                    |
| `effects.postCropVignetteFeather`           | `PostCropVignetteFeather`                | int    | 0..100      | 50      | linear    | all                    |
| `effects.postCropVignetteRoundness`         | `PostCropVignetteRoundness`              | int    | -100..100   | 0       | linear    | all                    |
| `effects.postCropVignetteHighlightContrast` | `PostCropVignetteHighlightContrast`      | int    | 0..100      | 0       | linear    | all                    |
| `effects.postCropVignetteStyle`             | `PostCropVignetteStyle`                  | enum   | —           | —       | none      | all                    |
| `effects.grainAmount`                       | `GrainAmount`                            | int    | 0..100      | 0       | linear    | all                    |
| `effects.grainSize`                         | `GrainSize`                              | int    | 0..100      | 25      | linear    | all                    |
| `effects.grainFrequency`                    | `GrainFrequency`                         | int    | 0..100      | 50      | linear    | all                    |
| `calibration.profile`                       | `CameraProfile`                          | string | —           | —       | none      | all                    |
| `calibration.shadowTint`                    | `ShadowTint`                             | int    | -100..100   | 0       | linear    | all                    |
| `calibration.redHue`                        | `RedHue`                                 | int    | -100..100   | 0       | linear    | all                    |
| `calibration.redSaturation`                 | `RedSaturation`                          | int    | -100..100   | 0       | linear    | all                    |
| `calibration.greenHue`                      | `GreenHue`                               | int    | -100..100   | 0       | linear    | all                    |
| `calibration.greenSaturation`               | `GreenSaturation`                        | int    | -100..100   | 0       | linear    | all                    |
| `calibration.blueHue`                       | `BlueHue`                                | int    | -100..100   | 0       | linear    | all                    |
| `calibration.blueSaturation`                | `BlueSaturation`                         | int    | -100..100   | 0       | linear    | all                    |
| `crop.hasCrop`                              | `HasCrop`                                | bool   | —           | False   | none      | all                    |
| `crop.top`                                  | `CropTop`                                | float  | 0.0..1.0    | 0.0     | linear    | all                    |
| `crop.left`                                 | `CropLeft`                               | float  | 0.0..1.0    | 0.0     | linear    | all                    |
| `crop.bottom`                               | `CropBottom`                             | float  | 0.0..1.0    | 1.0     | linear    | all                    |
| `crop.right`                                | `CropRight`                              | float  | 0.0..1.0    | 1.0     | linear    | all                    |
| `crop.angle`                                | `CropAngle`                              | float  | -45.0..45.0 | 0.0     | linear    | all                    |
| `crop.constrainToWarp`                      | `CropConstrainToWarp`                    | bool   | —           | False   | none      | all                    |

## Golden fixtures

| Fixture                    | Era                           | Exercises                                                                                                        |
| -------------------------- | ----------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `simple_pv2012.xmp`        | Lightroom Classic 13, PV 11.0 | attribute form, all four curves, HSL, split toning, calibration                                                  |
| `modern_masks_unknown.xmp` | Lightroom Classic 15, PV 15.4 | structured masks, Look table, LensBlur/PointColors heavy keys, an unknown future slider, crop, foreign namespace |
| `legacy_pv2010.xmp`        | Lightroom 3/4, PV 5.7         | legacy keys, `Brightness` (unknown in canonical), legacy tone curve                                              |
| `no_crs_metadata_only.xmp` | any                           | no develop settings at all                                                                                       |
| `malformed_truncated.xmp`  | —                             | parser must fail this file only, never the scan                                                                  |
