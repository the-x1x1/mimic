"""Conversation-grouped train/holdout split.

Inherited, with the nouns changed, from the shoot-grouped split the previous
product used — and for the same reason. If two messages from the same
conversation land on opposite sides of the split, the evaluation is measuring
how well the system can recall a thread it has already seen, not how well it
can write like the user in a thread it has not. The number that comes out is
higher and means nothing.

Groups are assigned from a seeded hash of the group key, so a split is
reproducible for a given dataset and seed.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass


@dataclass
class Split:
    train: list[int]
    holdout: list[int]
    strategy: str
    groups: int
    warnings: list[str]


def grouped_split(group_keys: list[str], seed: int = 42, holdout_fraction: float = 0.2) -> Split:
    """Split indices so that no group appears on both sides."""
    n = len(group_keys)
    warnings: list[str] = []
    groups = sorted(set(group_keys))

    if n == 0:
        return Split(train=[], holdout=[], strategy="empty", groups=0, warnings=["nothing to split"])

    if len(groups) < 2:
        # One conversation: there is no honest held-out set, and saying so is
        # more useful than producing one.
        warnings.append(
            "every message comes from one conversation, so a held-out set would not be independent; "
            "any score from this split is optimistic"
        )
        cut = max(1, int(n * (1 - holdout_fraction)))
        return Split(
            train=list(range(cut)),
            holdout=list(range(cut, n)),
            strategy="time_ordered_single_group",
            groups=len(groups),
            warnings=warnings,
        )

    order = sorted(groups, key=lambda g: hashlib.sha256(f"{seed}:{g}".encode()).hexdigest())
    sizes = {g: sum(1 for k in group_keys if k == g) for g in groups}
    target = holdout_fraction * n
    held: set[str] = set()
    acc = 0
    for g in order:
        # The first group always goes to holdout, so it is never empty.
        if not held or acc < target:
            held.add(g)
            acc += sizes[g]
    if len(held) == len(groups):
        # Everything landed in holdout; give the largest group back to train.
        biggest = max(groups, key=lambda g: sizes[g])
        held.discard(biggest)
        warnings.append("the holdout fraction would have consumed every conversation; kept the largest for training")

    train = [i for i, k in enumerate(group_keys) if k not in held]
    holdout = [i for i, k in enumerate(group_keys) if k in held]
    if len(groups) == 2:
        warnings.append("only two conversations: one trains, one is held out, so the score rests on a single thread")
    return Split(train=train, holdout=holdout, strategy="conversation_grouped", groups=len(groups), warnings=warnings)
