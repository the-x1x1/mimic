# ML pipeline

## Implemented in 0.1 (feature side)

- **Preview**: embedded RAW preview via LibRaw (rawpy) when ≥ 480 px, else half-size demosaic with camera WB and no auto-brighten; Pillow for JPEG/TIFF; EXIF-transposed; capped at 768 px; cached as JPEG keyed by `fastHash + preview_v1`. Previews are for features only, never a Lightroom rendering.
- **Statistics `features_v1`** (`engine/src/mimic_engine/features/stats.py`): 32-bin luminance histogram, luminance mean/std/percentiles (1/5/25/50/75/95/99), dynamic range, centre-vs-border delta, channel means/percentiles, gray-world gains, red-green and blue-yellow cast, saturation mean/p95, sky-like fraction, highlight/shadow clipping, Laplacian sharpness, MAD noise estimate. `flat_vector` yields a fixed-order 61-d vector with EXIF terms (log ISO, aperture, log shutter, log focal, EV bias).
- **Scene heuristics `scene_heuristics_v1`**: soft confidences for lowLight, highKey, lowKey, backlit, skyDominant, landscape, portraitLike, closeUp, outdoor, indoor; `primary` only when ≥ 0.5. Soft features, never a single point of failure.
- **Embeddings**: provider `stats_v1` (64-d layout/colour grid, L2-normalised) always available; `onnx:<id>@<version>` when a manifest in `models/manifests/` names a model present and SHA-256-verified in `models/encoders/` (DirectML preferred on Windows, CPU fallback). Stored as `.npy` + provider sidecar in `cache/embeddings/`; different providers are never mixed.

## Training (0.2.0)

`engine/src/mimic_engine/training/`:

- **Dataset** (`dataset.py`): read-only SQLite; one pair per asset with the latest observed snapshot (`xmp`/`lightroom_sdk`/`correction`) and `features_v1` features. Targets = normalized `value` of every predictable numeric control from `edit_mapping_v1` with a presence mask. Filters: no snapshot, no features, no meaningful edits (all controls at default), more than 40 unknown keys, unusable features. Embeddings are used only if every pair has one from the same provider.
- **Split** (`split.py`): group key = library + capture day (fallback: folder). ≥ 3 groups → deterministic seeded group assignment, at least one group in holdout and validation, sizes filled toward 70/15/15. 2 groups → train + holdout, warning. 1 group → time-ordered split, warning "metrics are optimistic". Never random across a shoot.
- **Models** (`baselines.py`): `GlobalMedian`, `ConditionedMedian` (camera+lens → camera → global, min support 5), `KnnRegressor` (k=8, 1/(d+1e-3) weights, masked targets, standardized features + optional embedding block), `RidgeResidual` (closed-form per control, fitted on leave-one-out KNN residuals, skipped for controls with thin support), `HybridModel` = clip(KNN + residual, 0..1).
- **Metrics** (`evaluation/metrics.py`): per control nMAE/nRMSE plus MAE/RMSE/p50/p90/p95 in raw units; per family; overall. `acceptanceProxy` = fraction of photos with every present high-impact control (WB, tone, presence) within 0.05 normalized — explicitly a proxy, never shown as No-Touch Rate.
- **Confidence** (`confidence/score.py`): calibration = distances of validation+holdout photos to the train set (median, p90, max, OOD threshold = max(1.75·p90, 1.15·max)), family validation nMAE, camera/lens sets, ISO range. Score = 0.35 similarity + 0.15 density + 0.20 component agreement + 0.15 validation + 0.15 coverage, capped at 0.49 when OOD. Components and reasons are stored with every prediction.
- **Artifacts**: `<styles>/<style_id>/<model_version_id>/{model.joblib, training_config.json, metrics.json}`, SHA-256 hashed and registered in `model_artifacts`. Never overwritten; a new version gets a new directory.
- **Reproducibility**: seed, k, alpha, embedding weight, split fractions, feature/target schemas, mapping version, data fingerprint (SHA-256 over asset ids + targets), dependency versions and timestamp in `training_config.json`. Same DB + config ⇒ identical metrics (tested).
- **Activation** (mimic-core `training/mod.rs`): first version activates; a later version activates automatically only when its holdout hybrid nMAE ≤ the active version's; otherwise it stays `ready` and the reason is reported. Manual activation is the rollback path.

## Prediction (engine `model.predict`)

Loads the version's `model.joblib`, rebuilds the feature vector from stored stats + EXIF, requires an embedding only when the model was trained with one, returns per asset: canonical global settings (normalized + raw via the mapping ranges), confidence + components + OOD + reasons, nearest training assets with distances, and raw component outputs. Missing assets or features produce per-item errors, never a failed batch.

## Training data filters (§41)

Applied: missing snapshot, missing features, no meaningful edits, unknown-key threshold, bad feature rows. Not yet exposed as user controls (camera/date/collection/rating exclusions are roadmap).
