import pytest

import tic_setup
from tic_setup import Edge

# The edges measured on 2026-11-08: the board's jig on CH1, the 58503A on CH2.
JIG = Edge(top=2.59, base=-0.02, rise_s=3.2e-9)
SMARTCLOCK = Edge(top=2.79, base=0.08, rise_s=20.8e-9)
MIDPOINT = 0.5


def test_the_midpoint_level() -> None:
    assert JIG.level(MIDPOINT) == pytest.approx(1.285)
    assert SMARTCLOCK.level(MIDPOINT) == pytest.approx(1.435)


def test_a_threshold_below_the_midpoint_is_crossed_early() -> None:
    assert JIG.lead_ns(1.0, MIDPOINT) == pytest.approx(0.437, abs=0.01)
    assert SMARTCLOCK.lead_ns(1.0, MIDPOINT) == pytest.approx(4.17, abs=0.01)
    assert JIG.lead_ns(JIG.level(MIDPOINT), MIDPOINT) == 0


def test_a_slow_channel_2_edge_makes_a_fixed_level_read_short() -> None:
    error = tic_setup.ti_error_ns({1: JIG, 2: SMARTCLOCK}, 1.0, MIDPOINT)
    assert error == pytest.approx(-3.73, abs=0.01)


def test_levels_are_one_per_channel() -> None:
    assert tic_setup.parse_levels("1.25,1.5") == {1: 1.25, 2: 1.5}
    with pytest.raises(Exception, match="one per channel"):
        tic_setup.parse_levels("1.25")
