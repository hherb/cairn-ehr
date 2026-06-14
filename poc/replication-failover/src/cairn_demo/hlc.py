"""Hybrid Logical Clock (HLC).

A Hybrid Logical Clock gives every event a timestamp that (a) tracks wall-clock
time closely enough to be human-meaningful, yet (b) yields a *deterministic
total order* across independent nodes even when their physical clocks disagree.
That total order is what lets two Cairn nodes that wrote independently while
partitioned converge on exactly the same ordering after they reconnect — with
no coordination and no "last write wins" data loss.

A timestamp is the pair ``(wall, counter)``:

* ``wall``    — milliseconds since the epoch (the physical component)
* ``counter`` — a logical tiebreak that increments when several events share the
                same ``wall`` value, so order is never ambiguous within a node.

Ordering between *different* nodes additionally breaks ties on the originating
node id, giving a strict total order over all events everywhere.

This module is intentionally pure (no I/O, no clock reads passed implicitly) so
it can be unit-tested deterministically — see ``tests/test_hlc.py``.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, order=False)
class HLC:
    """An immutable Hybrid Logical Clock timestamp."""

    wall: int
    counter: int

    def tick(self, now_ms: int) -> "HLC":
        """Advance this clock for a *locally originated* event.

        ``now_ms`` is the node's current physical clock reading. If physical
        time has moved forward we adopt it and reset the counter; otherwise we
        keep the (larger) logical time and bump the counter so the new event
        still sorts strictly after the previous one.
        """
        if now_ms > self.wall:
            return HLC(now_ms, 0)
        return HLC(self.wall, self.counter + 1)

    def merge(self, remote: "HLC", now_ms: int) -> "HLC":
        """Advance this clock after *receiving* a remote event.

        The standard HLC receive rule: the new wall time is the max of our
        clock, the remote clock, and physical now; the counter is chosen so the
        merged timestamp dominates whichever input(s) shared that max wall.
        """
        wall = max(self.wall, remote.wall, now_ms)
        if wall == self.wall and wall == remote.wall:
            counter = max(self.counter, remote.counter) + 1
        elif wall == self.wall:
            counter = self.counter + 1
        elif wall == remote.wall:
            counter = remote.counter + 1
        else:  # physical now is strictly ahead of both — fresh logical time
            counter = 0
        return HLC(wall, counter)

    # A strict total order: wall, then counter. (The node id is the final
    # tiebreak, applied by callers that compare across nodes — see sort_key.)
    def __lt__(self, other: "HLC") -> bool:
        return (self.wall, self.counter) < (other.wall, other.counter)


def sort_key(wall: int, counter: int, node_origin: str) -> tuple[int, int, str]:
    """The canonical total-order key for an event across all nodes."""
    return (wall, counter, node_origin)
