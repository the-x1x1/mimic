"""Session-grouped train/validation/holdout split (spec §11.5).

Photos from the same shoot never straddle train and evaluation. Groups are
assigned deterministically from a seeded hash of the group key so the split
is reproducible for a given dataset and seed.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass


@dataclass
class Split:
    train: list[int]
    validation: list[int]
    holdout: list[int]
    strategy: str
    groups: int
    warnings: list[str]


def grouped_split(
    group_keys: list[str], seed: int = 42, fractions: tuple[float, float, float] = (0.70, 0.15, 0.15)
) -> Split:
    groups = sorted(set(group_keys))
    warnings: list[str] = []
    n = len(group_keys)
    if len(groups) >= 3:
        # Deterministic pseudo-random order of groups.
        order = sorted(groups, key=lambda g: hashlib.sha256(f"{seed}:{g}".encode()).hexdigest())
        sizes = {g: sum(1 for k in group_keys if k == g) for g in groups}
        train_target = fractions[0] * n
        val_target = fractions[1] * n
        buckets: dict[str, str] = {}
        acc_train = acc_val = 0
        # Guarantee at least one group per bucket, then fill by size.
        for i, g in enumerate(order):
            if i == 0:
                buckets[g] = "holdout"
            elif i == 1:
                buckets[g] = "validation"
                acc_val += sizes[g]
            elif acc_train < train_target:
                buckets[g] = "train"
                acc_train += sizes[g]
            elif acc_val < val_target:
                buckets[g] = "validation"
                acc_val += sizes[g]
            else:
                buckets[g] = "holdout"
        strategy = "session_grouped"
    elif len(groups) == 2:
        big, small = sorted(groups, key=lambda g: -sum(1 for k in group_keys if k == g))
        buckets = {big: "train", small: "holdout"}
        warnings.append("only two shoots: one is train, the other holdout; no separate validation set")
        strategy = "session_grouped_two_groups"
    else:
        # Single shoot: fall back to a time-ordered split and say it is optimistic.
        idx = list(range(n))
        cut1 = int(n * fractions[0])
        cut2 = int(n * (fractions[0] + fractions[1]))
        warnings.append(
            "all examples come from one shoot; time-ordered split within the shoot — metrics are optimistic"
        )
        return Split(
            train=idx[:cut1],
            validation=idx[cut1:cut2],
            holdout=idx[cut2:],
            strategy="time_ordered_single_group",
            groups=1,
            warnings=warnings,
        )
    train = [i for i, k in enumerate(group_keys) if buckets[k] == "train"]
    validation = [i for i, k in enumerate(group_keys) if buckets[k] == "validation"]
    holdout = [i for i, k in enumerate(group_keys) if buckets[k] == "holdout"]
    if not train:
        train, holdout = holdout, train
        warnings.append("split produced an empty train set; swapped with holdout")
    return Split(
        train=train, validation=validation, holdout=holdout, strategy=strategy, groups=len(groups), warnings=warnings
    )
