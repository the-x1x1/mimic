#!/usr/bin/env python3
"""Minimal stand-in for mimic_engine used by mimic-core protocol tests.

Speaks the NDJSON protocol exactly: hello, echo, slow, progress, fail, huge,
crash, shutdown. Kept dependency-free so CI never needs the real engine to
prove the transport.
"""
import json
import sys
import time

PROTOCOL = 1


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def reply(req, result=None, error=None):
    body = {"protocolVersion": PROTOCOL, "requestId": req["requestId"], "ok": error is None}
    if error is None:
        body["result"] = result
    else:
        body["error"] = error
    send(body)


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    method = req.get("method")
    params = req.get("params") or {}
    if method == "engine.hello":
        reply(req, {"engineVersion": "fake-0.0.0", "protocolVersion": PROTOCOL, "pythonVersion": sys.version.split()[0],
                    "accelerator": "cpu", "capabilities": {"rawDecode": False, "onnx": False}})
    elif method == "engine.shutdown":
        reply(req, {})
        break
    elif method == "echo":
        reply(req, params)
    elif method == "slow":
        time.sleep(params.get("seconds", 1))
        reply(req, {"slept": params.get("seconds", 1)})
    elif method == "progress":
        n = params.get("n", 3)
        for i in range(n):
            send({"event": "job.progress", "jobId": params.get("jobId", "j"), "phase": "working", "current": i + 1, "total": n})
        reply(req, {"done": n})
    elif method == "fail":
        reply(req, error={"code": "boom", "message": "requested failure", "details": {"x": 1}})
    elif method == "huge":
        reply(req, {"blob": "x" * params.get("bytes", 1000)})
    elif method == "garbage":
        sys.stdout.write("this is not json\n")
        sys.stdout.flush()
        reply(req, {"ok": True})
    elif method == "crash":
        sys.stderr.write("fake engine crashing on purpose\n")
        sys.stderr.flush()
        sys.exit(3)
    else:
        reply(req, error={"code": "unknown_method", "message": f"unknown method {method}"})
