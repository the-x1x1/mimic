# Release process

## Versioning

Single source: root `package.json`. `node scripts/sync-version.mjs <version>` propagates everywhere (desktop package, contracts, ui, test-fixtures, Tauri config, Cargo workspace, engine PEP 440), then `cargo update -w` refreshes `Cargo.lock`. `--check` runs in `test.ps1` and CI. Tags are `vX.Y.Z[-pre]`. Pre-releases: `alpha.N` → PyPI-style `aN`.

## Slice discipline

One coherent slice per version: code + tests + docs + CHANGELOG entry + PROJECT_STATUS update in one commit. A big release is several slices, each with its own version and CHANGELOG entry; only the last one is tagged.

## Checklist per release

1. `.\scripts\test.ps1` green.
2. `.\scripts\validate.ps1 -Full` green (adds the pre-tag verification: version consistency, CHANGELOG section, PROJECT_STATUS mentions the version, updater key policy).
3. Docs swept for anything the release made untrue (README status table, PROJECT_STATUS, ROADMAP, MIGRATION_AUDIT, this file).
4. Commit authored `the-x1x1 <connersalt123@outlook.com>`, no attribution trailers.
5. Push branch, PR to `main`, rebase-merge, tag from `main`.
6. The `Release` workflow builds the Windows installer, signs the updater artifact, generates `latest.json` and checksums, and creates a **draft** GitHub Release with all assets; its last job verifies every asset is attached. Runs for one tag don't overlap (`concurrency`), and a run that finds a release for its tag already there stops rather than make a second: a draft doesn't reserve its tag, and two runs for the one push of 0.10.0-alpha.9 left two. A broken draft from a failed run has to be deleted before running again.
7. `gh release edit vX.Y.Z --draft=false --latest` publishes it.
8. `.\scripts\verify-release.ps1 -Tag vX.Y.Z` downloads `latest.json`, checks the signature field and asset reachability.

## Secrets the owner must add to GitHub Actions

| Secret                               | Purpose                                                                |
| ------------------------------------ | ---------------------------------------------------------------------- |
| `TAURI_SIGNING_PRIVATE_KEY`          | contents of the minisign private key from `pnpm tauri signer generate` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password (may be empty)                                            |

Until a real key pair is generated and its public key replaces the development key in `tauri.conf.json`, only `alpha` tags may be released.

## Mimic's registration with Microsoft

Signing in to Outlook.com, Hotmail and Microsoft 365 mailboxes (`sources::oauth`) needs Mimic registered as an app with Microsoft. The registration's client id is compiled in from `MIMIC_MICROSOFT_CLIENT_ID`, which the release workflow reads from a repository **variable** of that name. It is not a secret — an app on someone's computer cannot keep one — so it is a variable, not a secret. Without it the build still succeeds; the variable check in the release workflow warns, and the app says it can't sign in with Microsoft and offers nothing that would fail.

Registering, once, in the Microsoft Entra admin center (entra.microsoft.com, signed in with any Microsoft account):

1. **App registrations → New registration.** Name: `Mimic`. Supported account types: _Accounts in any organizational directory and personal Microsoft accounts_ (both Outlook.com and Microsoft 365).
2. **Redirect URI:** platform _Public client/native (mobile & desktop)_, value `http://localhost`. Microsoft accepts any port on `localhost` for this platform, which the sign-in needs (it listens on a port the system picks). Registered under _Web_ instead, the token exchange fails asking for a client secret.
3. **Register**, and copy the _Application (client) ID_.
4. **API permissions → Add a permission → Microsoft Graph → Delegated:** `IMAP.AccessAsUser.All`, `SMTP.Send` and `offline_access`. Mimic asks for them at sign-in either way (`SMTP.Send` since 0.10.0-alpha.23, to send replies); listing them lets an organisation's admin approve Mimic ahead of time. A work or school mailbox also needs its admin to allow authenticated SMTP for it (Exchange's `SmtpClientAuthenticationDisabled` off) before replies can be sent from it; reading is not affected.
5. Set the variable: `gh variable set MIMIC_MICROSOFT_CLIENT_ID --repo the-x1x1/mimic --body <client id>`.

For a local build, set `MIMIC_MICROSOFT_CLIENT_ID` in the environment before `pnpm tauri dev` or `.\scripts\build.ps1`; cargo rebuilds when it changes.

Since 0.10.0-alpha.59 a build without one, or a user who would rather use their own, can paste the client id into Settings → **Signing in with Microsoft and Google** instead (`commands::sign_in_setup`): it is kept in the secret store, used from the next sign-in on, and forgotten there to go back to the build's. A mailbox signed in under one client id must be signed in again under another.

Work and school tenants decide whether their users may approve an app themselves; many allow it only for apps from a verified publisher. Until Mimic's publisher is verified (a Microsoft Partner Network ID on the registration), someone in such a tenant needs their admin to approve Mimic — Microsoft says so on its own page, and the connect dialog tells the user to expect it. Personal accounts are not affected.

## Mimic's registration with Google

Signing in to Gmail and Google Workspace mailboxes with Google (0.10.0-alpha.26) needs Mimic registered as an app with Google. Without it, Gmail connects with an app password, as it always has. The registration gives a client id, compiled in from `MIMIC_GOOGLE_CLIENT_ID` (a repository **variable**), and a client secret, compiled in from `MIMIC_GOOGLE_CLIENT_SECRET` (a repository **secret**). Google says a desktop app's secret is not treated as confidential — anyone with the installer has it — but its token endpoint wants it, and keeping it a repository secret keeps it out of the build log. A build needs both; the release workflow warns when either is missing.

Registering, once, in the Google Cloud console (console.cloud.google.com, signed in with any Google account):

1. **Create a project** named `Mimic`.
2. **APIs & Services → Library → Gmail API → Enable.** IMAP and SMTP don't use the API, but its scope, `https://mail.google.com/`, is listed only once it is enabled.
3. **Google Auth Platform → Branding** (the OAuth consent screen): app name `Mimic`, a support email; **Audience:** _External_. **Data Access → Add scopes:** `https://mail.google.com/`.
4. **Audience → Publishing status.** In _Testing_, only the test users listed there can sign in, and Google lets their sign-ins lapse after seven days — Mimic would ask every week. _In production_ without Google's verification, anyone can sign in after a warning that Google hasn't verified the app (up to 100 people), and sign-ins last. For the owner's own use, publish it; verification of a restricted scope (a security assessment) is only worth it for wide distribution.
5. **Clients → Create client:** application type _Desktop app_, name `Mimic`. Copy the client id and the client secret. A desktop client takes the browser back to `http://127.0.0.1` on any port, which the sign-in needs; nothing else is registered.
6. Set them: `gh variable set MIMIC_GOOGLE_CLIENT_ID --repo the-x1x1/mimic --body <client id>` and `gh secret set MIMIC_GOOGLE_CLIENT_SECRET --repo the-x1x1/mimic --body <client secret>`.

For a local build, set both in the environment before `pnpm tauri dev` or `.\scripts\build.ps1`. Or, since 0.10.0-alpha.59, paste both into Settings → **Signing in with Microsoft and Google**, as for Microsoft; the secret is kept in the secret store and never shown again.

Google Workspace admins can block apps that aren't on their list (Security → API controls); a Workspace user then sees that from Google when signing in, and an app password, if the admin allows those, still works.

## Manual release checklist (spec §27.5)

Fresh install → launch → update from the previous public release → declare an identity → add a source → validate → import → cancel mid-import and resume → analyze → compose against a local provider → compose against a hosted provider → edit and record what was sent → delete a person and confirm the preview matched → re-analyze → delete everything → restart during an interrupted import → GitHub offline → engine crash → provider unreachable. Record results in PROJECT_STATUS.
