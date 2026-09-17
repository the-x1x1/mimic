# ROADMAP — future work only

Completed work lives in CHANGELOG.md and PROJECT_STATUS.md, never here.

## 0.3.x — Lightroom QA and session polish

- Real-Lightroom QA of the plugin apply/restore path and the capability matrix on current Lightroom Classic (fake-plugin coverage exists; real-catalog behaviour does not).
- Review filter for “corrected” photos (needs 0.4.0 corrections) and _Use as Reference_.
- Preview-cache size enforcement (setting exists since 0.1).
- Per-apply-batch item table in the UI (today: counts + Restore; per-photo outcome lives in the photo panel).

## 0.4.0 — Continuous learning

- Sync corrections; prediction vs final delta; corrections UI; retrain with corrections; version comparison; observed acceptance / No-Touch tracking after review-session close; style health insights.

## 0.5.0 — Session intelligence

- Reference photo, manual merge/split/rename of scene groups (bursts and groups are detected today but not editable), per-group confidence, per-camera/lens analysis, anomaly detection.

## 0.6.0 — Adaptive local editing research (capability-gated)

- Investigate mask/local data exposure on current Lightroom; formal adapter; beta only on tested versions; never reconstruct masks from opaque ACR data.

## 0.7.0 — Style portability + commercial polish

- Export/import Style packages (models + schema metadata, no photos), signed manifests, backup/restore, macOS.

## Ongoing

- Replace the development updater key before the first non-alpha release; commercial code signing when certificates exist.
- ONNX image encoder manifest (pinned, SHA-256) with first-run verified download and DirectML acceleration.
- DNG embedded-XMP reading for sidecar-less DNGs.
- Light theme review.
