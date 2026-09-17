import numpy as np
import pytest

from mimic_engine.embeddings.encoder import STATS_DIM, EncoderManager, load_embedding, stats_embedding
from mimic_engine.features.analyze import analyze_image
from mimic_engine.features.scene import scene_labels
from mimic_engine.features.stats import FEATURE_VERSION, compute_stats, flat_vector
from mimic_engine.raw.preview import PreviewError, cached_preview


def test_stats_are_deterministic_and_bounded(fixtures_dir):
    rgb, _, decoder = cached_preview(fixtures_dir / "images" / "demo_landscape_sky.jpg", None, None)
    assert decoder == "pillow"
    a = compute_stats(rgb)
    b = compute_stats(rgb)
    assert a == b
    assert a["featureVersion"] == FEATURE_VERSION
    assert abs(sum(a["histogram"]["luminance"]) - 1.0) < 1e-6
    assert 0 <= a["luminance"]["mean"] <= 1
    assert a["color"]["skyLikeFraction"] > 0.5
    names, vec = flat_vector(a, {"iso": 400, "aperture": 2.8, "shutterSpeed": 1 / 200, "focalLength": 50})
    assert len(names) == len(vec) == 32 + 2 + 7 + 2 + 3 + 3 + 5 + 2 + 2 + 5
    assert all(isinstance(v, float) and v == v for v in vec)


def test_scene_labels_track_fixture_intent(fixtures_dir):
    def labels(name, md=None):
        rgb, _, _ = cached_preview(fixtures_dir / "images" / name, None, None)
        return scene_labels(compute_stats(rgb), md)["labels"]

    sky = labels("demo_landscape_sky.jpg", {"iso": 100, "focalLength": 24, "aperture": 8})
    low = labels("demo_lowlight_indoor.jpg", {"iso": 6400})
    back = labels("demo_backlit.jpg")
    high = labels("demo_highkey_product.tif")
    assert sky["skyDominant"] > 0.5 and sky["outdoor"] > 0.5
    assert low["lowLight"] > 0.6 and low["lowKey"] > 0.3
    assert back["backlit"] > 0.4
    assert high["highKey"] > 0.6 and high["lowLight"] == 0.0


def test_preview_cache_and_corrupt_file(fixtures_dir, tmp_path):
    src = fixtures_dir / "images" / "demo_backlit.jpg"
    rgb1, path1, dec1 = cached_preview(src, tmp_path, "fh1:abc")
    rgb2, path2, dec2 = cached_preview(src, tmp_path, "fh1:abc")
    assert dec1 == "pillow" and dec2 == "cache" and path1 == path2
    assert rgb1.shape == rgb2.shape and max(rgb1.shape) <= 768
    with pytest.raises(PreviewError):
        cached_preview(fixtures_dir / "images" / "corrupt_not_an_image.jpg", None, None)


def test_stats_embedding_and_manager_fallback(fixtures_dir, tmp_path):
    rgb, _, _ = cached_preview(fixtures_dir / "images" / "demo_lowlight_indoor.jpg", None, None)
    v = stats_embedding(rgb)
    assert v.shape == (STATS_DIM,) and abs(float(np.linalg.norm(v)) - 1.0) < 1e-5
    mgr = EncoderManager(tmp_path / "enc", None)
    assert mgr.provider == "stats_v1"
    assert "stats_v1" in mgr.status()["reason"]
    # Manifest present but model not downloaded → still stats fallback, with a clear reason.
    manifests = fixtures_dir.parents[0] / "models" / "manifests"
    mgr2 = EncoderManager(tmp_path / "enc", manifests)
    assert mgr2.provider == "stats_v1"
    assert "not downloaded" in mgr2.status()["reason"] or "no encoder manifest" in mgr2.status()["reason"]


def test_analyze_image_end_to_end(fixtures_dir, tmp_path):
    r = analyze_image(
        fixtures_dir / "images" / "demo_landscape_sky.jpg",
        asset_id="a1",
        previews_dir=tmp_path / "previews",
        embeddings_dir=tmp_path / "emb",
        encoder=EncoderManager(None, None),
    )
    assert r["assetId"] == "a1" and r["featureVersion"] == FEATURE_VERSION
    assert r["previewPath"] and r["embeddingArtifactId"].startswith("emb_")
    vec, provider = load_embedding(tmp_path / "emb", r["embeddingArtifactId"])
    assert vec.shape == (STATS_DIM,) and provider == "stats_v1"
    assert r["sceneLabels"]["primary"] in r["sceneLabels"]["labels"] or r["sceneLabels"]["primary"] == "unclassified"
    assert r["metadata"]["width"] == 720
    with pytest.raises(FileNotFoundError):
        analyze_image(tmp_path / "missing.jpg")
