"""Set a counter up for a 1 PPS time interval from what the scope sees of each edge.

  tic_setup.py --scope HOST --counter ADDRESS [--model 53230A|53132A] [--fraction 0.5]
      [--impedance 1e6] [--readings 3] [--dry-run]
  tic_setup.py --levels V1,V2 --counter ADDRESS --model 53132A [--impedance 50]

Reads each channel's top, base and 10-90% rise time from an InfiniiVision scope (the MSO-X
4054A), reports them, and the level a fraction of the way up each edge (its midpoint by
default): a fixed threshold on a slow edge is crossed early, and on a fast one late, so a
time interval between a fast and a slow edge depends on it.  Then, unless --dry-run, sets
the counter to a time interval from channel 1 to channel 2 at those levels, rising edges, DC,
1 MOhm by default (the scope terminates the line in 50 ohm), one sample, immediate trigger
(and on a 53230A a measurement timeout long enough for 1 PPS); reads the settings back and
takes a few readings.  Sends nothing else.  `--levels` sets given levels instead, for lines
the scope does not see.  The counter's address is a socket host or a VISA resource
(`TCPIP::e5810a::gpib0,3::INSTR`, `instrument.py`).
"""

import argparse
import sys
from dataclasses import dataclass

import instrument
from instrument import Counter, Scpi

SCOPE_TIMEOUT_S = 10.0
COUNTER_TIMEOUT_S = instrument.MEASUREMENT_TIMEOUT_S + 2.0
"""Longer than the 53230A's own measurement timeout, so its no-measurement value comes first."""
CHANNELS = (1, 2)
RISE_FRACTION = 0.8
"""The 10-90% rise time covers this much of the swing."""
NS_PER_S = 1e9
FIXED_THRESHOLD_V = 1.0
"""A single level for both channels, reported against: the one used before this tool."""


@dataclass(frozen=True)
class Edge:
    """A rising edge as the scope measures it, volts and seconds."""

    top: float
    base: float
    rise_s: float
    """10-90%."""

    def level(self, fraction: float) -> float:
        """The voltage `fraction` of the way from base to top."""
        return self.base + fraction * (self.top - self.base)

    def lead_ns(self, threshold: float, fraction: float) -> float:
        """How far before the `fraction` point a `threshold` is crossed, ns, the edge taken as
        a straight line through its 10% and 90% points; negative when crossed after."""
        swing = self.top - self.base
        full_swing_s = self.rise_s / RISE_FRACTION
        return (self.level(fraction) - threshold) / swing * full_swing_s * NS_PER_S


def ti_error_ns(edges: dict[int, Edge], threshold: float, fraction: float) -> float:
    """What a TI from channel 1 to channel 2 at one `threshold` reads less the TI between the
    edges' `fraction` points, ns: each edge is caught its lead early."""
    return edges[1].lead_ns(threshold, fraction) - edges[2].lead_ns(threshold, fraction)


def read_edge(scope: Scpi, channel: int) -> Edge:
    def measure(name: str) -> float:
        return float(scope.query(f":MEAS:{name}? CHAN{channel}"))

    return Edge(top=measure("VTOP"), base=measure("VBAS"), rise_s=measure("RIS"))


def configure(
    counter: Scpi, model: Counter, levels: dict[int, float], impedance: float
) -> list[str]:
    """Sets the counter up; returns its settings and error queue read back."""
    for command in model.setup(levels, impedance):
        counter.send(command)
    back = [f"CH{c} level {counter.query(model.level_query.format(c=c))} V" for c in levels]
    back += [f"CH{c} impedance {counter.query(model.impedance_query.format(c=c))}" for c in levels]
    back.append(f"errors {counter.query('SYST:ERR?')}")
    return back


def parse_levels(text: str) -> dict[int, float]:
    """`V1,V2`: channel 1's level and channel 2's, volts."""
    values = [float(v) for v in text.split(",")]
    if len(values) != len(CHANNELS):
        raise argparse.ArgumentTypeError(f"levels {text!r}: one per channel")
    return dict(zip(CHANNELS, values, strict=True))


def scope_levels(scope_address: str, fraction: float) -> dict[int, float]:
    """Each channel's level `fraction` up its edge, reported against a fixed threshold."""
    scope = instrument.connect(scope_address, SCOPE_TIMEOUT_S)
    try:
        edges = {c: read_edge(scope, c) for c in CHANNELS}
    finally:
        scope.close()
    levels = {c: e.level(fraction) for c, e in edges.items()}
    fixed = FIXED_THRESHOLD_V
    print(f"channel  base V  top V  rise ns  level V ({fraction:.0%})  {fixed} V crossed")
    for c, e in edges.items():
        lead = e.lead_ns(fixed, fraction)
        print(
            f"CH{c}     {e.base:+.2f}   {e.top:+.2f}   {e.rise_s * NS_PER_S:5.1f}"
            f"    {levels[c]:.3f}            {lead:+.2f} ns early"
        )
    error = ti_error_ns(edges, fixed, fraction)
    print(
        f"at {fixed} V on both, TI (CH2 - CH1) reads {error:+.2f} ns"
        f" against the {fraction:.0%} points"
    )
    return levels


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    source = p.add_mutually_exclusive_group(required=True)
    source.add_argument("--scope", help="the scope's host, which sees both lines")
    source.add_argument("--levels", type=parse_levels, help="channel 1's and 2's levels, V1,V2")
    p.add_argument("--counter", required=True, help="the counter: a host or a VISA resource")
    p.add_argument("--model", choices=sorted(instrument.COUNTERS), default="53230A")
    p.add_argument("--fraction", type=float, default=0.5, help="where on each edge to trigger")
    p.add_argument("--impedance", type=float, default=instrument.HIGH_IMPEDANCE, help="ohms")
    p.add_argument("--readings", type=int, default=3)
    p.add_argument("--dry-run", action="store_true", help="report only; leave the counter")
    args = p.parse_args()

    levels = args.levels or scope_levels(args.scope, args.fraction)
    if args.dry_run:
        return 0
    model = instrument.COUNTERS[args.model]
    counter = instrument.connect(args.counter, COUNTER_TIMEOUT_S)
    try:
        for line in configure(counter, model, levels, args.impedance):
            print(line)
        for _ in range(args.readings):
            print("TI", counter.query(model.read))
    finally:
        counter.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
