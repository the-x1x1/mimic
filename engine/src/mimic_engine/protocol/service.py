"""Method registry: binds protocol methods to engine modules and holds runtime config."""

from __future__ import annotations

import contextlib
import platform
import sys
from pathlib import Path
from typing import Any

from mimic_engine import PROTOCOL_VERSION, __version__

from .errors import EngineError, InvalidParamsError, NotConfiguredError, NotFoundError
from .server import Progress, Server


class EngineService:
    def __init__(self) -> None:
        self.db_path: str | None = None
        self.previews_dir: Path | None = None
        self.embeddings_dir: Path | None = None
        self.encoders_dir: Path | None = None
        self.styles_dir: Path | None = None
        self.manifests_dir: Path | None = None
        self._encoder = None

    # ----- lifecycle -----------------------------------------------------

    def capabilities(self) -> dict[str, Any]:
        def has(mod: str) -> bool:
            try:
                __import__(mod)
                return True
            except Exception:
                return False

        return {
            "rawDecode": has("rawpy"),
            "onnx": has("onnxruntime"),
            "sklearn": has("sklearn"),
            "methods": [],
        }

    def hello(self, params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        caps = self.capabilities()
        accel = "cpu"
        try:
            import onnxruntime as ort

            if "DmlExecutionProvider" in ort.get_available_providers():
                accel = "directml"
        except Exception:
            pass
        return {
            "engineVersion": __version__,
            "protocolVersion": PROTOCOL_VERSION,
            "pythonVersion": platform.python_version(),
            "platform": platform.platform(),
            "accelerator": accel,
            "capabilities": caps,
            "appVersion": params.get("appVersion"),
        }

    def configure(self, params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        self.db_path = params.get("dbPath")
        for attr, key in (
            ("previews_dir", "previewsDir"),
            ("embeddings_dir", "embeddingsDir"),
            ("encoders_dir", "encodersDir"),
            ("styles_dir", "stylesDir"),
            ("manifests_dir", "manifestsDir"),
        ):
            v = params.get(key)
            setattr(self, attr, Path(v) if v else None)
            if v and attr != "manifests_dir":
                Path(v).mkdir(parents=True, exist_ok=True)
        self._encoder = None
        return {"ok": True, "encoder": self.encoder().status()}

    def encoder(self):
        if self._encoder is None:
            from mimic_engine.embeddings.encoder import EncoderManager

            self._encoder = EncoderManager(self.encoders_dir, self.manifests_dir)
        return self._encoder

    def health(self, _params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        return {
            "ok": True,
            "engineVersion": __version__,
            "configured": self.db_path is not None,
            "encoder": self.encoder().status(),
        }

    # ----- ingest --------------------------------------------------------

    def scan_folder(self, params: dict[str, Any], progress: Progress) -> dict[str, Any]:
        roots = params.get("roots")
        if not isinstance(roots, list) or not roots or not all(isinstance(r, str) for r in roots):
            raise InvalidParamsError("roots must be a non-empty list of paths")
        for r in roots:
            if not Path(r).exists():
                raise NotFoundError(f"path does not exist: {r}")
        from mimic_engine.ingest.scanner import scan_folders

        return scan_folders(roots, include_metadata=bool(params.get("includeMetadata", True)), progress=progress)

    def xmp_parse(self, params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        path = params.get("path")
        if not isinstance(path, str):
            raise InvalidParamsError("path required")
        from mimic_engine.xmp.parser import XmpParseError, parse_xmp_file

        try:
            return parse_xmp_file(path)
        except XmpParseError as e:
            raise EngineError(str(e), code="xmp_parse_error") from e

    def image_metadata(self, params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        path = params.get("path")
        if not isinstance(path, str):
            raise InvalidParamsError("path required")
        if not Path(path).is_file():
            raise NotFoundError(path)
        from mimic_engine.raw.metadata import read_metadata

        return read_metadata(path)

    def image_analyze(self, params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        path = params.get("path")
        if not isinstance(path, str):
            raise InvalidParamsError("path required")
        from mimic_engine.features.analyze import analyze_image
        from mimic_engine.raw.preview import PreviewError

        try:
            return analyze_image(
                path,
                asset_id=params.get("assetId"),
                fast_hash=params.get("fastHash"),
                previews_dir=self.previews_dir,
                embeddings_dir=self.embeddings_dir,
                encoder=self.encoder(),
            )
        except FileNotFoundError as e:
            raise NotFoundError(str(e)) from e
        except PreviewError as e:
            raise EngineError(str(e), code="decode_error") from e

    def image_analyze_batch(self, params: dict[str, Any], progress: Progress) -> dict[str, Any]:
        items = params.get("items")
        if not isinstance(items, list):
            raise InvalidParamsError("items must be a list")
        from mimic_engine.features.analyze import analyze_image
        from mimic_engine.raw.preview import PreviewError

        results = []
        total = len(items)
        for i, item in enumerate(items):
            asset_id = item.get("assetId")
            path = item.get("path")
            try:
                r = analyze_image(
                    path,
                    asset_id=asset_id,
                    fast_hash=item.get("fastHash"),
                    previews_dir=self.previews_dir,
                    embeddings_dir=self.embeddings_dir,
                    encoder=self.encoder(),
                )
                r.pop("vector", None)
                r.pop("vectorNames", None)
                results.append(r)
            except FileNotFoundError:
                results.append(
                    {
                        "assetId": asset_id,
                        "path": path,
                        "error": {"code": "not_found", "message": f"file not found: {path}"},
                    }
                )
            except PreviewError as e:
                results.append(
                    {"assetId": asset_id, "path": path, "error": {"code": "decode_error", "message": str(e)}}
                )
            except Exception as e:
                results.append(
                    {
                        "assetId": asset_id,
                        "path": path,
                        "error": {"code": "internal_error", "message": f"{type(e).__name__}: {e}"},
                    }
                )
            progress("analyzing", i + 1, total)
        return {"results": results, "featureVersion": "features_v1", "encoder": self.encoder().status()}

    # ----- training / inference (0.2.0) ---------------------------------

    def _require_db(self) -> str:
        if not self.db_path:
            raise NotConfiguredError("engine.configure with dbPath is required before training")
        return self.db_path

    def training_train(self, params: dict[str, Any], progress: Progress) -> dict[str, Any]:
        db = self._require_db()
        library_ids = params.get("libraryIds")
        style_id = params.get("styleId")
        mv_id = params.get("modelVersionId")
        if (
            not isinstance(library_ids, list)
            or not library_ids
            or not isinstance(style_id, str)
            or not isinstance(mv_id, str)
        ):
            raise InvalidParamsError("libraryIds (non-empty list), styleId and modelVersionId are required")
        styles_dir = params.get("stylesDir") or (str(self.styles_dir) if self.styles_dir else None)
        if not styles_dir:
            raise NotConfiguredError("stylesDir is not configured")
        from mimic_engine.training.trainer import InsufficientDataError, train

        try:
            return train(
                db,
                [str(x) for x in library_ids],
                style_id,
                mv_id,
                styles_dir,
                self.embeddings_dir,
                params.get("config") or {},
                progress,
            )
        except InsufficientDataError as e:
            raise EngineError(str(e), code="insufficient_data", details=e.details) from e

    def model_predict(self, params: dict[str, Any], progress: Progress) -> dict[str, Any]:
        db = self._require_db()
        model_path = params.get("modelPath")
        asset_ids = params.get("assetIds")
        if not isinstance(model_path, str) or not isinstance(asset_ids, list):
            raise InvalidParamsError("modelPath and assetIds are required")
        if not Path(model_path).is_file():
            raise NotFoundError(f"model artifact missing: {model_path}")
        import json

        import numpy as np

        from mimic_engine.features.stats import FEATURE_VERSION
        from mimic_engine.inference.predictor import Predictor
        from mimic_engine.training.dataset import open_readonly

        predictor = Predictor(model_path)
        results = []
        with open_readonly(db) as conn:
            for i, asset_id in enumerate(asset_ids):
                row = conn.execute(
                    """SELECT a.id, a.camera_make, a.camera_model, a.lens, a.iso, a.aperture, a.shutter_speed, a.focal_length,
                              f.histogram_json, f.luminance_json, f.color_json, f.clipping_json, f.sharpness, f.noise_estimate, f.embedding_artifact_id
                       FROM assets a LEFT JOIN visual_features f ON f.asset_id = a.id AND f.feature_version = ?
                       WHERE a.id = ?""",
                    (FEATURE_VERSION, asset_id),
                ).fetchone()
                if row is None:
                    results.append({"assetId": asset_id, "error": {"code": "not_found", "message": "asset not found"}})
                    continue
                if row["histogram_json"] is None:
                    results.append(
                        {
                            "assetId": asset_id,
                            "error": {
                                "code": "no_features",
                                "message": "asset has no visual features; analyze it first",
                            },
                        }
                    )
                    continue
                stats = {
                    "histogram": json.loads(row["histogram_json"]),
                    "luminance": json.loads(row["luminance_json"]),
                    "color": json.loads(row["color_json"]),
                    "clipping": json.loads(row["clipping_json"]),
                    "sharpness": row["sharpness"] or 0.0,
                    "noiseEstimate": row["noise_estimate"] or 0.0,
                }
                metadata = {
                    "iso": row["iso"],
                    "aperture": row["aperture"],
                    "shutterSpeed": row["shutter_speed"],
                    "focalLength": row["focal_length"],
                }
                embedding = None
                if predictor.hybrid.use_embedding and self.embeddings_dir and row["embedding_artifact_id"]:
                    ep = self.embeddings_dir / row["embedding_artifact_id"]
                    if ep.is_file():
                        embedding = np.load(ep, allow_pickle=False)
                camera = f"{row['camera_make'] or ''} {row['camera_model'] or ''}".strip() or "unknown"
                try:
                    out = predictor.predict_one(stats, metadata, embedding, camera, row["lens"] or "unknown")
                    out["assetId"] = asset_id
                    results.append(out)
                except ValueError as e:
                    results.append({"assetId": asset_id, "error": {"code": "predict_failed", "message": str(e)}})
                progress("predicting", i + 1, len(asset_ids))
        groups = params.get("groups")
        if isinstance(groups, dict):
            from mimic_engine.session.consistency import apply_consistency, detect_outliers
            from mimic_engine.training.mapping import control_by_canonical, raw_for

            ok_rows = [r for r in results if "error" not in r]
            if ok_rows:
                names = predictor.control_names
                mat = np.asarray(
                    [[r["global"][n.split(".", 1)[0]][n.split(".", 1)[1]]["value"] for n in names] for r in ok_rows],
                    dtype=np.float32,
                )
                row_groups = [groups.get(r["assetId"]) for r in ok_rows]
                # Outliers are judged on the raw predictions, before any blending.
                for r, flag in zip(ok_rows, detect_outliers(mat, names, row_groups), strict=True):
                    if flag is not None:
                        r["groupOutlier"] = {"control": flag[0], "distance": flag[1]}
                        r.setdefault("reasons", []).append(
                            f"{flag[0]} disagrees with its scene group by {flag[1] * 100:.0f}% of range"
                        )
                if params.get("consistency", True):
                    refs_in = params.get("references") or {}
                    index_by_asset = {r["assetId"]: i for i, r in enumerate(ok_rows)}
                    references = {
                        str(g): index_by_asset[a]
                        for g, a in refs_in.items()
                        if isinstance(a, str) and a in index_by_asset
                    }
                    adjusted, shift = apply_consistency(mat, names, row_groups, references)
                    for r, row, sh in zip(ok_rows, adjusted, shift, strict=True):
                        for j, n in enumerate(names):
                            fam, short = n.split(".", 1)
                            c = control_by_canonical(n)
                            if c is None:
                                continue
                            r["global"][fam][short]["value"] = round(float(row[j]), 5)
                            r["global"][fam][short]["raw"] = raw_for(c, float(row[j]))
                        r["consistencyShift"] = round(float(sh), 5)
                        if (
                            groups.get(r["assetId"]) in references
                            and index_by_asset[r["assetId"]] == references[groups[r["assetId"]]]
                        ):
                            r["isReference"] = True
        return {"results": results, "modelPath": model_path, "controlNames": predictor.control_names}

    def session_group(self, params: dict[str, Any], progress: Progress) -> dict[str, Any]:
        db = self._require_db()
        asset_ids = params.get("assetIds")
        if not isinstance(asset_ids, list) or not asset_ids:
            raise InvalidParamsError("assetIds required")
        import json

        import numpy as np

        from mimic_engine.features.stats import FEATURE_VERSION
        from mimic_engine.session.grouping import SessionItem, compact_vector, group_session
        from mimic_engine.training.dataset import open_readonly

        items: list[SessionItem] = []
        missing: list[str] = []
        with open_readonly(db) as conn:
            for aid in asset_ids:
                row = conn.execute(
                    """SELECT a.id, a.captured_at, f.histogram_json, f.luminance_json, f.color_json, f.embedding_artifact_id
                       FROM assets a LEFT JOIN visual_features f ON f.asset_id = a.id AND f.feature_version = ? WHERE a.id = ?""",
                    (FEATURE_VERSION, aid),
                ).fetchone()
                if row is None or row["histogram_json"] is None:
                    missing.append(aid)
                    continue
                stats = {
                    "histogram": json.loads(row["histogram_json"]),
                    "luminance": json.loads(row["luminance_json"]),
                    "color": json.loads(row["color_json"]),
                }
                emb = None
                if self.embeddings_dir and row["embedding_artifact_id"]:
                    ep = self.embeddings_dir / row["embedding_artifact_id"]
                    if ep.is_file():
                        emb = np.load(ep, allow_pickle=False)
                items.append(
                    SessionItem(
                        asset_id=aid, captured_at=row["captured_at"], features=compact_vector(stats), embedding=emb
                    )
                )
        progress("grouping", 0, len(items))
        cfg = params.get("config") or {}
        out = group_session(
            items,
            seed=int(cfg.get("seed", 42)),
            time_gap_s=float(cfg.get("timeGapSeconds", 1200)),
            burst_gap_s=float(cfg.get("burstGapSeconds", 3.0)),
            min_cluster=int(cfg.get("minClusterSize", 4)),
            target_cluster_size=int(cfg.get("targetClusterSize", 25)),
        )
        out["missingFeatures"] = missing
        progress("grouping", len(items), len(items))
        return out

    # ----- registration --------------------------------------------------

    def register(self, server: Server) -> None:
        server.register("engine.hello", self.hello)
        server.register("engine.configure", self.configure)
        server.register("engine.health", self.health)
        server.register("engine.shutdown", lambda _p, _pr: (server.stop(), {"ok": True})[1])
        server.register("scan.folder", self.scan_folder)
        server.register("xmp.parse", self.xmp_parse)
        server.register("image.metadata", self.image_metadata)
        server.register("image.analyze", self.image_analyze)
        server.register("image.analyze_batch", self.image_analyze_batch)
        server.register("training.train", self.training_train)
        server.register("model.predict", self.model_predict)
        server.register("session.group", self.session_group)


def build_server() -> Server:
    server = Server()
    EngineService().register(server)
    return server


def main_serve() -> int:
    # Windows: make sure stdout is UTF-8 and unbuffered lines.
    with contextlib.suppress(Exception):
        sys.stdout.reconfigure(encoding="utf-8", line_buffering=True)  # type: ignore[attr-defined]
    build_server().serve_forever()
    return 0
