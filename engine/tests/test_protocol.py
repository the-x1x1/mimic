import io
import json

from mimic_engine import PROTOCOL_VERSION
from mimic_engine.protocol.errors import EngineError
from mimic_engine.protocol.server import Server
from mimic_engine.protocol.service import build_server


def run(server: Server, out: io.StringIO, obj) -> list[dict]:
    out.seek(0)
    out.truncate()
    server.handle_line(obj if isinstance(obj, str) else json.dumps(obj))
    return [json.loads(line) for line in out.getvalue().splitlines() if line]


def req(method, params=None, rid="r1"):
    return {"protocolVersion": PROTOCOL_VERSION, "requestId": rid, "method": method, "params": params or {}}


def test_hello_and_unknown_method():
    out = io.StringIO()
    server = build_server()
    server._out = out
    (resp,) = run(server, out, req("engine.hello", {"appVersion": "0.1.0"}))
    assert resp["ok"] and resp["requestId"] == "r1" and resp["protocolVersion"] == PROTOCOL_VERSION
    assert resp["result"]["protocolVersion"] == PROTOCOL_VERSION
    assert resp["result"]["capabilities"]["sklearn"] is True
    (resp,) = run(server, out, req("nope"))
    assert not resp["ok"] and resp["error"]["code"] == "unknown_method"


def test_malformed_and_protocol_mismatch():
    out = io.StringIO()
    server = build_server()
    server._out = out
    (resp,) = run(server, out, "{not json")
    assert resp["error"]["code"] == "malformed_request"
    (resp,) = run(server, out, {"protocolVersion": 99, "requestId": "x", "method": "engine.hello"})
    assert resp["error"]["code"] == "protocol_mismatch" and resp["requestId"] == "x"
    (resp,) = run(server, out, {"protocolVersion": 1, "requestId": "y", "method": "engine.hello", "params": []})
    assert resp["error"]["code"] == "invalid_params"


def test_handler_exception_becomes_structured_error_and_progress_events():
    out = io.StringIO()
    server = Server(stdout=out)

    def boom(params, progress):
        progress("phase", 1, 2)
        raise RuntimeError("kaboom")

    def custom(params, progress):
        raise EngineError("nope", code="custom_code", details={"k": 1})

    server.register("boom", boom)
    server.register("custom", custom)
    lines = run(server, out, req("boom", {"jobId": "j1"}))
    events = [line for line in lines if "event" in line]
    resp = [line for line in lines if "requestId" in line and "event" not in line][-1]
    assert any(e["event"] == "job.progress" and e["jobId"] == "j1" and e["total"] == 2 for e in events)
    assert resp["error"]["code"] == "internal_error" and "kaboom" in resp["error"]["message"]
    (resp,) = run(server, out, req("custom"))
    assert resp["error"] == {"code": "custom_code", "message": "nope", "details": {"k": 1}}


def test_shutdown_stops_loop():
    inp = io.StringIO(
        json.dumps(req("engine.hello"))
        + "\n"
        + json.dumps(req("engine.shutdown", rid="r2"))
        + "\n"
        + json.dumps(req("engine.hello", rid="r3"))
        + "\n"
    )
    out = io.StringIO()
    server = build_server()
    server._in = inp
    server._out = out
    server.serve_forever()
    ids = [json.loads(line)["requestId"] for line in out.getvalue().splitlines()]
    assert ids == ["r1", "r2"], ids
