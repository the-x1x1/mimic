"""Method registry: binds protocol methods to engine modules and holds runtime config.

The engine is deliberately small. Everything that can be done
deterministically in Rust is done there; what lives here is the numeric work
that benefits from numpy, and running the sentence encoder the app
downloaded. The engine never touches the network.
"""

from __future__ import annotations

import platform
from pathlib import Path
from typing import Any

from mimic_engine import PROTOCOL_VERSION, __version__

from .errors import InvalidParamsError
from .server import Progress, Server


class EngineService:
    def __init__(self) -> None:
        self.db_path: str | None = None
        self.embeddings_dir: Path | None = None
        self.encoders_dir: Path | None = None
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

        return {"onnx": has("onnxruntime"), "sklearn": has("sklearn"), "numpy": has("numpy"), "methods": []}

    def hello(self, params: dict[str, Any], _p: Progress) -> dict[str, Any]:
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
            "capabilities": self.capabilities(),
            "appVersion": params.get("appVersion"),
        }

    def configure(self, params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        self.db_path = params.get("dbPath")
        for attr, key in (
            ("embeddings_dir", "embeddingsDir"),
            ("encoders_dir", "encodersDir"),
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

    def encoder_reload(self, _params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        """Look again for a downloaded encoder — after the app downloaded one,
        or removed it — and say which is in use now."""
        self._encoder = None
        return {"encoder": self.encoder().status()}

    def health(self, _params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        return {
            "ok": True,
            "engineVersion": __version__,
            "configured": self.db_path is not None,
            "encoder": self.encoder().status(),
        }

    # ----- text ----------------------------------------------------------

    def text_embed(self, params: dict[str, Any], progress: Progress) -> dict[str, Any]:
        """Embed one or more texts. Returns plain lists; the caller stores them."""
        texts = params.get("texts")
        if not isinstance(texts, list) or not all(isinstance(t, str) for t in texts):
            raise InvalidParamsError("texts must be a list of strings")
        enc = self.encoder()
        vectors = []
        step = 64
        for start in range(0, len(texts), step):
            for row in enc.embed_batch(texts[start : start + step]):
                vectors.append([round(float(x), 6) for x in row])
            if len(texts) > step:
                progress("embedding", min(start + step, len(texts)), len(texts))
        status = enc.status()
        return {
            "provider": status["provider"],
            "dims": status["dims"],
            "semantic": status["semantic"],
            "vectors": vectors,
        }

    def text_similarity(self, params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        """Rank `candidates` against `query`. Used by retrieval when the Rust
        side wants embedding ranking rather than lexical ranking."""
        query = params.get("query")
        candidates = params.get("candidates")
        if not isinstance(query, str) or not isinstance(candidates, list):
            raise InvalidParamsError("query must be a string and candidates a list of strings")
        k = int(params.get("k", 10))
        from mimic_engine.retrieval import top_k

        enc = self.encoder()
        matrix = enc.embed_batch([c if isinstance(c, str) else "" for c in candidates])
        ranked = top_k(enc.embed(query), matrix, k)
        return {
            "provider": enc.status()["provider"],
            "matches": [{"index": i, "score": round(s, 6)} for i, s in ranked],
        }

    # ----- evaluation ----------------------------------------------------

    def eval_compare(self, params: dict[str, Any], progress: Progress) -> dict[str, Any]:
        """Compare generated replies with the ones actually sent.

        `pairs` is a list of `{generated, actual}`. Returns one comparison per
        pair plus an aggregate, and no headline score: what a "Mimic Score"
        would mean has to be defined before one is shown.
        """
        pairs = params.get("pairs")
        if not isinstance(pairs, list):
            raise InvalidParamsError("pairs must be a list of {generated, actual}")
        from mimic_engine.evaluation.metrics import compare, summarize

        enc = self.encoder()
        cases = []
        for i, pair in enumerate(pairs):
            if not isinstance(pair, dict) or "generated" not in pair or "actual" not in pair:
                raise InvalidParamsError("each pair needs generated and actual")
            cases.append(compare(str(pair["generated"]), str(pair["actual"]), enc))
            progress("comparing", i + 1, len(pairs))
        return {"cases": cases, "summary": summarize(cases)}

    def eval_split(self, params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        """Conversation-grouped split, so no thread straddles the boundary."""
        keys = params.get("groupKeys")
        if not isinstance(keys, list) or not all(isinstance(k, str) for k in keys):
            raise InvalidParamsError("groupKeys must be a list of strings")
        from mimic_engine.evaluation.split import grouped_split

        split = grouped_split(
            keys, seed=int(params.get("seed", 42)), holdout_fraction=float(params.get("holdoutFraction", 0.2))
        )
        return {
            "train": split.train,
            "holdout": split.holdout,
            "strategy": split.strategy,
            "groups": split.groups,
            "warnings": split.warnings,
        }

    # ----- registration --------------------------------------------------

    def shutdown(self, _params: dict[str, Any], _p: Progress) -> dict[str, Any]:
        return {"ok": True}

    def register(self, server: Server) -> None:
        server.register("engine.hello", self.hello)
        server.register("engine.configure", self.configure)
        server.register("engine.health", self.health)
        server.register("engine.shutdown", lambda p, pr: (server.stop(), self.shutdown(p, pr))[1])
        server.register("encoder.reload", self.encoder_reload)
        server.register("text.embed", self.text_embed)
        server.register("text.similarity", self.text_similarity)
        server.register("eval.compare", self.eval_compare)
        server.register("eval.split", self.eval_split)


def build_server() -> Server:
    """A server with every method registered. Used by `serve` and by tests."""
    server = Server()
    EngineService().register(server)
    return server


def main_serve() -> int:
    build_server().serve_forever()
    return 0
