"""Reading a `pps_compare.py log` file: qErr's sign from the sawtooths, `@@Ay`'s steps.

The counter measures TI = VP pulse - board pulse, so an engine's pulse late by its sawtooth
raises TI if it is the VP and lowers it if it is the board.  The VP, a real Oncore, defines what
its sawtooth means; the board matches it when their correlations with TI have opposite signs.
The 58503A's B4121P1115 (2026-11-08): r +0.98, its pulse late by its sawtooth.
"""

import csv
import math
import statistics
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

PULSE_PERIOD_S = 1.0
"""The counter's TI runs from a channel 1 pulse to the next channel 2 one: a channel 2 pulse just
before channel 1's reads nearly a period, so readings are unwrapped into (-half, +half]."""

MAX_LAG_S = 2
"""How many seconds either way of its expected pulse a `@@Bn` is tried against."""


@dataclass(frozen=True)
class Reading:
    """One counter reading."""

    host_time: float
    """Host UTC when the reading came back, s."""
    ti_s: float


@dataclass(frozen=True)
class Sawtooth:
    """One `@@Bn`'s sawtooth, for the pulse after the one it follows."""

    host_time: float
    """Host UTC when it arrived, s."""
    sawtooth_ns: float


@dataclass(frozen=True)
class Log:
    """A log file's contents."""

    readings: list[Reading]
    sawtooths: dict[str, list[Sawtooth]]
    """By source: `board`, `vp`."""
    events: list[tuple[float, str]]
    """Host time and text of each step the logger took."""


def unwrap(ti_s: float) -> float:
    """TI as channel 2 less channel 1, within half a period either way."""
    return ti_s - PULSE_PERIOD_S if ti_s > PULSE_PERIOD_S / 2 else ti_s


def read_log(path: Path) -> Log:
    """A `pps_compare.py log` CSV file."""
    readings: list[Reading] = []
    sawtooths: dict[str, list[Sawtooth]] = {}
    events: list[tuple[float, str]] = []
    with path.open(newline="") as f:
        for row in csv.DictReader(f):
            t = float(row["host_time"])
            source = row["source"]
            if source == "counter":
                if row["ti_s"]:
                    readings.append(Reading(t, unwrap(float(row["ti_s"]))))
            elif source == "event":
                events.append((t, row["text"]))
            else:
                sawtooths.setdefault(source, []).append(Sawtooth(t, float(row["sawtooth_ns"])))
    return Log(readings, sawtooths, events)


def pulse_second(reading: Reading) -> int:
    """The host second of the board's pulse that started a reading."""
    return math.floor(reading.host_time - reading.ti_s)


def described_second(sawtooth: Sawtooth) -> int:
    """The host second of the pulse a `@@Bn` describes: the one after its arrival."""
    return math.floor(sawtooth.host_time) + 1


def correlation(xs: list[float], ys: list[float]) -> float:
    """Pearson's r, 0 if either series is constant."""
    if len(xs) < 3 or statistics.pstdev(xs) == 0 or statistics.pstdev(ys) == 0:
        return 0.0
    return statistics.correlation(xs, ys)


@dataclass(frozen=True)
class Alignment:
    """A source's sawtooths matched to the readings."""

    lag_s: int
    """Added to a `@@Bn`'s described second to match it to a reading."""
    r: float
    """Correlation of TI with the sawtooth."""
    pairs: list[tuple[int, float]]
    """Index of the reading and its sawtooth, ns."""


def align(readings: list[Reading], sawtooths: Iterable[Sawtooth]) -> Alignment:
    """The lag that best correlates TI with a source's sawtooth.

    Matching is by host time, since the host's latency in reading a port is not known to the
    second; each lag within MAX_LAG_S is tried.
    """
    by_second = {described_second(s): s.sawtooth_ns for s in sawtooths}
    best = Alignment(0, 0.0, [])
    seconds = [pulse_second(r) for r in readings]
    for lag in range(-MAX_LAG_S, MAX_LAG_S + 1):
        pairs = [(i, by_second[s - lag]) for i, s in enumerate(seconds) if s - lag in by_second]
        r = correlation([readings[i].ti_s for i, _ in pairs], [v for _, v in pairs])
        if abs(r) > abs(best.r):
            best = Alignment(lag, r, pairs)
    return best


def sawtooth_report(log: Log) -> list[str]:
    """Each source's correlation and the sign it implies, and TI's spread before and after."""
    lines = []
    ti_ns = [r.ti_s * 1e9 for r in log.readings]
    if len(ti_ns) < 3:
        return ["fewer than 3 readings"]
    lines.append(f"readings {len(ti_ns)}, TI spread {statistics.pstdev(ti_ns):.2f} ns")
    corrected = list(ti_ns)
    alignments = {source: align(log.readings, s) for source, s in sorted(log.sawtooths.items())}
    # A sawtooth that never varies (a receiver that corrects its own pulse reports 0) matches
    # nothing and gives no sign.
    signs = {source: 1 if a.r > 0 else -1 for source, a in alignments.items() if a.pairs}
    for source, alignment in alignments.items():
        if not alignment.pairs:
            lines.append(f"{source}: no sawtooth matched a reading")
            continue
        if source == "vp":
            verdict = f"its pulse {'late' if signs[source] > 0 else 'early'} by its sawtooth"
        elif "vp" in signs:
            verdict = "as the VP" if signs[source] == -signs["vp"] else "opposite of the VP"
        else:
            verdict = "no VP to compare with"
        lines.append(
            f"{source}: r {alignment.r:+.3f} at lag {alignment.lag_s:+d} s, "
            f"{len(alignment.pairs)} matched: {verdict}"
        )
        for i, value in alignment.pairs:
            corrected[i] -= signs[source] * value
    lines.append(f"TI spread with both sawtooths removed {statistics.pstdev(corrected):.2f} ns")
    return lines


def step_report(log: Log) -> list[str]:
    """The median TI between each pair of logged steps."""
    marks = [(log.readings[0].host_time if log.readings else 0.0, "start"), *log.events]
    lines = []
    for (start, text), (end, _) in zip(marks, [*marks[1:], (math.inf, "")], strict=True):
        segment = [r.ti_s * 1e9 for r in log.readings if start <= r.host_time < end]
        # Skip the first readings after a step, while the pulse moves.
        settled = segment[2:]
        if settled:
            lines.append(
                f"after {text}: median TI {statistics.median(settled):.2f} ns, "
                f"{len(settled)} readings"
            )
    return lines
