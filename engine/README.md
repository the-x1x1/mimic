# mimic-engine

Python sidecar for Mimic. It never runs as a server: the desktop app spawns it
with `mimic-engine serve` and talks newline-delimited JSON over stdin/stdout.

Most of Mimic's work happens in Rust. What lives here is the numeric work that
benefits from numpy — text embeddings, similarity search, and the held-out
evaluation that compares a generated reply with the one the user really sent.

```
uv sync --extra onnx --dev          # --extra directml on Windows: the two ONNX Runtimes can't both be installed
uv run pytest
uv run mimic-engine --version
uv run mimic-engine methods                             # protocol methods
uv run mimic-engine embed "let me know if that works"   # one vector
uv run mimic-engine compare "Yes, certainly." "yeah ok" # resemblance components
```

The embedding provider is the sentence encoder named by `models/manifests/`
(all-MiniLM-L6-v2) when the app has downloaded it and every file matches the
SHA-256 its manifest pins, run with ONNX Runtime and the encoder's own
tokenizer; otherwise `lexical_v1`: hashed word and character n-grams,
L2-normalized, which is not semantic. `health` says which is in use and why,
so the app never implies a model that is not there, and `encoder.reload` looks
again after a download. The engine never touches the network. See
`docs/VOICE_ENGINE.md` and `docs/ARCHITECTURE.md`.
