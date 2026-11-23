"""Two receivers against one reference, each on its own counter: the per-second series, and
ADEV, MDEV, TDEV and a three-cornered hat from it.

Both counters start on the same reference edge and stop on a receiver's, so a receiver's
pulse late by L raises its TI by L, and receiver A - receiver B is the difference of the two
corrected TIs, the reference cancelling.  A module's pulse (TIM-TP) is late by -qErr; an
Oncore's (`@@Bn`, `@@En`, `@@Hn`) by its sawtooth (`analysis.py`); a source with none (a
SmartClock's pulse) is taken as it is.
"""

import bisect
import math
from dataclasses import dataclass

import analysis

NS = 1e-9
WINDOW_S = (0.0, 5e-6)
"""A reading outside is a false start (a start taken on the reference pulse's falling edge,
0.8 s for a 200 ms pulse) or no measurement, and its second is missing."""
SAWTOOTH_AGE_S = 1.5
"""A sawtooth older than this when a reading comes back describes another pulse."""
TAUS = (1, 2, 5, 10, 20, 50, 100, 200, 500, 1000)
"""Averaging times, in seconds of tau0 = 1 s."""
MODULE = "module"
"""The source name `pps_compare.py log --module` records TIM-TP under."""


def lateness_ns(source: str, sawtooth_ns: float) -> float:
    """How late a source's pulse is by its sawtooth."""
    return -sawtooth_ns if source == MODULE else sawtooth_ns


@dataclass(frozen=True)
class Second:
    """One receiver's second: its TI, its sawtooth and the corrected TI, ns."""

    ti_ns: float
    sawtooth_ns: float
    corrected_ns: float


def seconds(log: analysis.Log, source: str | None) -> tuple[dict[int, Second], int]:
    """A log's seconds by the host second of the pulse each reading measured (a reading slow
    to come back keeps its pulse's second), each reading with the last sawtooth before it from
    `source`, or with none and a zero sawtooth when `source` is None; and how many readings were
    outside the window."""
    saws = log.sawtooths.get(source, []) if source else []
    times = [s.host_time for s in saws]
    out: dict[int, Second] = {}
    outside = 0
    for reading in log.readings:
        if not WINDOW_S[0] < reading.ti_s < WINDOW_S[1]:
            outside += 1
            continue
        ti = reading.ti_s / NS
        if source is None:
            out[analysis.pulse_second(reading)] = Second(ti, 0.0, ti)
            continue
        i = bisect.bisect(times, reading.host_time) - 1
        if i < 0 or reading.host_time - times[i] >= SAWTOOTH_AGE_S:
            continue
        saw = float(saws[i].sawtooth_ns)
        out[analysis.pulse_second(reading)] = Second(ti, saw, ti - lateness_ns(source, saw))
    return out, outside


@dataclass(frozen=True)
class Series:
    """Two receivers' common seconds, contiguous from the first."""

    rows: list[dict[str, float]]
    missing: list[int]
    """Seconds from the start where either receiver has none."""


def series(a: dict[int, Second], b: dict[int, Second], a_name: str, b_name: str) -> Series:
    """The per-second rows: each receiver's TI, sawtooth and corrected TI, and A - B."""
    common = sorted(set(a) & set(b))
    if not common:
        return Series([], [])
    first, last = common[0], common[-1]
    rows, missing = [], []
    for k in range(first, last + 1):
        if k not in a or k not in b:
            missing.append(k - first)
            continue
        x, y = a[k], b[k]
        rows.append(
            {
                "s": k - first,
                f"{a_name}_ti": x.ti_ns,
                f"{a_name}_saw": x.sawtooth_ns,
                f"{a_name}_corr": x.corrected_ns,
                f"{b_name}_ti": y.ti_ns,
                f"{b_name}_saw": y.sawtooth_ns,
                f"{b_name}_corr": y.corrected_ns,
                "diff": x.corrected_ns - y.corrected_ns,
            }
        )
    return Series(rows, missing)


def avar(x: list[float], m: int) -> float:
    """Overlapping Allan variance of phase `x`, s, at tau = m s."""
    n = len(x) - 2 * m
    return sum((x[i + 2 * m] - 2 * x[i + m] + x[i]) ** 2 for i in range(n)) / (2 * m * m * n)


def mvar(x: list[float], m: int) -> float:
    """Modified Allan variance of phase `x`, s, at tau = m s."""
    c = [0.0]
    for v in x:
        c.append(c[-1] + v)
    n = len(x) - 3 * m + 1
    total = 0.0
    for j in range(n):
        s = (c[j + 3 * m] - c[j + 2 * m]) - 2 * (c[j + 2 * m] - c[j + m]) + (c[j + m] - c[j])
        total += s * s
    return total / (2 * m**4 * n)


def tdev_ns(m: int, modified_variance: float) -> float:
    """Time deviation, ns, from the modified variance at tau = m s."""
    return m * math.sqrt(modified_variance / 3) / NS


def hat(ab: float, ac: float, bc: float) -> tuple[float, float, float]:
    """Each of three clocks' variance from the three pairs' (A-B, A-C, B-C): A's, B's, C's.
    A negative one means the three noises are not independent."""
    return (ab + ac - bc) / 2, (ab + bc - ac) / 2, (ac + bc - ab) / 2
