# ML pipeline

## Implemented in 0.1 (feature side)

- **Preview**: embedded RAW preview via LibRaw (rawpy) when ≥ 480 px, else half-size demosaic with camera WB and no auto-brighten; Pillow for JPEG/TIFF; EXIF-transposed; capped at 768 px; cached as JPEG keyed by `fastHash + preview_v1`. Previews are for features only, never a Lightroom rendering.
- **Statistics `features_v1`** (`engine/src/mimic_engine/features/stats.py`): 32-bin luminance histogram, luminance mean/std/percentiles (1/5/25/50/75/95/99), dynamic range, centre-vs-border delta, channel means/percentiles, gray-world gains, red-green and blue-yellow cast, saturation mean/p95, sky-like fraction, highlight/shadow clipping, Laplacian sharpness, MAD noise estimate. `flat_vector` yields a fixed-order 61-d vector with EXIF terms (log ISO, aperture, log shutter, log focal, EV bias).
- **Scene heuristics `scene_heuristics_v1`**: soft confidences for lowLight, highKey, lowKey, backlit, skyDominant, landscape, portraitLike, closeUp, outdoor, indoor; `primary` only when ≥ 0.5. Soft features, never a single point of failure.
- **Embeddings**: provider `stats_v1` (64-d layout/colour grid, L2-normalised) always available; `onnx:<id>@<version>` when a manifest in `models/manifests/` names a model present and SHA-256-verified in `models/encoders/` (DirectML preferred on Windows, CPU fallback). Stored as `.npy` + provider sidecar in `cache/embeddings/`; different providers are never mixed.

## Planned for 0.2.0 (training)

Training pair X = features + EXIF + session context, Y = normalized canonical settings. Baseline hierarchy: global median → camera/lens median → KNN (distance-weighted) → per-family regressors → hybrid KNN + residual with session consistency. Session-grouped split (70/15/15 by shoot day/collection). Per-control MAE/RMSE/nMAE/p50/p90/p95 and per-family metrics; a version becomes active only if it beats the current active model or the user chooses it. Confidence from neighbour distance, density, disagreement, family validation error, coverage and OOD. All of this is roadmap until it lands with tests.

## Training data filters (0.2.0)

Exclude or flag: missing source image, no meaningful edits (`meaningful_edit_count == 0`), parse failure, unsupported process version without safe mapping, virtual-copy ambiguity, damaged files, too many unknown target keys.
