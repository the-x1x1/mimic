# Changelog

All notable changes to Mimic are documented here. The format follows Keep a Changelog; versions follow SemVer with pre-release tags for alpha/beta builds.

## [0.8.0-alpha.1] — 2026-09-20

A home screen. Mimic opens on what is waiting for a reply, with the draft it has written for each one, and approve / modify / reject on every draft. Compose is folded into it rather than being a screen of its own.

### Added

- **Dashboard**, the new home screen. Threads whose last message came from someone else and was never answered, newest first, each with the message in full, who it is from, whether a draft would be shaped by how you write _to them_ or only in general, and any draft Mimic already has. Counts of what has been imported, and when the last import ran.
- **Approve, modify, reject.** Approving copies the reply and records it as sent unedited; modifying records what you changed, which is the only thing the learning loop can learn from; rejecting discards it. "Tell Mimic why" records a stated preference, which outweighs anything inferred from an edit. None of these send: Mimic has no send path, and adding one stays a separate decision.
- **Assisted drafting** (`assist.autoDraft`, off by default): with it on, Mimic drafts a reply for each waiting thread after every import or analysis, and the dashboard shows them waiting for approval. Bounded to ten threads per run, never a thread that already has an unresolved draft, cancellable between threads, and it never runs while the setting is off — a disabled run reaches no provider at all. The Settings copy says plainly that this sends incoming messages to the configured model without being asked each time, and names whether that model is local.
- `threads_awaiting_reply` and its count: a conversation is waiting because its last message has `direction = 'other'`, not because anything inferred urgency. Messages whose direction could not be established never make a thread look answered either way.
- `fixtures/contracts/dashboard.json`, written by the Rust end-to-end test and parsed by the zod suite, including a thread carrying a prepared draft.

### Changed

- Navigation is Dashboard / People / Voice / Sources / Settings. The Compose screen is now the "Write something new" panel on the dashboard; the intent field is still the largest input on it.
- The dashboard describes itself honestly: nothing arrives on its own, every message on it came from an import, and with nothing imported it says so rather than showing an empty feed.
- `fixtures/import/sample_export.json` gained a trailing inbound message so the corpus actually contains a thread awaiting a reply (46 messages, 45 attributable).

### Not in this release

A send path. Mimic drafts; you send. Everything here is built so that turning that on later is one deliberate change rather than a slide.

## [0.7.0-alpha.1] — 2026-09-20

The first run works. 0.6.0 built the pipeline and could be walked through by someone who knew where the walls were; this release is about a person installing Mimic, opening it, and getting to a draft without being stranded or misled.

### Fixed

- **Onboarding no longer sits on "Importing…" forever.** Native job events were subscribed inside the application shell, which onboarding does not render, so a finished import or analysis never reached the screen that was waiting for it. The only way past was to quit and reopen the app. The subscription now lives above the gate, and onboarding shows the running job's progress.
- **An import that finds nothing of yours says so.** When the file is read and not one message matches a declared identifier — the likeliest first-run mistake, and the one the add-source dialog warns about — the step used to render a paragraph and no button at all. It now names the addresses it looked for, takes another one inline, and re-reads the file (which costs nothing, because messages are keyed on their own identifiers).
- **A small mailbox is no longer a lock-out.** Finishing onboarding required a measurable voice profile, which needs twenty of the user's own messages; below that, the last step re-rendered forever. Anyone with their own messages imported can now leave onboarding, and Compose already says when it is drafting without a measured style.
- **Adding a second address no longer throws you forward.** The identity step is left on a click rather than on the first identifier appearing.
- **A failed import offers to retry or to remove the source**, rather than leaving the step in the same shape as a successful one.

### Changed

- **The model badge stops claiming a model is there.** The local provider is always registered, so the top bar showed a green "Local model" on a machine with nothing listening on the endpoint, and every draft failed with a toast underneath it. The badge now reflects a real reachability check, and until that check has run the answer is "unknown", not "fine".
- **Compose refuses to draft when the model is not answering**, and says which one and why. `composeReadiness` takes the provider's state and is the one place that decides; the button is disabled rather than pretending.
- Settings gained the three things it configured but could not do: create a diagnostics bundle (the "include full paths" checkbox now has something to affect), restart the engine when it is not running, and check for or install an update. The update badge in the top bar links to that section instead of to the top of the page.
- The Voice empty state links to Sources instead of describing where to go.
- `PeoplePage` uses the shared `MIN_SAMPLE` rather than its own copy of `20`.
- `scripts/test.ps1 -Quick` now means something: it skips the production frontend build, which is what `validate.ps1` without `-Full` advertised and did not do.
- Stale text removed: `CONTRIBUTING.md` no longer asks for a Lightroom capability matrix or names the bridge and EditDNA contracts; the Tauri capability description no longer mentions an asset protocol that is disabled; the install guard's comment names imports and analyses rather than photography jobs.

### Packaging

- The release job installs the engine's Python environment **before** `cargo test --workspace`. It ran after, so the four engine-protocol tests — the ones that exercise the thing the release ships — skipped themselves on every release build while CI ran them properly.
- The release job now fails if the bundle was built without a packaged engine, or if the installer is implausibly small to contain one. The previous failure mode was an installer that opened to a permanent "Engine unavailable".
- `models/manifests/` is no longer bundled: it contains one README and no encoder ships in this version.
- `package-engine.ps1` writes back the tracked `resources/engine/README.txt` it deletes, so packaging locally no longer shows up as a deleted file.

## [0.6.0-alpha.2] — 2026-09-20

Release-pipeline fix only; application code is identical to 0.6.0-alpha.1 (whose Release workflow never produced an installer).

### Fixed

- `scripts/package-engine.ps1` still copied `packages/contracts/edit_mapping_v1.json` into the packaged engine. The migration moved that file to `archive/legacy-photography/contracts/`, so the "Package engine" step failed with `Cannot find path ... edit_mapping_v1.json` after PyInstaller had already succeeded, and no draft release was ever created. The copy is gone, along with the `rawpy` binary collection and the `PIL._tkinter_finder` hidden import — both left over from the image pipeline, neither a dependency of the text engine.

## [0.6.0-alpha.1] — 2026-09-20

**Mimic is now a different product.** It no longer learns how you edit photographs in Lightroom Classic. It learns how you communicate, from messages you have already written, and helps you draft replies that sound like yourself.

Versions 0.1.0 through 0.5.0 were a Lightroom Classic editing assistant. Everything below describes retiring that product and building the first working slice of this one. `docs/MIGRATION_AUDIT.md` records the whole decision, component by component.

### Removed

- The photography domain in full: `edit_dna` (Lightroom develop-setting normalization), `bridge` (the loopback HTTP server the Lightroom plugin talked to), `capability` (the runtime SDK probe), `sessions` (shoots, scene clusters, predictions, apply batches, restores), `ingest` and `training` in their photography form, the Python image pipeline (XMP parsing, RAW previews, EXIF, visual features, scene heuristics, the hybrid KNN + ridge trainer), and roughly 5,500 lines of photography UI.
- Nineteen database tables, dropped by migration `0005`: `libraries`, `assets`, `sidecars`, `edit_snapshots`, `visual_features`, `style_profiles`, `style_profile_libraries`, `sessions`, `session_assets`, `scene_clusters`, `training_sets`, `model_versions`, `model_artifacts`, `predictions`, `apply_batches`, `applied_edits`, `corrections`, `correction_syncs`, `lightroom_connections`.
- Dependencies: `axum`, `tower`, `tower-http` and `reqwest` (dev) on the Rust side; `pillow`, `exifread` and the `rawpy` extra on the Python side; the Tauri `protocol-asset` feature and the asset protocol itself.
- The `plugin` CI job, the Lua version targets in `sync-version.mjs`, the plugin zip in the release workflow, `install-lightroom-plugin.ps1` and `test-plugin.ps1`, the synthetic image fixtures, and the image encoder manifests.

### Archived

Moved to `archive/legacy-photography/`, excluded from every workspace, from CI and from the bundle: the Lightroom Classic plugin and its tests; `edit_mapping_v1.json` with `EDIT_DNA.md`; the Lightroom integration and capability-matrix documents; `ML_PIPELINE.md`, for the shoot-grouped split reasoning that the new evaluation harness reuses; ADRs 003, 004 and 005; and the golden XMP corpus that proved the mapping. None of it is built or shipped.

### Changed

- `mimic-core` is now database, sources, import, voice, retrieval, generation, providers, privacy and jobs. The SQLite layer, the migration mechanism, the job queue, the engine sidecar client, `paths`, `ids`, `version` and `diagnostics` carry over unchanged in substance.
- The Python engine is a text engine: embeddings, similarity and held-out evaluation. The NDJSON server, dispatch, progress and error envelopes are untouched.
- Navigation is Compose / People / Voice / Sources / Settings. Compose is the home screen.
- Diagnostics report providers instead of a Lightroom bridge, and strip `token`, `apiKey`, `api_key`, `password` and `secret` at every depth.
- The app data layout loses `cache/previews`, `models/styles`, `bridge/` and `plugin/`, and gains `credentials/`.
- `docs/ARCHITECTURE.md`, `PRIVACY.md`, `PROJECT_STATUS.md` and `ROADMAP.md` were rewritten from scratch; `DATABASE.md`, `TEST_STRATEGY.md` and `UI_SPEC.md` were replaced by `DATA_MODEL.md`, the testing section of `ARCHITECTURE.md`, and `PRODUCT.md`.

### Added

- **Schema v5** (`0005_communication.sql`): user identity and identifiers; sources; participants and their identifiers; conversations, conversation participants and messages; message embeddings; situations; layered voice profiles, manual preferences and representative examples; drafts and draft feedback; analysis runs; evaluations and evaluation cases. Indexed for 100k–1M+ messages. A v4 photography database upgrades cleanly, keeping settings, jobs and events.
- **Source connectors** behind one `CommunicationSource` trait that streams conversations to a sink: `mbox` (RFC 4155, reference-chain threading with a narrow subject fallback, separator detection that does not split on a body line beginning with "From ", content-derived ids when `Message-ID` is missing) and `mimic_json` (the documented generic format). Validation is a dry run of the connector's own import, so it cannot disagree with what the import will do.
- **Normalization** that removes quoted history, attribution lines in four languages, forwarded banners, Outlook reply blocks and signatures — while keeping sign-offs the user typed, because "Thanks, C" is how someone writes and a `--` block is not.
- **Import**: identity-based direction (`self` / `other` / `unknown`, never guessed), participant resolution through a cache, batched inserts of 500 in a transaction, reply linking and response latency derived per conversation, cancellation between conversations, and a resume that costs nothing because `(source_id, external_id)` is unique. Refuses to run when no identity is declared.
- **The voice engine**: deterministic metrics over the user's own messages (length distribution, terminal punctuation, capitalization two ways, emoji, contractions per hundred words, greetings, sign-offs, repeated phrases, response latency), computed per layer — global, channel, relationship — with a 20-message floor below which a scope reports its sample size and no rates. Representative examples chosen deterministically and de-duplicated.
- **Retrieval** that filters on participant, channel, relationship, situation, conversation, source and date range before ranking, and ranks lexically with inverse document frequency.
- **Generation**: a context builder that gathers evidence, a prompt assembler that is a pure function of that context, measured habits turned into instructions rather than quoted as numbers, an output budget derived from the user's own p90 message length, and four adjustments.
- **Model providers** behind one trait: a local OpenAI-compatible endpoint (Ollama, LM Studio, llama.cpp) whose locality claim is computed from the URL rather than asserted, the Claude Messages API, and a deterministic mock for tests. The default is always a local provider.
- **Deletion that deletes**: a preview produced by the same code path as the deletion, removal of the user's own half of a one-to-one conversation, survival of group threads minus that person, invalidation of every aggregate computed over the removed material, source deletion, and a delete-everything that keeps settings and identity.
- **The learning loop's recording half**: draft, what was actually sent, a described diff, and weights in which a stated preference outranks an inferred edit three to one.
- **A held-out evaluation harness** in the engine: conversation-grouped splitting so no thread straddles the boundary, and comparison along named components — length, vocabulary, punctuation, embedding — with **no headline score**.
- **The interface**: Compose with the intent field as its centre and an evidence panel beside every draft; People with per-person counts and a deletion dialog that states consequences in sentences; Voice, which shows what was measured and what was not; Sources with pre-import validation; Settings covering identity, provider, privacy and deletion.
- **Cross-language contract fixtures**: the Rust end-to-end test writes `fixtures/contracts/`, the zod suite parses them, and a shape change on one side that is not made on the other fails a test.

### Migration notes

- **A v4 database loses its photography data.** That data describes a product that no longer exists. `Db::open` writes a timestamped backup to `data/backups/` before the migration runs, so it is recoverable if anyone needs it.
- Settings, jobs, the event log and update state survive. `review.highThreshold`, `review.mediumThreshold`, `performance.accelerator`, `performance.previewCacheMaxMb` and `performance.inferenceBatchSize` are gone; `generation.provider`, `generation.localUrl`, `generation.localModel` and `generation.anthropicModel` are new.
- The Lightroom plugin is no longer installed or updated by the app. An existing copy under `%LOCALAPPDATA%\Formicaria\Mimic\plugin\` is left alone rather than deleted, and can be removed by hand.
- Anyone who wants the photography product should use the `v0.5.0-alpha.1` tag; it is the last release before this one and the full record of what was removed.

### Known limitations

- **No accuracy figure anywhere**, because the evaluation harness has not been run over a real corpus and neither baseline is implemented. This is deliberate; see `docs/VOICE_ENGINE.md`.
- Embeddings are `lexical_v1` — hashed word and character n-grams. Genuinely useful, not semantic, and the engine reports `semantic: false` so the app cannot imply otherwise.
- The situational voice layer has a schema, a resolution path and a prompt slot; nothing classifies into it yet.
- Provider credentials live in an owner-only file, not the OS credential store.
- Voice analysis materializes a scope's messages in memory before computing. Fine at a hundred thousand; not at a million.
- The Anthropic provider has never been exercised against the live API in CI.
- `tauri build` has not been run in this environment, so the Windows installer and the signed updater path are unverified for this release.
- The nightly smoke workflow was rewritten against the packaged engine but has never been observed running.

## [0.5.0-alpha.1] — 2026-09-17

Session intelligence: reference photos, group editing, per-group and per-camera confidence, and group-outlier flags.

### Added

- Schema v4 (`0004_session_intelligence.sql`): `scene_clusters.reference_asset_id` (FK to assets) and `edited_at`; the seeded upgrade test now runs v1 → v4.
- Engine: `model.predict` accepts `references {groupId: assetId}`; the consistency policy pulls a group toward its reference (blend 0.8, same per-family caps) and never changes the reference itself (`isReference`, `consistencyShift = 0`); `detect_outliers` flags photos whose exposure, temperature or tint sits > 12 % of range from their group's median (groups ≥ 4) as `groupOutlier` with a reason line, judged on raw predictions before blending.
- mimic-core `sessions`: `edit_groups` (rename; set/clear a member-only reference; merge with the target keeping label and reference; move photos into an existing or new group with emptied sources deleted and orphaned references cleared), every edit stamping `edited_at`; `predict_session` passes group references and counts outliers; `session_detail` gains `groupStats` (photos, predicted, mean/min confidence, low-confidence, unfamiliar, outliers, applied, rejected per group), `cameraStats` (photos, mean confidence, known-to-model per camera/lens), `groupingChangedSincePrediction` and `syncSuggested`.
- Command `edit_session_groups`; `GroupEdit` tagged-union contract and `session_detail.json` fixture round-tripped in Rust and zod (the fixture caught a snake_case field leak).
- UI: Scene groups table with inline rename, "use selected as reference", two-step merge, move/split of the multi-selected photos (Ctrl/Cmd/Shift-click in the grid); cameras/lenses table when a session mixes bodies or uses one the model has not seen; banners when groups changed after the last prediction and when a corrections sync is due; outlier badge and reference star on tiles; Review's attention queue includes group outliers.
- Tests: pytest reference/outlier unit tests and service-level reference assertions; repository tests for every group edit; `sessions_e2e` extended with rename → split → invalid reference → merge → predict → reference → stale flag → re-predict; GroupsPanel component tests; contracts tests.

## [0.4.0-alpha.2] — 2026-09-17

Release-pipeline fix only; application code is identical to 0.4.0-alpha.1 (whose Release workflow never produced an installer).

### Fixed

- `scripts/package-engine.ps1` smoke check treated the two-line stdio reply (`engine.hello` + `engine.shutdown`) as a failure: PowerShell's `-notmatch` on an array returns the non-matching lines rather than a boolean. The reply is now joined before matching, and a non-zero exit of the packaged engine is reported on its own. This is why the 0.3.0-alpha.1 and 0.4.0-alpha.1 release jobs failed at "Package engine" although the bundle worked.

## [0.4.0-alpha.1] — 2026-09-17

Continuous learning: Mimic now reads applied photos back after your own pass in Lightroom, keeps what you changed as corrections, measures the No-Touch Rate from what you left alone, and trains the next version on those corrections. Proven against a scripted plugin over the real bridge; real-Lightroom behaviour remains unverified.

### Added

- Schema v3 (`0003_corrections.sql`): `correction_syncs` (one row per sync: checked / untouched / corrected / unresolved) and a unique `corrections(prediction_id)`; the migration test now upgrades a seeded v1 database through every version.
- mimic-core `corrections`: `sync_corrections` job (same catalog required; photos resolved like apply; `collect_correction_state` in chunks of 25; keys Mimic wrote compared with read-back tolerances; per-control normalized deltas and magnitude; `correction` edit snapshot with the photographer's final settings; re-sync replaces an unused correction and keeps one already used by training), `no_touch_stats` (per version, synced sessions only, restored edits excluded), `style_health` (active No-Touch, corrections pending training, most-corrected controls with signed bias, computed insight sentences).
- Training with corrections: `train_style` passes the Style's pending corrections as `correctionAssetIds`; the engine's dataset loader adds those assets only when their latest snapshot is a `correction`, groups them as their own shoot, reports `correctionPairs`; the job marks them `included_in_training_version`.
- Commands: `sync_corrections`, `get_style_health`, `list_corrections`; `StyleSummary.noTouchRate`; `StyleDetail.health`; `SessionDetail.correctionSyncs`; zod contracts with fixtures shared with the Rust round-trip tests (`style_health.json`, `correction_row.json`) and a `collect_correction_state` bridge fixture.
- UI: Style Corrections tab (No-Touch per version, insights, most-corrected controls, correction list with trained/pending state, honest empty state), Versions tab side-by-side comparison (overall and per-family holdout error, measured No-Touch), session _Sync corrections_ button with reasons and a sync history table, Home and active-version cards show the measured No-Touch Rate or “—”.
- Tests: pytest dataset test for correction assets, Rust unit tests for the diff and health, `sessions_e2e` extended with sync → idempotent re-sync → retrain consuming the correction → clean failure on a session without applies, component tests for the Corrections panel and version comparison.

## [0.3.0-alpha.1] — 2026-09-17

Sessions, scene grouping, prediction with confidence, Review, and the Lightroom apply/restore path with read-back verification. Everything that touches a catalog is proven against a scripted plugin over the real bridge; behaviour on a real Lightroom Classic is still unverified.

### Added

- Schema v2 (`0002_sessions.sql`): `libraries.purpose` (training vs session-backing libraries, hidden from the Libraries UI), `applied_edits.restored_at / restore_result / restore_error_json`, `predictions.capability_schema_version / cluster_id`; migration test seeds a v1 database with rows and upgrades it; the engine's test database builder now applies every checked-in migration.
- Engine `session.group`: capture-time blocks (20 min gap, untimed frames share one block), seeded k-means on standardized visual statistics (+ embedding when present) with a minimum cluster size, burst detection, deterministic output. Engine `model.predict` gains `groups` + `consistency`: a bounded per-family pull toward the scene-group median (white balance, colour, presence); exposure and tone are never blended, the maximum shift is reported per photo.
- mimic-core `sessions`: `create_session` (folder or Lightroom scope), `ingest_session` (reuses the folder/Lightroom ingest, capture-ordered membership), `group_session`, `predict_session` (active version only, feature-schema check, supersedes earlier predictions, records the Lightroom capability schema version), review transitions, `apply_preflight` (connected, canApply/canSnapshot, writable controls, same catalog, stale capability schema, apply already running), `apply_session` (photo resolution by local id or normalized path against the catalog listing; batches of 25 with `Mimic Before` snapshot and read-back; `verify_readback` per item; `prediction` edit snapshot on success; cancellation between batches; `outcome_unknown` recorded when the bridge fails mid-batch), `restore_batch` (before-values of the written keys only, read-back verified, per-item restore result, predictions back to pending). Apply and restore are never re-queued after an interruption.
- Commands: `list_sessions`, `create_session`, `get_session_detail`, `list_session_photos`, `set_session_style`, `delete_session`, `group_session`, `predict_session`, `set_prediction_review`, `get_apply_preflight`, `apply_session`, `list_applied_edits`, `restore_apply_batch`, `get_prediction`; typed contracts with checked-in fixtures (`fixtures/sessions/*`) asserted by Rust round-trip and zod tests.
- Sessions UI: list, New Session dialog (folder or Lightroom scope, optional Style), detail page with Analyze scenes → Predict → Apply to Lightroom (each disabled with a reason), live job card, metrics, per-group and needs-attention filters, confidence badges on every tile, prediction panel (predicted Lightroom values, confidence components and reasons, Lightroom outcome with read-back mismatches, Looks right / Reject / Apply this photo), apply history with Restore, confirm dialog that states the safety steps and shows the backend's blockers verbatim.
- Review UI: attention-only default (below the medium threshold, unfamiliar, failed apply), All pending, Every prediction; filmstrip, large preview, prev/next, the same prediction panel; works offline, Apply requires Lightroom.
- Bridge fixtures for the restore payload and the catalog listing; Lightroom integration, ML pipeline, database, architecture and UI docs updated.
- Tests: pytest session suite (grouping determinism, time gaps, visual split, untimed frames, consistency policy, service-level `session.group` + consistent predict), Rust `tests/sessions_e2e.rs` (real engine ingest of fixture images, grouping, prediction, re-prediction supersedes, rejected photo excluded, apply refused without Lightroom, apply against a scripted plugin with one read-back mismatch and one missing photo, stale-capability refusal, restore with verification), migration upgrade test, repository tests for restore bookkeeping and session delete, frontend tests for the prediction panel, confidence badge, confirm dialog and review queue.

### Changed

- Home and Settings no longer describe Sessions/Review as future work; the unhonoured “create a snapshot before applying” toggle was removed — snapshot + read-back are mandatory and stated as such.
- `apps/desktop/tsconfig.tsbuildinfo` is no longer tracked (it is a build cache and blocked fast-forward pulls).
- Styles with predictions cannot be deleted (apply history references their versions); delete the sessions first. The UI reports the reason.

## [0.2.0-alpha.1] — 2026-09-17

First Style Brain. Training is real, reproducible and measured; prediction is exposed through the engine and exercised end to end, but there is still no Sessions/Review UI or Lightroom apply (0.3.0).

### Added

- Training pipeline in the engine (`training.train`): training-set builder over normalized EditDNA pairs with filters (no snapshot, no features, no meaningful edits, too many unknown keys), session-grouped train/validation/holdout split that never puts one shoot on both sides (deterministic per seed; honest fallbacks for two shoots and single-shoot libraries), baselines (global median, camera/lens-conditioned median, distance-weighted KNN), per-control ridge residual on top of leave-one-out KNN (`hybrid_knn_residual`), per-control and per-family metrics in raw and normalized units (MAE/RMSE/nMAE/p50/p90/p95), acceptance proxy explicitly labelled as not a No-Touch Rate, baseline comparison, reproducible training config (seed, versions, fingerprint, dependency versions), SHA-256 hashed artifacts.
- Confidence calibration persisted with every model: unseen-photo neighbour distances, family validation error, camera/lens/ISO coverage; per-photo confidence with stored components, out-of-distribution flag capped at 0.49, and plain-language reasons.
- Prediction method (`model.predict`) returning canonical settings (normalized + raw), nearest training examples, raw component outputs and confidence per asset.
- `train_style` job in mimic-core: immutable `model_versions` row created in `training` state, finalized once with metrics + artifact manifest, training set recorded, artifacts registered; activation policy — first version activates, later versions activate only when holdout error is not worse than the active one; failed runs leave a `failed` row with the structured error. Composite job executor and engine progress forwarding into job records.
- Commands: `train_style`, `activate_model_version` (rollback), `archive_model_version`, `get_model_version`; Style detail reports training availability with the exact reason.
- Styles UI: Train New Version (enabled only when data, engine and no running training allow it), live training progress, active-version card with holdout nMAE, Versions tab with real metrics (overall nMAE, exposure MAE in EV, evaluation set, beats-median) and Activate/Archive; Home shows holdout error; onboarding gains a Train step with phase-based progress and the resulting metrics.
- Tests: pytest training suite on a synthetic database built from the real migration (dataset filters, leak-free deterministic split, reproducibility, hybrid beats global median by a wide margin on exposure, prediction + OOD behaviour, insufficient-data failure); Rust end-to-end training test with the real engine through the job runner (versions, activation policy, rollback, prediction, insufficient data precheck); VersionList component tests; contracts tests for metric helpers.

### Fixed

- Engine exit code was read with `try_wait()` immediately after stdout closed and came back `None` on Windows; the client now awaits the real exit status.

## [0.1.0-alpha.1] — 2026-09-16

Foundation + real ingest. This is a pre-release: the ingest pipeline is real and tested end to end; training, prediction and Lightroom apply are not part of this build.

### Added

- Tauri 2 desktop shell with dark graphite theme, onboarding flow (Lightroom / folders / clearly labelled DEMO), Home, Styles (list, detail with Overview/Training Data/Versions/Corrections), Sessions and Review empty states, Settings (General, Lightroom, Performance, Storage, Privacy, Updates, Diagnostics).
- SQLite database with forward-only transactional migrations, automatic backup before migrating an existing database, WAL, foreign keys, and all 21 tables from the data model.
- Persistent job system: queued/running/completed/failed/canceled/interrupted, heartbeat, item-level progress, cancellation between items, restart recovery that re-queues only resumable jobs.
- Python engine sidecar (`mimic-engine serve`): NDJSON stdio protocol with request correlation, structured errors, progress events, 32 MiB message cap; automatic restart with a bounded budget.
- Folder scanner: RAW/rendered detection, XMP/ACR pairing by directory + basename, duplicate-basename and orphan-sidecar reporting, fast identity hash, EXIF extraction.
- Read-only XMP parser (`xmp_parser_v1`): attribute and element forms, curves, structured masks and Look tables, unknown key preservation, metadata summary, malformed-file isolation.
- RAW preview decoding (LibRaw via rawpy, embedded preview first) with an on-disk cache; deterministic image statistics (`features_v1`), heuristic scene labels, `stats_v1` embeddings stored as `.npy`, ONNX encoder manager with SHA-256 verification and mandatory fallback.
- Canonical EditDNA (`edit_mapping_v1`, 104 controls across 11 families) shared by Rust, Python and TypeScript, with golden fixtures for modern (PV 15.4 with masks and unknown keys), PV 2012 and legacy PV 2010 XMP.
- Lightroom Classic plugin (`Mimic.lrplugin`): discovery-file handshake, long-poll command loop, runtime capability probe, catalog listing, develop-settings read, before-snapshot, plugin-preset apply with read-back, correction-state collection, Plugin Manager panel, dependency-free Lua JSON.
- Loopback-only bridge server with per-launch 256-bit token, body limits, origin rejection, command queue with timeouts, liveness sweep and reconnect handling; fake-plugin integration tests.
- Capability matrix derived from the probe: supported / observed-not-writable / unsupported per control; apply and snapshot gated on runtime flags.
- Data quality report: assets, valid pairs, missing edits, Lightroom-connected vs sidecar-only coverage, ACR heavy-edit count, local-edit count, camera and shoot-day distribution, recommendation level, honest warnings.
- Diagnostics bundle with no tokens and redacted paths; structured JSON logs with daily rotation.
- Updater plumbing: Tauri updater with embedded public key, 6-hour jittered background checks, install guard that refuses while jobs run, persisted update state; GitHub Actions release workflow that builds, signs, generates `latest.json` and checksums, and creates a draft release.
- CI: frontend, Rust (with the real-engine end-to-end test), Python, Lua plugin, security audit and secret scan.

### Known limitations

- Training (0.2.0), sessions/prediction/review/apply (0.3.0) and correction sync (0.4.0) are not implemented.
- The plugin apply path is verified against fixtures and a fake plugin only: NEEDS REAL-LIGHTROOM QA.
- The updater configuration ships a development public key; a non-alpha release is refused by `verify-release.ps1` and the release workflow until it is replaced.
