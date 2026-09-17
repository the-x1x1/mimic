# Privacy

**Everything stays on this computer.** Mimic is local-first by design and by default.

- RAW files, sidecars, previews, embeddings, model weights, edit history and corrections never leave the machine. There is no telemetry.
- The only network request Mimic makes is the update check against `https://github.com/the-x1x1/mimic/releases/latest/download/latest.json` (and the download of a signed installer when an update is available). It can be disabled in Settings › Updates.
- A future ONNX encoder download is manifest-driven, SHA-256 verified and only happens when the user enables it; no manifest ships in 0.1.
- Any future network feature (Style sharing, remote training, sync) must be explicitly opt-in and will be listed here before it ships. The `privacy.networkFeatures` setting exists, is off, and is disabled in the UI because no such feature exists.
- The diagnostic bundle (Settings › Diagnostics) contains no tokens and redacts file paths to file names unless _Include full file paths_ is turned on. Nothing is sent anywhere; you copy it to the clipboard yourself.
- Logs are written to `%LOCALAPPDATA%\Formicaria\Mimic\logs`, rotated daily, 14 files kept. No pixel data, no tokens.
- The Lightroom plugin sends metadata and develop settings to Mimic over `127.0.0.1` only. Photos are read from disk by Mimic directly and are never transmitted through the plugin.
