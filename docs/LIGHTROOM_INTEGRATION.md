# Lightroom integration

Mimic integrates with Lightroom Classic exclusively through a Lightroom SDK plugin (`lightroom/Mimic.lrplugin`) that polls a loopback HTTP bridge owned by the desktop app. Adobe's cloud Lightroom REST API (end of life July 31, 2026) is not used and never will be the foundation.

## Constraints designed around (spec §3)

- Develop-settings tables may change between Lightroom versions → capability probe at runtime, schema-versioned matrix, unknown keys preserved, compatibility fixtures.
- Since Lightroom Classic 15.0 heavy edits may live in an `.acr` sidecar → detected as `opaque`, counted in the data quality report, never parsed or mutated. Connected ingest is preferred because `photo:getDevelopSettings()` returns the live table.
- Mask support is capability-gated and `unsupported` in every 0.x release.

## Transport

- Desktop binds `127.0.0.1:<random port>` on launch and writes `bridge/bridge.json` (`{protocolVersion, appVersion, baseUrl, token, pid, writtenAt}`) to the per-user app data folder with owner-only permissions where the OS supports it. Never `0.0.0.0`.
- Plugin reads the discovery file (default `%LOCALAPPDATA%\Formicaria\Mimic\bridge\bridge.json`, overridable in the Plugin Manager panel) and validates protocol version, loopback URL, token length.
- Every request carries `Authorization: Bearer <token>`; bodies are capped (8 MiB default); requests with a browser `Origin` header are rejected.
- Transport is `LrHttp.post/get` from the plugin. No file-queue fallback is implemented; if `LrHttp` is unavailable the plugin reports `lrHttp=false` in the probe and the desktop shows the connection as offline.

## Flow

```
plugin  → POST /bridge/v1/handshake   {protocolVersion, pluginVersion, lightroomVersion, sdkVersion, catalogFingerprint, catalogName, capabilities}
desktop ← 200 {accepted:true, sessionId, pollIntervalMs, maxBatchSize} | 426 {accepted:false, reason}
plugin  → GET  /bridge/v1/commands/next?waitMs=5000   (long-poll; 204 when idle; 409 if handshake required)
desktop ← 200 {command:{commandId, commandType, payload}}
plugin  → POST /bridge/v1/commands/{id}/result   {commandId, ok, result | error{code,message,details}}
plugin  → POST /bridge/v1/events                 {events:[{type, at, payload}]}
plugin  → POST /bridge/v1/capabilities           CapabilityProbe (refresh after selecting a photo)
```

Liveness: the desktop declares the plugin disconnected when it has not polled for 6 s; queued commands then fail fast with `disconnected`. A new handshake supersedes any earlier session and fails commands queued for it.

## Commands

| Command                           | Mutates catalog                                                | Payload → result                                                                                                                         |
| --------------------------------- | -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `ping`                            | no                                                             | `{echo}` → `{pong:true}`                                                                                                                 |
| `get_catalog_info`                | no                                                             | → `{path, lightroomVersion, hasWriteAccess}`                                                                                             |
| `get_capabilities`                | no (a no-op write transaction is used as a write-access probe) | → `CapabilityProbe`                                                                                                                      |
| `get_selected_photos`             | no                                                             | `{scope: selection                                                                                                                       | collection | folder                                                       | catalog, maxPhotos}`→`{photos:[{photoId, path, uuid, isVirtualCopy, metadata{…}}], total, truncated}` |
| `get_photo_metadata`              | no                                                             | `{photoIds}` → `{items:[…formatted]}`                                                                                                    |
| `get_develop_settings`            | no                                                             | `{photoIds}` (≤ maxBatchSize) → `{items:[{photoId, path, settings{…raw develop table}}]}`                                                |
| `create_before_snapshot`          | yes                                                            | `{items:[{photoId, snapshotName}]}` → per-item `created                                                                                  | failed`    |
| `apply_settings_as_plugin_preset` | yes                                                            | `{items:[{photoId, predictionId, snapshotName, settings}], createSnapshot, readBack}` → `{items:[{photoId, predictionId, status: applied | failed     | skipped, snapshotName, before, readBack, error}], canceled}` |
| `read_back_develop_settings`      | no                                                             | `{photoIds}` → same as get_develop_settings                                                                                              |
| `collect_correction_state`        | no                                                             | `{photoIds}` → settings + `collectedAt`                                                                                                  |

Batch rules: bounded by `maxBatchSize` (25), per-photo success/failure, cancellation checked between photos, a batch with any failed item is never reported as a clean success (`apply_batches.status = completed_with_failures`).

## Apply strategy (§10.4) — implemented in Lua, NEEDS REAL-LIGHTROOM QA

1. Desktop maps canonical → Lightroom keys through the live capability matrix (`edit_dna::to_lightroom_settings`), dropping anything not writable.
2. Per photo inside `catalog:withWriteAccessDo` (15 s timeout): read `before` via `getDevelopSettings`; if a snapshot was requested and `createDevelopSnapshot` fails, **stop for that photo** (never apply without the safety net); `LrApplication.addDevelopPresetForPlugin(_PLUGIN, "Mimic <predictionId>", settings)`; `photo:applyDevelopPreset(preset, _PLUGIN)`; read back.
3. Desktop compares intended vs read-back (`verify_readback`): mismatch → `verify_failed`, not counted as applied.

## Plugin install experience (§49)

Mimic copies the plugin to `%LOCALAPPDATA%\Formicaria\Mimic\plugin\Mimic.lrplugin` on every launch (idempotent sync) and shows exact Plug-in Manager steps with a copy button and a reveal button. It never edits Lightroom preferences. The plugin exposes _Library › Plug-in Extras › Mimic: Connection Status… / Reconnect Now_ and a Plugin Manager panel to override the bridge file path.

## What has and has not been verified

- Verified in CI: the whole HTTP protocol with a fake plugin (handshake, polling, results, timeout, disconnect, reconnect, events, body cap, auth), fixture round-trips in Rust/TS/Lua, Lua syntax of every plugin file.
- Not verified (no Lightroom in CI): the SDK calls themselves on a real catalog, `LrHttp` behaviour with long-poll timeouts, snapshot creation, preset application and read-back equality on a real Lightroom version. Track in `docs/LIGHTROOM_CAPABILITY_MATRIX.md` as reports arrive.
