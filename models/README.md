# Models

No model binaries are committed. `manifests/*.json` (validated by `manifest.schema.json`) describe downloadable image encoders: id, semantic version, file name, SHA-256, size, URL, license, input size/normalisation, runtime compatibility. The engine's `EncoderManager` loads the first `image_encoder` manifest whose file exists in `%LOCALAPPDATA%\Formicaria\Mimic\models\encoders\` and passes SHA-256; otherwise it uses the built-in `stats_v1` embedding and says so in Settings › Performance.

No manifest ships in 0.1.0-alpha.1: adding one requires a reviewed pin (hash computed from the downloaded file) and a UI download action, both roadmap items.
