import argparse
from collections.abc import Sequence
from pathlib import Path

import pytest

import analysis
import pps_compare

STEP = (60.0, "board", 10)
BOARD_DEVICE = "board-device"
VP_DEVICE = "vp-device"
BOTH = "board,vp"
LOG_PATH = Path("run.csv")
SAWTOOTH = analysis.Sawtooth(0.0, 0.0)


def args(
    board: str | None = None,
    vp: str | None = None,
    time_raim: str = "",
    step: Sequence[tuple[float, str, int]] = (),
) -> argparse.Namespace:
    return argparse.Namespace(board=board, vp=vp, time_raim=time_raim, step=list(step))


def test_no_engines_named_none_missing() -> None:
    assert pps_compare.missing_engines(args()) == []


def test_time_raim_on_an_absent_engine_is_missing() -> None:
    assert pps_compare.missing_engines(args(vp=VP_DEVICE, time_raim=BOTH)) == ["board"]


def test_step_on_an_absent_engine_is_missing() -> None:
    assert pps_compare.missing_engines(args(step=[STEP])) == ["board"]


def test_present_engines_not_missing() -> None:
    named = args(board=BOARD_DEVICE, vp=VP_DEVICE, time_raim=BOTH, step=[STEP])
    assert pps_compare.missing_engines(named) == []


def sawtooth_log(*sources: str) -> analysis.Log:
    return analysis.Log(readings=[], sawtooths={s: [SAWTOOTH] for s in sources}, events=[])


def test_a_log_with_no_sawtooth_has_no_source() -> None:
    assert pps_compare.only_source(sawtooth_log(), LOG_PATH) is None


def test_a_log_s_one_sawtooth_is_its_source() -> None:
    assert pps_compare.only_source(sawtooth_log("vp"), LOG_PATH) == "vp"


def test_a_log_with_several_sawtooths_needs_one_named() -> None:
    with pytest.raises(ValueError, match="name one"):
        pps_compare.only_source(sawtooth_log("board", "vp"), LOG_PATH)
