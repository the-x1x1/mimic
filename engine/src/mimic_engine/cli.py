"""`mimic-engine` command line."""

from __future__ import annotations

import argparse
import json
import sys

from mimic_engine import PROTOCOL_VERSION, __version__


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="mimic-engine", description="Mimic text/ML engine sidecar")
    parser.add_argument(
        "--version", action="version", version=f"mimic-engine {__version__} (protocol {PROTOCOL_VERSION})"
    )
    sub = parser.add_subparsers(dest="cmd")
    sub.add_parser("serve", help="serve the NDJSON protocol over stdio (used by the desktop app)")
    emb = sub.add_parser("embed", help="embed a string and print the vector")
    emb.add_argument("text")
    cmp_ = sub.add_parser("compare", help="compare a generated reply with a real one")
    cmp_.add_argument("generated")
    cmp_.add_argument("actual")
    sub.add_parser("methods", help="list protocol methods")
    args = parser.parse_args(argv)

    if args.cmd in (None, "serve"):
        from mimic_engine.protocol.service import main_serve

        return main_serve()
    if args.cmd == "embed":
        from mimic_engine.embeddings.encoder import EncoderManager

        enc = EncoderManager()
        json.dump({"status": enc.status(), "vector": [round(float(x), 6) for x in enc.embed(args.text)]}, sys.stdout)
        print()
        return 0
    if args.cmd == "compare":
        from mimic_engine.embeddings.encoder import EncoderManager
        from mimic_engine.evaluation.metrics import compare

        json.dump(compare(args.generated, args.actual, EncoderManager()), sys.stdout, indent=2)
        print()
        return 0
    if args.cmd == "methods":
        from mimic_engine.protocol.service import build_server

        print("\n".join(build_server().methods()))
        return 0
    parser.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
