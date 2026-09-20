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

Default: `http://127.0.0.1:11434/v1` with `llama3.1:8b`, which is Ollama out of the box. LM Studio and llama.cpp's server speak the same shape.

`info().local` is computed from the configured URL rather than hard-coded. Pointing "local" at `192.168.1.50` stops it being local, the badge changes, and the description says the messages are sent to it. A provider that claims to be private while it is not is worse than no provider.

### `anthropic` — the Claude Messages API

For users who would rather have the quality than the locality, having been told which it is. The description states exactly what is sent: the message being replied to, the intent, and a handful of the user's own past messages. It refuses to run without an API key rather than failing at the network.

### `mock` — deterministic, tests only

Not a stub that returns a fixed string: it records every request and echoes back the prompt it was given, which is what lets a test assert that the assembler actually put the examples, the intent and the length target where it claimed to. Never listed in the app.

## Choosing one

`ProviderRegistry::default_id` returns the first **local** provider, falling back to the first of any kind. Sending a person's private correspondence to a third party is not a sensible default, and the default should not depend on the order a list happens to be built in.

The user's choice is stored in `generation.provider`. Changing any `generation.*` setting rebuilds the registry, so an endpoint or model change takes effect without a restart.

## Credentials

**Known limitation, stated plainly rather than papered over.** Credentials live in `credentials/credentials.json` under the app data directory, with `0600` permissions on Unix. On Windows that directory is already ACL'd to the user. So another _user_ cannot read it — but any program running as this user can.

This is not the OS credential store, and the code says so in `apps/desktop/src-tauri/src/secrets.rs`. Moving to DPAPI on Windows and Keychain/Secret Service elsewhere is tracked in `docs/ROADMAP.md` as Phase 2 work.

What is already true, and tested:

- credentials are never written to the database;
- the diagnostics bundle strips `token`, `apiKey`, `api_key`, `password` and `secret` at every depth;
- the Settings screen lists which credential _keys_ have a value, never the values;
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
