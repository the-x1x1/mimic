"""Training orchestration: dataset → split → baselines → hybrid → metrics → artifacts.

Given the same database, library set, seed and config, training is
reproducible (deterministic split, closed-form ridge, KNN). Artifacts are
written under `<styles_dir>/<style_id>/<model_version_id>/` and hashed.
"""

from __future__ import annotations

import json
import platform
from collections.abc import Callable
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import joblib
import numpy as np

from mimic_engine import __version__
from mimic_engine.artifacts.hashing import atomic_write_bytes, sha256_file
from mimic_engine.confidence.score import ConfidenceCalibration
from mimic_engine.evaluation.metrics import acceptance_proxy, control_metrics
from mimic_engine.features.stats import FEATURE_VERSION
from mimic_engine.training.baselines import (
    ConditionedMedian,
    GlobalMedian,
    HybridModel,
    KnnRegressor,
    RidgeResidual,
    Standardizer,
)
from mimic_engine.training.dataset import MIN_PAIRS, Dataset, load_dataset
from mimic_engine.training.mapping import load_mapping
from mimic_engine.training.split import grouped_split

MODEL_TYPE = "hybrid_knn_residual"
MODEL_FORMAT = "joblib_v1"
DEFAULT_CONFIG: dict[str, Any] = {
    "seed": 42,
    "k": 8,
    "ridgeAlpha": 1.0,
    "embeddingWeight": 0.5,
    "split": [0.70, 0.15, 0.15],
    "minPairs": MIN_PAIRS,
}


class InsufficientDataError(Exception):
    def __init__(self, message: str, details: dict[str, Any]):
        super().__init__(message)
        self.details = details


def _meta(ds: Dataset, idx: list[int]) -> list[tuple[str, str]]:
    return [(ds.pairs[i].camera, ds.pairs[i].lens) for i in idx]


def train(
    db_path: str | Path,
    library_ids: list[str],
    style_id: str,
    model_version_id: str,
    styles_dir: str | Path,
    embeddings_dir: str | Path | None,
    config: dict[str, Any] | None = None,
    progress: Callable[[str, int, int], None] | None = None,
) -> dict[str, Any]:
    cfg = {**DEFAULT_CONFIG, **(config or {})}
    seed = int(cfg["seed"])
    np.random.seed(seed)
    report = lambda phase, cur, tot: progress(phase, cur, tot) if progress else None  # noqa: E731

    report("loading training pairs", 0, 0)
    ds = load_dataset(db_path, library_ids, embeddings_dir)
    if len(ds) < int(cfg["minPairs"]):
        raise InsufficientDataError(
            f"{len(ds)} usable pairs; at least {cfg['minPairs']} are needed",
            {"pairs": len(ds), "excluded": ds.excluded, "warnings": ds.warnings},
        )

    report("splitting by shoot", 0, 0)
    split = grouped_split([p.group_key for p in ds.pairs], seed=seed, fractions=tuple(cfg["split"]))
    x, y, m, e = ds.matrix()
    use_embedding = e is not None and float(cfg["embeddingWeight"]) > 0
    tr, va, ho = split.train, split.validation, split.holdout

    report("fitting baselines", 1, 4)
    scaler = Standardizer().fit(x[tr])
    gm = GlobalMedian().fit(x[tr], y[tr], m[tr])
    cm = ConditionedMedian().fit(x[tr], y[tr], m[tr], _meta(ds, tr))
    knn = KnnRegressor(k=int(cfg["k"]))
    hybrid = HybridModel(
        scaler=scaler,
        knn=knn,
        residual=RidgeResidual(alpha=float(cfg["ridgeAlpha"])),
        conditioned=cm,
        global_median=gm,
        use_embedding=use_embedding,
        embedding_weight=float(cfg["embeddingWeight"]),
    )
    z_tr = hybrid.features(x[tr], e[tr] if e is not None else None)
    knn.fit(z_tr, y[tr], m[tr])

    report("fitting residual model", 2, 4)
    # Leave-one-out KNN on train so the residual model does not learn from self-matches.
    loo = _loo_knn(knn, z_tr, y[tr], m[tr])
    hybrid.residual.fit(z_tr, y[tr] - loo, m[tr])

    report("evaluating", 3, 4)

    def evaluate(idx: list[int]) -> dict[str, Any]:
        if not idx:
            return {"n": 0}
        parts = hybrid.predict_parts(x[idx], e[idx] if e is not None else None, _meta(ds, idx))
        out = {name: control_metrics(y[idx], pred, m[idx], ds.control_names) for name, pred in parts.items()}
        out["acceptanceProxy"] = acceptance_proxy(y[idx], parts["hybrid"], m[idx], ds.control_names)
        out["n"] = len(idx)
        return out

    metrics = {"validation": evaluate(va), "holdout": evaluate(ho), "train": {"n": len(tr)}}
    # Calibration statistics for confidence: distances of genuinely unseen photos
    # (validation + holdout) to the training set; leave-one-out train distances
    # only when no evaluation photos exist.
    unseen = va + ho
    if unseen:
        _, un_dist = knn.neighbours(hybrid.features(x[unseen], e[unseen] if e is not None else None))
        tr_nn = un_dist.mean(axis=1)
    else:
        _, tr_dist = knn.neighbours(z_tr)
        tr_nn = tr_dist[:, 1:].mean(axis=1) if tr_dist.shape[1] > 1 else tr_dist[:, 0]
    fam_val = {
        fam: v["nMae"]
        for fam, v in (
            metrics["validation"].get("hybrid", {}).get("perFamily", {})
            or metrics["holdout"].get("hybrid", {}).get("perFamily", {})
        ).items()
    }
    isos = [float(ds.pairs[i].features[ds.feature_names.index("iso_log")]) for i in tr]
    iso_range = (100 * 2 ** min(isos), 100 * 2 ** max(isos)) if isos else (50.0, 51200.0)
    calib = ConfidenceCalibration(
        tr_nn, fam_val, {ds.pairs[i].camera for i in tr}, {ds.pairs[i].lens for i in tr}, iso_range
    )

    beats = _beats_baselines(metrics)

    report("writing artifacts", 4, 4)
    out_dir = Path(styles_dir) / style_id / model_version_id
    out_dir.mkdir(parents=True, exist_ok=True)
    model_path = out_dir / "model.joblib"
    joblib.dump(
        {
            "hybrid": hybrid,
            "controlNames": ds.control_names,
            "featureNames": ds.feature_names,
            "calibration": calib.to_dict(),
            "embeddingProvider": ds.embedding_provider,
            "trainAssetIds": [ds.pairs[i].asset_id for i in tr],
        },
        model_path,
        compress=3,
    )
    training_config = {
        "seed": seed,
        "k": cfg["k"],
        "ridgeAlpha": cfg["ridgeAlpha"],
        "embeddingWeight": cfg["embeddingWeight"] if use_embedding else 0,
        "embeddingProvider": ds.embedding_provider,
        "split": cfg["split"],
        "splitStrategy": split.strategy,
        "groups": split.groups,
        "featureVersion": FEATURE_VERSION,
        "featureNames": ds.feature_names,
        "targetControls": ds.control_names,
        "mappingVersion": load_mapping()["mappingVersion"],
        "editSchemaVersion": load_mapping()["editDnaSchemaVersion"],
        "trainingDataFingerprint": ds.fingerprint(),
        "dependencies": {
            "python": platform.python_version(),
            "numpy": np.__version__,
            "joblib": joblib.__version__,
            "mimic_engine": __version__,
        },
        "counts": {"pairs": len(ds), "train": len(tr), "validation": len(va), "holdout": len(ho)},
        "excluded": ds.excluded,
        "warnings": ds.warnings + split.warnings,
        "trainedAt": datetime.now(UTC).isoformat(timespec="seconds").replace("+00:00", "Z"),
    }
    atomic_write_bytes(out_dir / "training_config.json", json.dumps(training_config, indent=2).encode())
    atomic_write_bytes(out_dir / "metrics.json", json.dumps(metrics, indent=2).encode())
    artifacts = []
    for p in (model_path, out_dir / "training_config.json", out_dir / "metrics.json"):
        artifacts.append(
            {
                "kind": p.stem,
                "path": str(p),
                "sha256": sha256_file(p),
                "sizeBytes": p.stat().st_size,
                "format": MODEL_FORMAT if p.suffix == ".joblib" else "json",
            }
        )
    return {
        "modelType": MODEL_TYPE,
        "featureSchemaVersion": FEATURE_VERSION,
        "editSchemaVersion": training_config["editSchemaVersion"],
        "trainingConfig": training_config,
        "metrics": metrics,
        "beatsBaselines": beats,
        "artifacts": artifacts,
        "artifactDir": str(out_dir),
        "counts": training_config["counts"],
        "split": {"strategy": split.strategy, "groups": split.groups, "warnings": split.warnings},
        "trainingSet": {
            "fingerprint": training_config["trainingDataFingerprint"],
            "assetCount": len(ds) + sum(ds.excluded.values()),
            "validPairCount": len(ds),
        },
    }


def _loo_knn(knn: KnnRegressor, z: np.ndarray, y: np.ndarray, m: np.ndarray) -> np.ndarray:
    """Leave-one-out KNN prediction on the training set."""
    k = min(knn.k, z.shape[0] - 1)
    if k <= 0:
        return np.tile(np.nan_to_num(np.nanmedian(np.where(m, y, np.nan), axis=0), nan=0.5), (z.shape[0], 1))
    d = np.sqrt(np.maximum(((z[:, None, :] - z[None, :, :]) ** 2).sum(axis=2), 0.0))
    np.fill_diagonal(d, np.inf)
    idx = np.argsort(d, axis=1)[:, :k]
    dist = np.take_along_axis(d, idx, axis=1)
    w = 1.0 / (dist + 1e-3)
    out = np.empty_like(y)
    from mimic_engine.training.baselines import masked_median

    fallback = masked_median(y, m)
    for i in range(z.shape[0]):
        wi = w[i][:, None] * m[idx[i]]
        denom = wi.sum(axis=0)
        out[i] = np.where(denom > 0, (wi * y[idx[i]]).sum(axis=0) / np.maximum(denom, 1e-9), fallback)
    return out


def _beats_baselines(metrics: dict[str, Any]) -> dict[str, Any]:
    ref = metrics["holdout"] if metrics["holdout"].get("n") else metrics["validation"]
    if not ref.get("n"):
        return {"evaluated": False}
    hyb = ref["hybrid"]["overall"]["nMae"]
    out: dict[str, Any] = {
        "evaluated": True,
        "set": "holdout" if metrics["holdout"].get("n") else "validation",
        "hybridNmae": hyb,
    }
    for name in ("global_median", "conditioned_median", "knn"):
        base = ref[name]["overall"]["nMae"]
        out[name] = {"nMae": base, "beaten": bool(hyb is not None and base is not None and hyb <= base)}
    out["beatsGlobalMedian"] = out["global_median"]["beaten"]
    out["beatsKnn"] = out["knn"]["beaten"]
    return out
