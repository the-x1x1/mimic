# Update system

Mimic updates from GitHub Releases through the Tauri 2 updater plugin.

## Release structure

`Mimic_<v>_x64-setup.exe` (NSIS, per-user install), `Mimic_<v>_x64-setup.exe.sig` (minisign signature produced by `tauri build` with `createUpdaterArtifacts: true`), `latest.json`, `SHA256SUMS.txt`, `Mimic.lrplugin-<v>.zip`, release notes from CHANGELOG.

`latest.json`:

```json
{
  "version": "0.1.1",
  "notes": "…",
  "pub_date": "2026-…Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<.sig contents>",
      "url": "https://github.com/the-x1x1/mimic/releases/download/v0.1.1/Mimic_0.1.1_x64-setup.exe"
    }
  }
}
```

Its shape is validated by the release workflow, by `scripts/verify-release.ps1 -Tag vX.Y.Z`, and by the `LatestJson` zod contract test.

## Signing

`pnpm tauri signer generate -w <path>` creates the key pair. Public key → `tauri.conf.json` `plugins.updater.pubkey`. Private key + password → GitHub Actions secrets `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The repository ships a **development** public key (`docs/DEV_UPDATER_PUBKEY.txt`) so alpha builds can be produced; `verify-release.ps1 -PreTag` and the release workflow refuse any non-alpha tag while it is in use. Platform code signing is added when a certificate exists; updater signature verification is mandatory regardless.

## Behaviour (spec §24.3)

- 20 s after launch, then every 6 h ± 20 min jitter, and on _Check for updates_.
- With _Download updates automatically_ (default on): discover → verify version/target → download → signature verified by the plugin → staged (`update_state.staged_version`).
- Install happens only through `can_install_update_now`, which refuses while any job is queued/running/paused. The UI offers _Install and restart_; passive NSIS mode avoids babysitting.
- GitHub unreachable → `update_state.last_update_result = error`, app fully usable, retry later. Never blocks launch.
- Channels: `stable` (default) and `beta` are stored; both currently read the same `latest.json` endpoint because no beta feed exists yet. Nightly builds never publish `latest.json`.

## Database safety across updates

On first launch after an update the app opens the database, and if the schema version is behind, writes a backup to `data/backups/` and migrates transactionally. A failed migration leaves the original file untouched (the transaction rolls back) and the backup exists. Downgrades do not migrate down; restore the backup manually.

## Documented update-path test (to run once two releases exist)

1. Install v0.1.0-alpha.1 from the release page on a clean Windows user.
2. Publish v0.1.0-alpha.2 (any change) with the same signing key.
3. Launch the old app: within ~20 s Settings › Updates shows _Latest seen 0.1.0-alpha.2_; with auto-download on it becomes _Staged_.
4. Start a folder scan; press _Install and restart_: the guard refuses with the active job named.
5. Let the scan finish; press _Install and restart_: app relaunches as 0.1.0-alpha.2; Settings › Diagnostics shows the same schema version and asset counts; `data/backups/` gains a file only if the schema changed.
6. Tamper test: replace `.sig` on a local mirror → the plugin rejects the download (signature error surfaces in _Last check failed_).
   Record the outcome in PROJECT_STATUS (row _Tested update path_).
