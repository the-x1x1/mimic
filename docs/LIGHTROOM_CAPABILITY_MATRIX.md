# Lightroom capability matrix

The matrix is computed at runtime by `mimic_core::capability::CapabilityMatrix::from_probe` from what the plugin reports; nothing here is a static promise. Settings › Lightroom shows the live matrix; the diagnostic bundle includes it.

## Statuses per control

| Status                  | Meaning                                                                                                                                                                                                             |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `supported`             | Key returned by `getDevelopSettings` on the probe photo **and** in the 0.x write policy **and** the connection can apply (preset application + plugin presets + catalog write access all true). Mimic may write it. |
| `observed_not_writable` | Key returned but Mimic will not write it in 0.x (point curves, free-text profile names, `lens`/`crop` families), or apply is unavailable on this connection, or no probe photo was selected yet.                    |
| `unsupported`           | Not returned by this Lightroom — cannot be read or written.                                                                                                                                                         |

Connection-level flags: `canRead` (getDevelopSettings), `canApply`, `canSnapshot`, `probeHadPhoto`. `schemaVersion` (`cap_<hash>`) fingerprints Lightroom version + flags + writable key set; it is stored on snapshots and predictions so an apply can detect that Lightroom changed since prediction time.

## 0.x static write policy

Writable families: whiteBalance, tone, presence, hsl, toneCurve (parametric only), colorGrading, detail, effects, calibration. Read-only: lens, crop. Never: masks, local adjustments, Look tables, AI features.

## Observed results

| Lightroom Classic | Plugin | Probe photo | Supported controls | Apply | Snapshot | Source                                                                   |
| ----------------- | ------ | ----------- | ------------------ | ----- | -------- | ------------------------------------------------------------------------ |
| _(none yet)_      |        |             |                    |       |          | Fixture `handshake.request.json` models a 14.3 catalog; it is synthetic. |

Add a row from Settings › Lightroom whenever a new version is tested; file a _Lightroom compatibility report_ issue with the table.
