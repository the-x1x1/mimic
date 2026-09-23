# Model providers

Generation is the one step that may involve a language model, and the one step where the user's words can leave their computer. The boundary is narrow on purpose.

## The contract

```rust
pub trait ModelProvider: Send + Sync {
    fn info(&self) -> ProviderInfo;
    fn generate(&self, request: &GenerationRequest) -> ProviderResult<GenerationResponse>;
    fn health(&self) -> ProviderResult<()>;
}
```

A provider takes an assembled prompt and returns text. It never sees the database, never decides what goes in the prompt, and never logs message content. `health` is a reachability check that sends no message content anywhere.

`ProviderInfo` carries a `local` flag and a one-sentence `description` written for the user, both shown in the UI. It carries no credential field: credentials come from `SecretStore`, which the shell implements.

## Shipped providers

### `local` — an OpenAI-compatible endpoint on this machine

Default: `http://127.0.0.1:11434/v1` with `llama3.2:3b`, which is Ollama out of the box. The small model is deliberate: setup downloads it for people who have never installed one, and two gigabytes on a laptop is a different proposition from five. Anyone who wants a larger one changes it in Settings. LM Studio and llama.cpp's server speak the same shape.

`info().local` is computed from the configured URL rather than hard-coded. Pointing "local" at `192.168.1.50` stops it being local, the badge changes, and the description says the messages are sent to it. A provider that claims to be private while it is not is worse than no provider.

### `anthropic` — the Claude Messages API

For users who would rather have the quality than the locality, having been told which it is. The description states exactly what is sent: the message being replied to, the intent, and a handful of the user's own past messages. It refuses to run without an API key rather than failing at the network.

### `mock` — deterministic, tests only

Not a stub that returns a fixed string: it records every request and echoes back the prompt it was given, which is what lets a test assert that the assembler actually put the examples, the intent and the length target where it claimed to. Never listed in the app.

## Choosing one

`ProviderRegistry::default_id` returns the first **local** provider, falling back to the first of any kind. Sending a person's private correspondence to a third party is not a sensible default, and the default should not depend on the order a list happens to be built in.

The user's choice is stored in `generation.provider`. Changing any `generation.*` setting rebuilds the registry, so an endpoint or model change takes effect without a restart.

## Credentials

**On Windows, credentials are sealed to the user's Windows account** before they are written. `apps/desktop/src-tauri/src/secrets.rs` passes each value through DPAPI (`CryptProtectData`: current-user scope, `CRYPTPROTECT_UI_FORBIDDEN`, and an entropy string of Mimic's own) and writes the result, hex-encoded, to `credentials/secrets.json` under the app data directory. A copy of that file — a backup, a sync, the disk read by another account or another operating system — cannot be opened without the user's Windows password.

**What it does not stop, stated plainly:** a program running as this same user can ask Windows to unseal the values, exactly as it could the browser's saved passwords, and so can an administrator of this computer (and, on a domain account, the domain's DPAPI backup key). The Privacy card says so.

- **Moving in.** Versions before 0.10.0-alpha.6 wrote the values unsealed to `credentials/credentials.json`. On every start and before every save while that file exists, each of its values not already dealt with is sealed into `secrets.json` — every one the first time, and afterwards only one an older version of Mimic saved there since. The values are written and read back from disk before they are marked as dealt with (`taken`: a SHA-256 of key and value, sealed like the values and kept after the old file is gone), and the old file is deleted only after that. So a value that opens is never lost with it, and one changed or cleared since is not brought back by it, or by a restored copy of it. A mark is written unsealed only while sealing is refused — when the old file, holding the value itself, is still on disk — and is sealed, or dropped, once the old file is gone. If `secrets.json` itself has to be set aside, its marks go with it, and whatever an old file still on disk holds is taken again; the same happens when the marks cannot be opened here (another account or computer, or a reset password) — an old file whose values open is preferred to marks that do not. A byte-order mark and non-string values are tolerated; an old file that is not a JSON object at all (the old store did not write atomically) is deleted, since nothing in it can be used and what can be read of it is unsealed. Deleting does not erase: a key saved before 0.10.0-alpha.6 may survive in backups or free disk space, and replacing it is the way to be sure.
- **When Windows will not seal**, nothing is written unsealed. The old file stays and is used for the session, a new value that cannot be sealed is not saved and the command says why, and clearing a value still works — it is marked as dealt with, so the next start cannot bring it back (an old file that is there but cannot be read at that moment fails the clear instead); the old file itself is never rewritten. When the old file cannot be deleted, saves go on working and it is tried again at every start and save. Either way the Privacy card says something unsealed is still on disk (`CredentialState.unsealedLeft`).
- **Two copies of the app** saving one after the other do not undo each other: every save starts from the file on disk (a file deleted by hand is not written back), and each process writes through its own temporary file; temporary files a crash left behind are removed after a minute. Two saving at the very same moment are not guarded against — running two copies is not supported.
- **A value that does not open here** — sealed on another Windows account or computer, or before the account's password was reset by someone else — is kept, not deleted, and reported as locked (`CredentialState.locked`). The Settings key field asks for the key again, and **New password** under Your mail takes a mailbox's again (`sources::imap::replace_password`: the login is tried first, and nothing imported is touched).
- **An unreadable `secrets.json`** is renamed aside and saves start a new one (`unreadable: "setAside"`); one written in another format, or that cannot be moved aside, is left as it is and nothing is saved over it (`"leftAlone"`). The Privacy card says so in that session, and it is logged.
- **What the screen says is computed** by the store: `protection` is what this build does (`account` with DPAPI, `file` without), never what was meant to happen.
- **Other platforms.** Development builds that are not Windows write the values unsealed to an owner-only (`0600`) file and report `file`. macOS, which would use the Keychain, is unbuilt.
- **Going back** to 0.10.0-alpha.5 or earlier after 0.10.0-alpha.6 has started means entering the key and mailbox passwords again: older versions read only the old file. What is entered there is moved in when 0.10.0-alpha.6 or later starts again — unless it is the same value that was moved in under that key before, which cannot be told apart from a restored copy of the old file.

What is already true, and tested:

- credentials are never written to the database;
- the diagnostics bundle strips `token`, `apiKey`, `api_key`, `password` and `secret` at every depth;
- the Settings screen lists which credential _keys_ have a value, never the values;
- saves replace the file in one step (write, flush, rename), and a file that cannot be parsed is set aside rather than overwritten;
- provider errors are truncated to one line and carry the provider's status and message, never the request body.

## Errors

| Code                   | Means                                                         |
| ---------------------- | ------------------------------------------------------------- |
| `provider_config`      | Misconfigured — usually a missing API key.                    |
| `provider_unreachable` | Nothing answered. For `local`, usually Ollama is not running. |
| `provider_refused`     | It answered with a non-2xx status; the reason is included.    |
| `provider_malformed`   | It answered with something unusable.                          |
| `provider_unknown`     | No provider by that id is configured.                         |

## Adding a provider

1. Implement `ModelProvider` in `crates/mimic-core/src/providers/`.
2. Be honest in `info()`. `local` means the request does not leave the machine, and it must be computed rather than asserted.
3. Read credentials through `SecretStore`, never from settings or the database.
4. Never log the request or the response. Errors get the status and the provider's message, nothing else.
5. Register it in `apps/desktop/src-tauri/src/providers_config.rs`, and add the tests that assert its locality claim is true.
