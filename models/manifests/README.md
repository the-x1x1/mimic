# Encoder manifests

Text-embedding encoders are downloaded when the user asks, and pinned by
SHA-256. No model binaries are committed to this repository, and none ship
with the app.

Each manifest names one encoder and every file it needs: where to download it
from (a URL pinned to a revision, never a branch), its size, and its SHA-256.

- The app downloads the files into its encoders folder, under the manifest's
  `id`, checking each digest as it goes (`crates/mimic-core/src/encoder.rs`).
  A file that does not match is deleted, never kept.
- The engine runs an encoder only while every file matches its manifest,
  checked again each time it loads one
  (`engine/src/mimic_engine/embeddings/encoder.py`). Otherwise it uses the
  lexical fallback and says so.
- The engine itself never touches the network.

`all-minilm-l6-v2.json` is all-MiniLM-L6-v2 (Apache-2.0), the ONNX export
from its repository, with its tokenizer: 91 MB. It turns a message into 384
numbers; messages that mean much the same thing get numbers close together,
which retrieval uses to find the past replies worth showing a draft.

It is the full-precision export, not one of the 8-bit ones a quarter of the
size: those work out their scale from everything in a batch, so a message's
numbers would depend on which messages it was read with. The full-precision
model gives a message the same numbers however it is batched.
