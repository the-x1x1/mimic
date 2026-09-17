# UI spec

Dark graphite workspace, neutral surfaces so photographs dominate, one restrained amber accent used for controls only (never over previews), rounded but not bubbly, subtle borders, short functional motion, reduced-motion respected. Tokens in `packages/ui/src/tokens.css`.

## Navigation

Sidebar: Home · Styles · Sessions · Review · Settings. No per-engine pages. Top bar: Lightroom connection badge, engine badge, unobtrusive update badge. Sidebar footer: job tray with item-based progress and Stop.

## Screens

- **Onboarding**: 1 source choice (Connect Lightroom / Train from folders / Explore with DEMO data) → 2 plugin setup (Continue enabled only after a real handshake) → 3 first Style + scan → 4 data quality with live item progress → 5 train (enabled only when the pair count allows).
- **Home**: primary action _Edit a New Session_, secondary _Train / Improve a Style_, active Style summary (name, version or "not trained", examples, holdout error, measured No-Touch Rate or "—" until a sync has checked applied photos), recent activity, compact system status, privacy note. Empty state when no Style.
- **Styles**: list cards (name, version badge, examples, cameras, updated). Detail tabs: Overview (data quality per library, training availability with reason, active version metrics), Training Data (libraries, browse ingested photos), Versions (immutable list, activate = rollback, archive), Corrections (No-Touch per version, insights, most-corrected controls with bias, correction list with trained/pending state; honest empty state before any sync). Versions also offers a side-by-side comparison of two versions (overall and per-family holdout error, measured No-Touch). Actions: Add Training Data, Train New Version (disabled with reason), delete.
- **Sessions**: list cards (status badge, photo count, Style, created). New Session dialog: folder or Lightroom scope, optional Style. Detail: Style selector, _Analyze scenes_ → _Predict_ → _Apply to Lightroom_ (each disabled with a tooltip reason), live job card, metrics (groups, pending, needs attention, Lightroom), apply history with per-batch Restore, _Sync corrections_ (disabled with a reason until something was applied and Lightroom is connected) with a sync history table, filter row (all / needs attention / per scene group), grid with confidence badges, side panel = PredictionPanel for the selected photo (predicted changes with Lightroom keys, confidence components and reasons, Lightroom outcome with mismatches; Looks right / Reject / Apply this photo). Confirm dialog states the safety steps and shows the backend's blockers verbatim.
- **Review**: session selector; filters _Needs attention_ (default: below medium threshold, unfamiliar, failed apply), _All pending_, _Every prediction_; large preview, prev/next, filmstrip with confidence badges, PredictionPanel. Review works offline; Apply requires Lightroom.
- **Settings**: General (theme, review thresholds used by Review), Lightroom (setup, capability matrix, apply-safety facts — snapshot + read-back cannot be disabled), Performance (engine facts, workers, encoder note), Storage (data root, cache limit), Privacy, Updates (version metrics, check, install, auto-download, channel), Diagnostics (health, open logs, copy bundle, restart engine, warnings table).

## Empty/error states (spec §18)

No Style · Lightroom disconnected (Test connection / Plugin setup; Apply disabled with reason) · No sidecars (report warning) · Low coverage (report warning) · Update failed (app usable, retry) · Engine unavailable (badge + restart, window still opens) · Non-Tauri (explains how to run) · Session with no photos / ingest failed · Nothing needs attention · Apply refused (blockers listed).

## Accessibility

Keyboard navigable (buttons/links/radiogroups), visible focus ring token, semantic buttons, labels on icon buttons, confidence/state never by colour alone (badges carry text), contrast-checked tokens, reduced-motion media query.
