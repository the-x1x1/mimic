# ROADMAP — future work only

Completed work lives in CHANGELOG.md and PROJECT_STATUS.md, never here.

## 0.3.0 — New session prediction + Lightroom apply

- Sessions: folder or Lightroom selection, scene grouping (time gaps → embedding clusters → colour refinement), session grid with filters.
- Prediction jobs writing `predictions` with confidence and nearest examples; session consistency policy (bounded smoothing per family).
- Review: attention-only default, filmstrip/preview/panel, Apply/Reject/Use as Reference, filters (<80 %, OOD, failures, corrected).
- Apply batches: pre-apply checks (same catalog, capability schema unchanged, prediction fresh), snapshot, plugin-preset apply, read-back, VERIFY_FAILED handling, rollback path via snapshot + `before_settings_json`.
- Real-Lightroom QA of the plugin apply path and the capability matrix on current Lightroom Classic.
- Preview-cache size enforcement (setting exists since 0.1).

## 0.4.0 — Continuous learning

- Sync corrections; prediction vs final delta; corrections UI; retrain with corrections; version comparison; observed acceptance / No-Touch tracking after review-session close; style health insights.

## 0.5.0 — Session intelligence

- Burst awareness, reference photo, manual merge/split/name groups, per-group confidence, per-camera/lens analysis, anomaly detection.

## 0.6.0 — Adaptive local editing research (capability-gated)

- Investigate mask/local data exposure on current Lightroom; formal adapter; beta only on tested versions; never reconstruct masks from opaque ACR data.

## 0.7.0 — Style portability + commercial polish

- Export/import Style packages (models + schema metadata, no photos), signed manifests, backup/restore, macOS.

## Ongoing

- Replace the development updater key before the first non-alpha release; commercial code signing when certificates exist.
- ONNX image encoder manifest (pinned, SHA-256) with first-run verified download and DirectML acceleration.
- DNG embedded-XMP reading for sidecar-less DNGs.
- Light theme review.
