# mimic-engine

Python sidecar for Mimic. It never runs as a server: the desktop app spawns it
with `mimic-engine serve` and talks newline-delimited JSON over stdin/stdout.

Most of Mimic's work happens in Rust. What lives here is the numeric work that
benefits from numpy — text embeddings, similarity search, and the held-out
evaluation that compares a generated reply with the one the user really sent.

```
uv sync --all-extras --dev
uv run pytest
uv run mimic-engine --version
uv run mimic-engine methods                             # protocol methods
uv run mimic-engine embed "let me know if that works"   # one vector
uv run mimic-engine compare "Yes, certainly." "yeah ok" # resemblance components
```

The embedding provider in this release is `lexical_v1`: hashed word and
character n-grams, L2-normalized. It is not semantic and `health` reports that
it is not, so the app can say so rather than implying a model that is not
there. See `docs/VOICE_ENGINE.md` and `docs/ARCHITECTURE.md` for the protocol
and the plan for a real encoder.
