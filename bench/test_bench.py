"""Tests for the 1 PPS comparison's framing and analysis."""

import random
from pathlib import Path

import pytest

import analysis
import oncore


def bn(next_pulse: tuple[int, int, int], sawtooth_ns: int, solution: int = 0) -> bytes:
    """A `@@Bn` answer with the given fields, the rest zeros."""
    data = bytearray(52)
    data[12:15] = bytes(next_pulse)
    data[17] = solution
    data[21] = sawtooth_ns & 0xFF
    return oncore.frame(b"Bn" + bytes(data))


def test_parses_a_time_raim_answer() -> None:
    parsed = oncore.parse_time_raim(bn((1, 2, 3), -52, 1))
    assert parsed == oncore.TimeRaim(next_pulse=(1, 2, 3), solution=1, sawtooth_ns=-52)


def test_rejects_a_bad_checksum() -> None:
    message = bytearray(bn((1, 2, 3), 5))
    message[-3] ^= 1
    assert oncore.parse_time_raim(bytes(message)) is None


def test_deframes_across_reads_and_other_messages() -> None:
    stream = oncore.frame(b"Ba" + bytes(62)) + bn((0, 0, 1), 7) + b"@@" + bn((0, 0, 2), -7)
    deframer = oncore.Deframer()
    found = [
        *deframer.feed(stream[:40]),
        *deframer.feed(stream[40:100]),
        *deframer.feed(stream[100:]),
    ]
    assert [f.sawtooth_ns for f in found] == [7, -7]


def test_commands() -> None:
    assert oncore.pulse_offset_command(200) == oncore.frame(b"Ay\x00\x00\x00\xc8")
    with pytest.raises(ValueError):
        oncore.pulse_offset_command(1_000_000_000)
    assert oncore.time_raim_command(1, 1, 10) == oncore.frame(b"Bn\x01\x01\x00\x0a\x01" + bytes(10))


def synthetic_log(path: Path, vp_sign: int, board_sign: int, lag_s: int) -> None:
    """A log whose TI follows each engine's sawtooth with the given signs."""
    rng = random.Random(1)
    lines = ["host_time,source,ti_s,next_pulse,solution,sawtooth_ns,text"]
    for k in range(300):
        second = 1_000_000 + k
        vp, board = rng.randint(-50, 50), rng.randint(-10, 10)
        ti_ns = 100 + vp_sign * vp + board_sign * board + rng.gauss(0, 0.5)
        arrival = second - 0.5 - lag_s
        lines.append(f"{arrival},vp,,00:00:00,0,{vp},")
        lines.append(f"{arrival},board,,00:00:00,0,{board},")
        lines.append(f"{second + 0.2},counter,{ti_ns * 1e-9!r},,,,")
    path.write_text("\n".join(lines) + "\n")


def test_a_board_whose_sawtooth_means_what_the_vps_does(tmp_path: Path) -> None:
    path = tmp_path / "log.csv"
    # Each pulse late by its sawtooth: TI rises with the VP's, falls with the board's.
    synthetic_log(path, vp_sign=1, board_sign=-1, lag_s=1)
    report = analysis.sawtooth_report(analysis.read_log(path))
    assert any(
        line.startswith("board:") and "lag +1 s" in line and "as the VP" in line for line in report
    )
    assert any(line.startswith("vp:") and "late by its sawtooth" in line for line in report)
    spread = float(report[-1].split()[-2])
    assert spread < 1.0


def test_a_board_whose_sawtooth_is_the_vps_negated(tmp_path: Path) -> None:
    path = tmp_path / "log.csv"
    synthetic_log(path, vp_sign=1, board_sign=1, lag_s=0)
    report = analysis.sawtooth_report(analysis.read_log(path))
    assert any(line.startswith("board:") and "opposite of the VP" in line for line in report)


def test_step_report_medians(tmp_path: Path) -> None:
    path = tmp_path / "log.csv"
    rows = ["host_time,source,ti_s,next_pulse,solution,sawtooth_ns,text"]
    rows += [f"{t},counter,{100e-9!r},,,," for t in range(10)]
    rows.append("10.5,event,,,,,vp Ay 200")
    rows += [f"{t},counter,{300e-9!r},,,," for t in range(11, 21)]
    path.write_text("\n".join(rows) + "\n")
    report = analysis.step_report(analysis.read_log(path))
    assert report == [
        "after start: median TI 100.00 ns, 8 readings",
        "after vp Ay 200: median TI 300.00 ns, 8 readings",
    ]


SOLUTION_UNKNOWN = 2
SAWTOOTH_NS = -5
NEXT_PULSE = (14, 28, 10)
FRAMING = len(b"@@") + len(b"C\r\n")


def time_raim_message(
    form: oncore.TimeRaimFormat, next_pulse: tuple[int, int, int] | None
) -> bytes:
    """A message of `form` with solution unknown, sawtooth SAWTOOTH_NS and `next_pulse`."""
    body = bytearray(form.length - FRAMING)
    body[0:2] = form.id
    body[form.solution - 2] = SOLUTION_UNKNOWN
    body[form.sawtooth - 2] = SAWTOOTH_NS & 0xFF
    if form.hour is not None and next_pulse is not None:
        body[form.hour - 2 : form.hour + 1] = bytes(next_pulse)
    return oncore.frame(bytes(body))


def test_an_m12_hn_is_read() -> None:
    """A @@Hn at the M12+ guide's offsets."""
    found = oncore.Deframer(oncore.HN).feed(time_raim_message(oncore.HN, None))
    assert [(t.next_pulse, t.solution, t.sawtooth_ns) for t in found] == [
        (None, SOLUTION_UNKNOWN, SAWTOOTH_NS)
    ]


def test_an_eight_channel_vp_en_is_read() -> None:
    """A @@En at the VP reference's offsets (p. 140)."""
    found = oncore.Deframer(oncore.EN).feed(time_raim_message(oncore.EN, NEXT_PULSE))
    assert [(t.next_pulse, t.solution, t.sawtooth_ns) for t in found] == [
        (NEXT_PULSE, SOLUTION_UNKNOWN, SAWTOOTH_NS)
    ]


def test_a_reading_past_half_a_period_is_unwrapped() -> None:
    assert analysis.unwrap(0.999999917) == pytest.approx(-83e-9)
    assert analysis.unwrap(9.7e-9) == 9.7e-9


def test_no_vp_sign_from_a_constant_sawtooth(tmp_path: Path) -> None:
    path = tmp_path / "log.csv"
    synthetic_log(path, vp_sign=0, board_sign=-1, lag_s=0)
    lines = path.read_text().splitlines()
    constant = [
        ",".join(f if i != 5 else "0" for i, f in enumerate(line.split(",")))
        if ",vp," in line
        else line
        for line in lines
    ]
    path.write_text("\n".join(constant) + "\n")
    report = analysis.sawtooth_report(analysis.read_log(path))
    assert any(line.startswith("board:") and "no VP to compare with" in line for line in report)
