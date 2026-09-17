"""`mimic-engine` command line."""

from __future__ import annotations

import argparse
import json
import sys

from mimic_engine import PROTOCOL_VERSION, __version__


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="mimic-engine", description="Mimic image/ML engine sidecar")
    parser.add_argument(
        "--version", action="version", version=f"mimic-engine {__version__} (protocol {PROTOCOL_VERSION})"
    )
    sub = parser.add_subparsers(dest="cmd")
    sub.add_parser("serve", help="serve the NDJSON protocol over stdio (used by the desktop app)")
    scan = sub.add_parser("scan", help="scan folders and print a JSON report")
    scan.add_argument("roots", nargs="+")
    scan.add_argument("--no-metadata", action="store_true")
    px = sub.add_parser("parse-xmp", help="parse an XMP sidecar and print raw crs settings")
    px.add_argument("path")
    an = sub.add_parser("analyze", help="analyze one image and print features")
    an.add_argument("path")
    sub.add_parser("methods", help="list protocol methods")
    args = parser.parse_args(argv)

    if args.cmd in (None, "serve"):
        from mimic_engine.protocol.service import main_serve

        return main_serve()
    if args.cmd == "scan":
        from mimic_engine.ingest.scanner import scan_folders

        report = scan_folders(args.roots, include_metadata=not args.no_metadata)
        json.dump(report, sys.stdout, indent=2, default=str)
        print()
        return 0
    if args.cmd == "parse-xmp":
        from mimic_engine.xmp.parser import parse_xmp_file

        json.dump(parse_xmp_file(args.path), sys.stdout, indent=2)
        print()
        return 0
    if args.cmd == "analyze":
        from mimic_engine.features.analyze import analyze_image
        from mimic_engine.utils.jsonutil import to_jsonable

        json.dump(to_jsonable(analyze_image(args.path)), sys.stdout, indent=2)
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
