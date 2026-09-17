"""Method registry: binds protocol methods to engine modules and holds runtime config."""

from __future__ import annotations

import contextlib
import platform
import sys
from pathlib import Path
from typing import Any

from mimic_engine import PROTOCOL_VERSION, __version__

from .errors import EngineError, InvalidParamsError, NotFoundError
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
