"""Deterministic scene grouping for a session (spec §22).

Pipeline: order by capture time → split into time blocks at large gaps →
inside each block cluster standardized visual features (luminance/colour
statistics + optional embedding) with seeded k-means → merge tiny clusters
into their nearest sibling. Bursts are runs of frames closer than
`burst_gap_s`. Output is stable for the same input and seed.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from typing import Any

import numpy as np

GROUPING_VERSION = "grouping_v1"


@dataclass
class SessionItem:
    asset_id: str
    captured_at: str | None
    features: np.ndarray  # compact visual vector (see compact_vector)
    embedding: np.ndarray | None = None


def _ts(s: str | None) -> float | None:
    if not s:
        return None
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def compact_vector(stats: dict[str, Any]) -> np.ndarray:
    lum = stats["luminance"]
    col = stats["color"]
    hist = np.asarray(stats["histogram"]["luminance"], dtype=np.float32)
    coarse = hist.reshape(8, -1).sum(axis=1) if hist.size % 8 == 0 else hist[:8]
    return np.asarray(
        [
            lum["mean"],
            lum["std"],
            lum["percentiles"]["p5"],
            lum["percentiles"]["p95"],
            col["castRedGreen"] * 4,
            col["castBlueYellow"] * 4,
            col["saturationMean"],
            col["skyLikeFraction"],
            *coarse,
        ],
        dtype=np.float32,
    )


def _kmeans(x: np.ndarray, k: int, seed: int, iters: int = 30) -> np.ndarray:
    rng = np.random.default_rng(seed)
    n = x.shape[0]
    k = max(1, min(k, n))
    # k-means++ style seeding, deterministic through rng.
    centers = [x[int(rng.integers(n))]]
    for _ in range(1, k):
        d = np.min([((x - c) ** 2).sum(axis=1) for c in centers], axis=0)
        probs = d / d.sum() if d.sum() > 0 else np.full(n, 1.0 / n)
        centers.append(x[int(rng.choice(n, p=probs))])
    c = np.stack(centers)
    labels = np.zeros(n, dtype=int)
    for _ in range(iters):
        dist = ((x[:, None, :] - c[None, :, :]) ** 2).sum(axis=2)
        new = dist.argmin(axis=1)
        if np.array_equal(new, labels) and _ > 0:
            break
        labels = new
        for j in range(k):
            if (labels == j).any():
                c[j] = x[labels == j].mean(axis=0)
    return labels


def group_session(
    items: list[SessionItem],
    *,
    seed: int = 42,
    time_gap_s: float = 20 * 60,
    burst_gap_s: float = 3.0,
    min_cluster: int = 4,
    target_cluster_size: int = 25,
) -> dict[str, Any]:
    if not items:
        return {"version": GROUPING_VERSION, "clusters": [], "assignments": {}, "bursts": {}, "timeBlocks": 0}
    order = sorted(
        range(len(items)),
        key=lambda i: (_ts(items[i].captured_at) is None, _ts(items[i].captured_at) or 0.0, items[i].asset_id),
    )
    # Time blocks.
    blocks: list[list[int]] = []
    untimed: list[int] = []
    prev_t = None
    for i in order:
        t = _ts(items[i].captured_at)
        if t is None:
            untimed.append(i)  # sorted last; all untimed frames share one block
        elif not blocks or prev_t is None or (t - prev_t) > time_gap_s:
            blocks.append([i])
        else:
            blocks[-1].append(i)
        if t is not None:
            prev_t = t
    if untimed:
        blocks.append(untimed)
    # Bursts.
    bursts: dict[str, str] = {}
    burst_no = 0
    prev_t = None
    prev_burst: str | None = None
    for pos, i in enumerate(order):
        t = _ts(items[i].captured_at)
        if t is not None and prev_t is not None and (t - prev_t) <= burst_gap_s:
            if prev_burst is None:
                burst_no += 1
                prev_burst = f"burst-{burst_no}"
                bursts[items[order[pos - 1]].asset_id] = prev_burst
            bursts[items[i].asset_id] = prev_burst
        else:
            prev_burst = None
        prev_t = t
    # Feature matrix: standardized compact stats (+ embedding block).
    feats = np.stack([it.features for it in items]).astype(np.float32)
    mean, std = feats.mean(axis=0), feats.std(axis=0) + 1e-6
    z = (feats - mean) / std
    if all(it.embedding is not None for it in items):
        emb = np.stack([it.embedding for it in items]).astype(np.float32)  # type: ignore[arg-type]
        z = np.hstack([z, emb * np.sqrt(z.shape[1]) * 0.5])
    assignments: dict[str, str] = {}
    clusters: list[dict[str, Any]] = []
    cluster_no = 0
    for b_idx, block in enumerate(blocks):
        n = len(block)
        k = max(1, round(float(np.sqrt(n / max(1, target_cluster_size / 4)))))
        k = min(k, max(1, n // min_cluster))
        labels = _kmeans(z[block], k, seed + b_idx) if k > 1 else np.zeros(n, dtype=int)
        # Merge tiny clusters into the nearest larger one.
        for _ in range(k):
            sizes = {j: int((labels == j).sum()) for j in set(labels.tolist())}
            small = [j for j, s in sizes.items() if s < min_cluster and len(sizes) > 1]
            if not small:
                break
            j = small[0]
            others = [o for o in sizes if o != j]
            cj = z[block][labels == j].mean(axis=0)
            nearest = min(others, key=lambda o: float(((z[block][labels == o].mean(axis=0) - cj) ** 2).sum()))
            labels[labels == j] = nearest
        for j in sorted(set(labels.tolist()), key=lambda j: min(block[i] for i in range(n) if labels[i] == j)):
            cluster_no += 1
            cid = f"group-{cluster_no}"
            members = [block[i] for i in range(n) if labels[i] == j]
            for m in members:
                assignments[items[m].asset_id] = cid
            fm = feats[members]
            times = [t for t in (_ts(items[m].captured_at) for m in members) if t is not None]
            clusters.append(
                {
                    "id": cid,
                    "label": f"Group {cluster_no}",
                    "timeBlock": b_idx + 1,
                    "count": len(members),
                    "summary": {
                        "medianLuminance": float(np.median(fm[:, 0])),
                        "medianCastRedGreen": float(np.median(fm[:, 4]) / 4),
                        "medianCastBlueYellow": float(np.median(fm[:, 5]) / 4),
                        "medianSaturation": float(np.median(fm[:, 6])),
                        "start": min(times) if times else None,
                        "end": max(times) if times else None,
                    },
                }
            )
    return {
        "version": GROUPING_VERSION,
        "clusters": clusters,
        "assignments": assignments,
        "bursts": bursts,
        "timeBlocks": len(blocks),
    }
