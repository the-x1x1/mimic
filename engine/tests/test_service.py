import io
import json

from mimic_engine import PROTOCOL_VERSION
from mimic_engine.protocol.service import build_server


def call(server, out, method, params, rid="r"):
    out.seek(0)
    out.truncate()
    server.handle_line(
        json.dumps({"protocolVersion": PROTOCOL_VERSION, "requestId": rid, "method": method, "params": params})
    )
    lines = [json.loads(line) for line in out.getvalue().splitlines() if line]
    resp = [line for line in lines if line.get("requestId") == rid][-1]
    events = [line for line in lines if "event" in line]
    return resp, events


def service(tmp_path):
    out = io.StringIO()
    server = build_server()
    server._out = out
    call(
        server,
        out,
        "engine.configure",
        {
            "dbPath": str(tmp_path / "m.db"),
            "embeddingsDir": str(tmp_path / "emb"),
            "encodersDir": str(tmp_path / "enc"),
        },
    )
    return server, out


def test_hello_and_health_report_which_encoder_is_in_use(tmp_path):
    server, out = service(tmp_path)
    resp, _ = call(server, out, "engine.hello", {"appVersion": "0.6.0"})
    assert resp["ok"]
    assert resp["result"]["protocolVersion"] == PROTOCOL_VERSION
    assert resp["result"]["appVersion"] == "0.6.0"

    resp, _ = call(server, out, "engine.health", {})
    encoder = resp["result"]["encoder"]
    assert encoder["provider"] == "lexical_v1"
    # The app shows this: claiming a semantic model where there is none is the
    # exact failure this field exists to prevent.
    assert encoder["semantic"] is False
    assert "lexical fallback" in encoder["reason"]


def test_the_encoder_is_looked_for_again_when_asked(tmp_path):
    server, out = service(tmp_path)
    resp, _ = call(server, out, "encoder.reload", {})
    assert resp["ok"]
    encoder = resp["result"]["encoder"]
    assert encoder["provider"] == "lexical_v1"
    assert encoder["semantic"] is False


def test_embedding_is_deterministic_and_normalized(tmp_path, own_messages):
    server, out = service(tmp_path)
    texts = own_messages[:5]
    first, _ = call(server, out, "text.embed", {"texts": texts})
    second, _ = call(server, out, "text.embed", {"texts": texts})
    assert first["result"]["vectors"] == second["result"]["vectors"]
    assert first["result"]["dims"] == len(first["result"]["vectors"][0])
    norm = sum(x * x for x in first["result"]["vectors"][0]) ** 0.5
    assert abs(norm - 1.0) < 1e-3


def test_empty_text_embeds_to_a_zero_vector_rather_than_failing(tmp_path):
    server, out = service(tmp_path)
    resp, _ = call(server, out, "text.embed", {"texts": ["", "   "]})
    assert resp["ok"]
    assert all(all(x == 0.0 for x in v) for v in resp["result"]["vectors"])


def test_similarity_ranks_the_closest_candidate_first(tmp_path):
    server, out = service(tmp_path)
    resp, _ = call(
        server,
        out,
        "text.similarity",
        {
            "query": "can you send the quarterly deck",
            "candidates": ["pub friday?", "could you send the quarterly deck over", "the growth line looks optimistic"],
            "k": 2,
        },
    )
    matches = resp["result"]["matches"]
    assert matches[0]["index"] == 1
    assert matches[0]["score"] > matches[1]["score"]
    assert len(matches) == 2


def test_eval_compare_reports_components_and_refuses_a_headline_score(tmp_path):
    server, out = service(tmp_path)
    resp, events = call(
        server,
        out,
        "eval.compare",
        {
            "pairs": [
                {"generated": "yeah sounds good", "actual": "yeah sounds good"},
                {"generated": "Yes, that would be acceptable to me.", "actual": "yeah fine"},
            ]
        },
    )
    cases = resp["result"]["cases"]
    assert cases[0]["length"] == 1.0
    assert cases[0]["vocabulary"] == 1.0
    assert cases[1]["vocabulary"] < 0.5
    summary = resp["result"]["summary"]
    assert summary["measurable"] is True
    assert summary["cases"] == 2
    assert "score" not in summary, "a single headline number has to be defined before it is shown"
    assert any(e.get("event") == "job.progress" for e in events) is False, "no jobId means no progress spam"


def test_eval_compare_with_no_pairs_says_it_is_unmeasurable(tmp_path):
    server, out = service(tmp_path)
    resp, _ = call(server, out, "eval.compare", {"pairs": []})
    assert resp["result"]["summary"] == {"cases": 0, "measurable": False}


def test_bad_params_are_rejected_by_name(tmp_path):
    server, out = service(tmp_path)
    resp, _ = call(server, out, "text.embed", {"texts": "not a list"})
    assert not resp["ok"]
    assert resp["error"]["code"] == "invalid_params"
    resp, _ = call(server, out, "eval.compare", {"pairs": [{"generated": "x"}]})
    assert not resp["ok"]

    resp, _ = call(server, out, "nope.method", {})
    assert resp["error"]["code"] == "unknown_method"


def test_split_never_puts_a_conversation_on_both_sides(tmp_path):
    server, out = service(tmp_path)
    keys = ["t1"] * 10 + ["t2"] * 10 + ["t3"] * 10 + ["t4"] * 10
    resp, _ = call(server, out, "eval.split", {"groupKeys": keys, "seed": 7})
    r = resp["result"]
    train_groups = {keys[i] for i in r["train"]}
    holdout_groups = {keys[i] for i in r["holdout"]}
    assert train_groups and holdout_groups
    assert not (train_groups & holdout_groups)
    assert r["strategy"] == "conversation_grouped"
    assert sorted(r["train"] + r["holdout"]) == list(range(len(keys)))
