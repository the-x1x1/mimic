"""One-call analysis of a photo: preview → stats → scene labels → embedding."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from mimic_engine.embeddings.encoder import EncoderManager, embedding_artifact_id, save_embedding
from mimic_engine.features.scene import scene_labels
from mimic_engine.features.stats import FEATURE_VERSION, compute_stats, flat_vector
from mimic_engine.raw.metadata import read_metadata
from mimic_engine.raw.preview import PreviewError, cached_preview


def analyze_image(
    path: str | Path,
    *,
    asset_id: str | None = None,
    fast_hash: str | None = None,
    previews_dir: str | Path | None = None,
    embeddings_dir: str | Path | None = None,
    encoder: EncoderManager | None = None,
    metadata: dict[str, Any] | None = None,
) -> dict[str, Any]:
    p = Path(path)
    if not p.is_file():
        raise FileNotFoundError(str(p))
    md = metadata if metadata is not None else read_metadata(p)
    if fast_hash is None:
        from mimic_engine.artifacts.hashing import fast_hash as fh

        fast_hash = fh(p)
    try:
        rgb, preview_path, decoder = cached_preview(p, previews_dir, fast_hash)
    except PreviewError as e:
        raise PreviewError(str(e)) from e
    stats = compute_stats(rgb)
    scene = scene_labels(stats, md)
    names, vector = flat_vector(stats, md)
    embedding_id = None
    provider = None
    if encoder is not None:
        provider = encoder.provider
        emb = encoder.embed(rgb)
        if embeddings_dir:
            embedding_id = embedding_artifact_id(provider, fast_hash)
            save_embedding(embeddings_dir, embedding_id, emb, provider)
    del rgb  # release decoded pixels explicitly (spec §30)
    return {
        "assetId": asset_id,
        "path": str(p),
        "featureVersion": FEATURE_VERSION,
        "decoder": decoder,
        "previewPath": preview_path,
        "metadata": md,
        "histogram": stats["histogram"],
        "luminance": stats["luminance"],
        "color": stats["color"],
        "clipping": stats["clipping"],
        "sharpness": stats["sharpness"],
        "noiseEstimate": stats["noiseEstimate"],
        "sceneLabels": scene,
        "vectorNames": names,
        "vector": vector,
        "embeddingArtifactId": embedding_id,
        "embeddingProvider": provider,
    }
