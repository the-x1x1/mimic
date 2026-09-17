# UI spec

Dark graphite workspace, neutral surfaces so photographs dominate, one restrained amber accent used for controls only (never over previews), rounded but not bubbly, subtle borders, short functional motion, reduced-motion respected. Tokens in `packages/ui/src/tokens.css`.

## Navigation

Sidebar: Home · Styles · Sessions · Review · Settings. No per-engine pages. Top bar: Lightroom connection badge, engine badge, unobtrusive update badge. Sidebar footer: job tray with item-based progress and Stop.

## Screens (0.1)

- **Onboarding**: 1 source choice (Connect Lightroom / Train from folders / Explore with DEMO data) → 2 plugin setup (Continue enabled only after a real handshake) → 3 first Style + scan → 4 data quality with live item progress. Train step arrives in 0.2.0 and says so.
- **Home**: primary action _Edit a New Session_ (states 0.3.0), secondary _Train / Improve a Style_, active Style summary (name, version or "not trained", examples, No-Touch Rate "—" until legitimate), recent activity, compact system status, privacy note. Empty state when no Style.
- **Styles**: list cards (name, version badge, examples, cameras, updated). Detail tabs: Overview (data quality per library, honest training notice), Training Data (libraries, browse ingested photos), Versions (empty until 0.2.0), Corrections (empty until 0.4.0). Actions: Add Training Data, Train New Version (disabled with reason), delete.
- **Sessions / Review**: empty states that name the release where they arrive; no dead controls.
- **Settings**: General (theme, review thresholds), Lightroom (setup, capability matrix, apply safety), Performance (engine facts, workers, encoder note), Storage (data root, cache limit), Privacy, Updates (version metrics, check, install, auto-download, channel), Diagnostics (health, open logs, copy bundle, restart engine, warnings table).

## Empty/error states (spec §18)

No Style · Lightroom disconnected (Test connection / Plugin setup) · No sidecars (report warning) · Low coverage (report warning) · Update failed (app usable, retry) · Engine unavailable (badge + restart, window still opens) · Non-Tauri (explains how to run).

## Accessibility

Keyboard navigable (buttons/links/radiogroups), visible focus ring token, semantic buttons, labels on icon buttons, confidence/state never by colour alone (badges carry text), contrast-checked tokens, reduced-motion media query.
