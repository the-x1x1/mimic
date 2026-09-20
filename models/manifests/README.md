# Encoder manifests

Text-embedding encoders are downloaded on demand and pinned by SHA-256. No
model binaries are committed to this repository.

Retrieval in 0.6.0 does not use an encoder: it ranks candidates lexically
(`crates/mimic-core/src/retrieval`). This directory is the mechanism the
Phase 2 embedding work will use, kept because the download-and-verify plumbing
in `engine/src/mimic_engine/embeddings/encoder.py` still expects it.
