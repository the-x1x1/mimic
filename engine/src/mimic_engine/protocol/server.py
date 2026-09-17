"""NDJSON stdio server.

Request : {"protocolVersion":1,"requestId":"…","method":"…","params":{}}
Response: {"protocolVersion":1,"requestId":"…","ok":true,"result":{}}
Error   : {"protocolVersion":1,"requestId":"…","ok":false,"error":{"code","message","details"}}
Event   : {"event":"job.progress","jobId":"…","phase":"…","current":n,"total":n}

Rules: one request per line, bounded line length, no shell, no eval. Requests
are handled sequentially on one thread; long operations report progress
through `Progress` so the desktop can show real item counts.
"""

from __future__ import annotations

import json
import sys
import threading
import traceback
from collections.abc import Callable
from typing import Any, TextIO

from mimic_engine import PROTOCOL_VERSION

from .errors import EngineError, InvalidParamsError, UnknownMethodError

MAX_LINE_BYTES = 32 * 1024 * 1024
Handler = Callable[[dict[str, Any], "Progress"], Any]


class Progress:
    """Emits `job.progress` events for a request."""

    def __init__(self, emit: Callable[[dict[str, Any]], None], job_id: str | None):
        self._emit = emit
        self.job_id = job_id
        self.cancel_requested = False

    def __call__(self, phase: str, current: int, total: int, message: str | None = None) -> None:
        if self.job_id is None:
            return
        ev: dict[str, Any] = {
            "event": "job.progress",
            "jobId": self.job_id,
            "phase": phase,
            "current": int(current),
            "total": int(total),
        }
        if message:
            ev["message"] = message
        self._emit(ev)

    def log(self, level: str, message: str) -> None:
        self._emit({"event": "log", "level": level, "message": message})


class Server:
    def __init__(self, stdin: TextIO | None = None, stdout: TextIO | None = None):
        self._in = stdin or sys.stdin
        self._out = stdout or sys.stdout
        self._lock = threading.Lock()
        self._handlers: dict[str, Handler] = {}
        self._running = True

    def register(self, method: str, handler: Handler) -> None:
        self._handlers[method] = handler

    def methods(self) -> list[str]:
        return sorted(self._handlers)

    def stop(self) -> None:
        self._running = False

    def emit(self, obj: dict[str, Any]) -> None:
        from mimic_engine.utils.jsonutil import dumps

        line = dumps(obj)
        with self._lock:
            self._out.write(line + "\n")
            self._out.flush()

    def _reply(self, request_id: str, result: Any = None, error: dict[str, Any] | None = None) -> None:
        body: dict[str, Any] = {"protocolVersion": PROTOCOL_VERSION, "requestId": request_id, "ok": error is None}
        if error is None:
            body["result"] = result
        else:
            body["error"] = error
        self.emit(body)

    def handle_line(self, line: str) -> None:
        """Dispatch one request line. Public for tests."""
        if len(line) > MAX_LINE_BYTES:
            self._reply("", error=EngineError("request exceeds maximum size", code="message_too_large").to_wire())
            return
        try:
            req = json.loads(line)
        except json.JSONDecodeError as e:
            self._reply("", error=EngineError(f"malformed JSON: {e}", code="malformed_request").to_wire())
            return
        if not isinstance(req, dict):
            self._reply("", error=EngineError("request must be an object", code="malformed_request").to_wire())
            return
        request_id = str(req.get("requestId", ""))
        if req.get("protocolVersion") != PROTOCOL_VERSION:
            self._reply(
                request_id,
                error=EngineError(
                    f"unsupported protocolVersion {req.get('protocolVersion')!r}", code="protocol_mismatch"
                ).to_wire(),
            )
            return
        method = req.get("method")
        params = req.get("params")
        if params is None:
            params = {}
        if not isinstance(method, str) or method not in self._handlers:
            self._reply(request_id, error=UnknownMethodError(f"unknown method {method!r}").to_wire())
            return
        if not isinstance(params, dict):
            self._reply(request_id, error=InvalidParamsError("params must be an object").to_wire())
            return
        progress = Progress(self.emit, params.get("jobId"))
        try:
            result = self._handlers[method](params, progress)
            self._reply(request_id, result)
        except EngineError as e:
            self._reply(request_id, error=e.to_wire())
        except Exception as e:
            tb = traceback.format_exc(limit=8)
            self.emit({"event": "log", "level": "error", "message": f"{method}: {e}\n{tb}"})
            self._reply(request_id, error=EngineError(f"{type(e).__name__}: {e}", code="internal_error").to_wire())

    def serve_forever(self) -> None:
        for raw in self._in:
            if not self._running:
                break
            line = raw.strip()
            if not line:
                continue
            self.handle_line(line)
            if not self._running:
                break
