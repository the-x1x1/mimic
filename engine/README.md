# mimic-engine

Python sidecar for Mimic. It never runs as a server: the desktop app spawns it
with `mimic-engine serve` and talks newline-delimited JSON over stdin/stdout.

```
uv sync --all-extras --dev
uv run pytest
uv run mimic-engine --version
uv run mimic-engine scan D:\Photos\2025        # ad-hoc scan report
uv run mimic-engine parse-xmp IMG_0001.xmp      # dump parsed crs settings
```

See `docs/ML_PIPELINE.md` and `docs/ARCHITECTURE.md` §20 for the protocol.
