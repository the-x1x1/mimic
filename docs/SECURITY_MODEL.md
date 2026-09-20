# Security model

## Assets and threats

Assets: the user's imported correspondence (their own words and other people's), the SQLite database, provider credentials, the updater trust root.

Threats considered: a runaway or crashing sidecar; a malformed or hostile export file; a compromised update channel; message content escaping into logs, diagnostics or an error string; a credential leaking into the database or a bundle; a provider that is not what it claims to be.

Not in scope, and stated rather than implied: another program running as the same user. Provider credentials are in an owner-only file, not the OS credential store, so any process with this user's rights can read them. See `docs/MODEL_PROVIDERS.md`.

## Controls

| Area            | Control                                                                                                                                                                                                                                                                                                                         |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Network         | No inbound listener of any kind. Outbound: the update check, and whatever the selected model provider requires. The default provider is loopback-only.                                                                                                                                                                          |
| Engine          | Spawned via argument array with `kill_on_drop`; NDJSON only; 32 MiB per-message cap in both directions with a restart on violation; a restart budget of 5, then manual; every handler exception becomes a structured error; no `eval`, no shell.                                                                                |
| Imports         | Read-only. An export file is never modified or moved. A malformed file produces a `source_malformed` error, not a panic; the connector tests include truncated JSON and headerless mail.                                                                                                                                        |
| Files           | Everything Mimic writes is under the per-user app data directory. The `credentials/` directory is `0700` and the file inside it `0600` on Unix; `%LOCALAPPDATA%` is already per-user ACL'd on Windows.                                                                                                                          |
| Database        | Per-user app data; migrations transactional with a pre-migration backup.                                                                                                                                                                                                                                                        |
| Credentials     | Never in the database, never in settings, never in a log. Providers receive them through `SecretStore`. Diagnostics strip `token`, `apiKey`, `api_key`, `password` and `secret` at every depth.                                                                                                                                 |
| Message content | Never logged by default. Provider errors are truncated to one line and carry the provider's status and message, never the request body. The diagnostics bundle contains counts and error events, never bodies.                                                                                                                  |
| Webview         | CSP restricted to `self`, `ipc:` and inline styles. The Tauri asset protocol is disabled entirely — the app has no reason to load a file from disk into the webview.                                                                                                                                                            |
| Updater         | Tauri updater with signature verification against the public key embedded in `tauri.conf.json`; the private key exists only as `TAURI_SIGNING_PRIVATE_KEY` in GitHub Actions; the release workflow refuses to build without it and refuses the development public key for non-alpha tags; installs never interrupt active jobs. |
| Models          | Encoder manifests are SHA-256 pinned; a manifest missing `id`, `url`, `sha256` or `dims` is refused. The lexical fallback needs no download.                                                                                                                                                                                    |
| Supply chain    | Lockfiles committed; `cargo audit` and `pnpm audit` in CI (warning level in 0.x); a secret scan on every push.                                                                                                                                                                                                                  |

## Known gaps

- Provider credentials are not in the OS credential store. Tracked in `docs/ROADMAP.md` as Phase 2.
- No commercial code-signing certificate (SmartScreen warnings on first install); updater signatures still protect updates.
- The development updater key pair was generated during repository creation. Acceptable for alpha builds only; it must be rotated before a stable release (see `docs/RELEASE_PROCESS.md`).
- A hosted provider receives message content by design. The user chooses it, the top bar names it, and the Settings description says exactly what is transmitted — but Mimic has no way to verify what the provider does with it.
