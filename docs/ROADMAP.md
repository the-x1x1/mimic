# ROADMAP — future work only

Completed work lives in CHANGELOG.md and PROJECT_STATUS.md, never here.

## 0.4.x — Lightroom QA and polish

- Real-Lightroom QA of the plugin apply/restore/sync path and the capability matrix on current Lightroom Classic (fake-plugin coverage exists; real-catalog behaviour does not).
- Review filter for “corrected” photos and _Use as Reference_.
- Weighting of correction pairs in training (today corrections join the dataset as ordinary pairs).
- Automatic sync prompt when a session's Lightroom edits settle (today sync is a button).
- Preview-cache size enforcement (setting exists since 0.1).
- Per-apply-batch item table in the UI (today: counts + Restore; per-photo outcome lives in the photo panel).

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
