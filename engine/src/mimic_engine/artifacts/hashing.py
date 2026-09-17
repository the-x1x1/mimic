"""Content hashing and atomic writes for cache/model artifacts."""

from __future__ import annotations

import contextlib
import hashlib
import os
import tempfile
from pathlib import Path


def sha256_file(path: str | os.PathLike[str], chunk: int = 1 << 20) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            block = f.read(chunk)
            if not block:
                break
            h.update(block)
    return h.hexdigest()


def fast_hash(path: str | os.PathLike[str], head: int = 64 * 1024) -> str:
    """Cheap identity fingerprint: size + mtime + sha256 of first/last 64 KiB.

    Full hashes are computed lazily only when needed (spec §7.3).
    """
    p = Path(path)
    st = p.stat()
    h = hashlib.sha256()
    h.update(f"{st.st_size}:{int(st.st_mtime)}".encode())
    with open(p, "rb") as f:
        h.update(f.read(head))
        if st.st_size > 2 * head:
            f.seek(-head, os.SEEK_END)
            h.update(f.read(head))
    return "fh1:" + h.hexdigest()[:32]


def atomic_write_bytes(path: str | os.PathLike[str], data: bytes) -> None:
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=p.parent, prefix=p.name + ".", suffix=".tmp")
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, p)
    except BaseException:
        with contextlib.suppress(OSError):
            os.unlink(tmp)
        raise
