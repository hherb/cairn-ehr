"""Unit tests for the Hybrid Logical Clock.

These pin down the ordering guarantees the sync engine relies on. They are
pure (no DB, no real clock) so they run anywhere and never flake.
"""

from cairn_demo.hlc import HLC, sort_key


def test_tick_adopts_physical_time_when_it_advances():
    clk = HLC(1000, 5)
    assert clk.tick(2000) == HLC(2000, 0)


def test_tick_bumps_counter_when_physical_time_stalls():
    clk = HLC(2000, 0)
    assert clk.tick(2000) == HLC(2000, 1)
    assert clk.tick(1500) == HLC(2000, 1)  # clock went backwards; logical wins


def test_consecutive_ticks_are_strictly_increasing():
    clk = HLC(0, 0)
    prev = clk
    for now in [10, 10, 10, 11, 11, 9]:  # includes stalls and a regression
        nxt = prev.tick(now)
        assert (prev.wall, prev.counter) < (nxt.wall, nxt.counter)
        prev = nxt


def test_merge_dominates_remote_with_equal_wall():
    local = HLC(2000, 1)
    remote = HLC(2000, 7)
    merged = local.merge(remote, now_ms=1500)
    assert merged == HLC(2000, 8)  # max counter + 1


def test_merge_adopts_fresh_physical_time():
    local = HLC(2000, 3)
    remote = HLC(1900, 9)
    merged = local.merge(remote, now_ms=3000)
    assert merged == HLC(3000, 0)


def test_merge_result_dominates_both_inputs():
    local = HLC(2000, 4)
    remote = HLC(2000, 2)
    merged = local.merge(remote, now_ms=2000)
    assert local < merged and remote < merged


def test_total_order_breaks_ties_on_node_id():
    # Same wall+counter, different originating node => still a strict order.
    a = sort_key(2000, 1, "A")
    b = sort_key(2000, 1, "B")
    assert a < b
    assert sorted([b, a]) == [a, b]
