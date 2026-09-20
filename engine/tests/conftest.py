from __future__ import annotations

import json
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[2]
FIXTURES = REPO / "fixtures"


@pytest.fixture(scope="session")
def fixtures_dir() -> Path:
    return FIXTURES


@pytest.fixture(scope="session")
def sample_export() -> dict:
    """The same conversation export the Rust importer's tests use."""
    return json.loads((FIXTURES / "import" / "sample_export.json").read_text(encoding="utf-8"))


@pytest.fixture()
def own_messages(sample_export) -> list[str]:
    """Message bodies from the export, as plain strings."""
    return [m["body"] for c in sample_export["conversations"] for m in c["messages"]]
