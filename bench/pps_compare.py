"""Compare 1 PPS outputs on counters, with each receiver's sawtooth where it has a link.

  pps_compare.py log --counter ADDRESS [--counter-model 53230A|53132A] [--board DEVICE]
      [--vp DEVICE] [--module DEVICE] --seconds N --out FILE
      [--time-raim board,vp] [--step SECONDS:board|vp:OFFSET_NS ...] [--vp-message Bn|En|Hn]
  pps_compare.py analyze FILE
  pps_compare.py series A_LOG B_LOG --a-name NAME --b-name NAME --out SERIES.json
  pps_compare.py stats SERIES.json

`log` reads the counter's time interval (start channel 1, the board; stop channel 2, the VP)
once a second and every `@@Bn` from both jigs into a CSV file, no position.  The counter must
already be set up (`tic_setup.py`); the tool sets a 53230A's measurement timeout, and
reconnects after a reading that times out.  The counter's address is a socket host or a VISA
resource (`instrument.py`).  `--time-raim` sends
`@@Bn` (Time RAIM on, one a second) to the named engines first; `--step` sends `@@Ay` (the 1 PPS
offset) to an engine that many seconds into the run.  Both are RAM settings on the engine.
`--module` polls a u-blox module's TIM-TP over its USB or serial port on its own thread, so the
counter is read every second; it only polls.

`series` joins two `log` files whose counters share one start (the reference) and stop on a
receiver each, into the per-second series a report reads (`stability.py`); a log with no
sawtooth (a SmartClock's pulse) goes in as it is; `stats` gives its
ADEV, MDEV and TDEV and a three-cornered hat, and refuses a series with a missing second.
"""

import argparse
import csv
import json
import math
import sys
import threading
import time
from collections.abc import Callable
from pathlib import Path

import serial

import analysis
import instrument
import oncore
import stability
import ubx

BAUD = 9600
COUNTER_TIMEOUT_S = instrument.MEASUREMENT_TIMEOUT_S + 2.0
"""The connection's: longer than the 53230A's own, so its 9.91e37 comes first."""
NO_MEASUREMENT = 9.9e37
"""The 53230A's value for no measurement (9.91e37)."""
TIME_RAIM_ALARM_LIMIT = 10
"""Time RAIM's alarm limit, 100 ns units: 1 us."""
PULSE_ALWAYS_ON = 1
MODULE_POLL_S = 0.5
"""How often TIM-TP is polled: twice a pulse, so none is missed."""
MODULE_READ_S = 0.05
FORMATS = {"Bn": oncore.BN, "En": oncore.EN, "Hn": oncore.HN}
"""The channel 2 receiver's Time RAIM message, by name."""
FIELDS = ["host_time", "source", "ti_s", "next_pulse", "solution", "sawtooth_ns", "text"]


class Recorder:
    """The CSV file, written from several threads."""

    def __init__(self, path: Path) -> None:
        self._file = path.open("w", newline="")
        self._writer = csv.DictWriter(self._file, FIELDS)
        self._writer.writeheader()
        self._lock = threading.Lock()

    def write(self, source: str, **fields: str) -> None:
        with self._lock:
            self._writer.writerow({"host_time": f"{time.time():.6f}", "source": source, **fields})
            self._file.flush()

    def close(self) -> None:
        self._file.close()


class Engine:
    """One jig's serial port: reads its `@@Bn`s on a thread, and sends commands."""

    def __init__(
        self, name: str, device: str, recorder: Recorder, form: oncore.TimeRaimFormat
    ) -> None:
        self.name = name
        self._form = form
        self._port = serial.Serial(device, BAUD, timeout=0.2)
        self._recorder = recorder
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)
        self.count = 0

    def start(self) -> None:
        self._thread.start()

    def send(self, message: bytes, text: str) -> None:
        self._port.write(message)
        self._recorder.write("event", text=f"{self.name} {text}")

    def _run(self) -> None:
        deframer = oncore.Deframer(self._form)
        while not self._stop.is_set():
            for bn in deframer.feed(self._port.read(4096)):
                self.count += 1
                self._recorder.write(
                    self.name,
                    next_pulse="{:02d}:{:02d}:{:02d}".format(*bn.next_pulse)
                    if bn.next_pulse
                    else "",
                    solution=str(bn.solution),
                    sawtooth_ns=str(bn.sawtooth_ns),
                )

    def close(self) -> None:
        self._stop.set()
        self._thread.join()
        self._port.close()


class Module:
    """A u-blox module's port: polls TIM-TP on a thread, recording each new pulse's qErr."""

    def __init__(self, device: str, recorder: Recorder) -> None:
        self._port = serial.Serial(device, BAUD, timeout=MODULE_READ_S)
        self._recorder = recorder
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)
        self.count = 0
        self.name = stability.MODULE

    def start(self) -> None:
        self._thread.start()

    def _run(self) -> None:
        deframer = ubx.Deframer()
        last = None
        while not self._stop.is_set():
            self._port.write(ubx.frame(ubx.TIM_TP))
            deadline = time.monotonic() + MODULE_POLL_S
            while time.monotonic() < deadline and not self._stop.is_set():
                for message, payload in deframer.feed(self._port.read(4096)):
                    tp = ubx.time_pulse(payload) if message == ubx.TIM_TP else None
                    if tp and tp.tow_ms != last:
                        last = tp.tow_ms
                        self.count += 1
                        self._recorder.write(
                            stability.MODULE,
                            sawtooth_ns=f"{tp.q_err_ns:.3f}",
                            text=f"tow {tp.tow_ms}",
                        )

    def close(self) -> None:
        self._stop.set()
        self._thread.join()
        self._port.close()


class Counter:
    """The counter, set up already (`tic_setup.py`): one time interval per query."""

    def __init__(self, address: str, model: instrument.Counter) -> None:
        self._model = model
        self._scpi = instrument.connect(address, COUNTER_TIMEOUT_S)
        for command in model.session:
            self._scpi.send(command)

    def read(self) -> float:
        return float(self._scpi.query(self._model.read))

    def reconnect(self) -> None:
        """After a timeout: the counter may still be measuring, so it is stopped."""
        self._scpi.reconnect()
        self._scpi.send(self._model.stop)

    def close(self) -> None:
        self._scpi.close()


def parse_step(text: str) -> tuple[float, str, int]:
    """`SECONDS:board|vp:OFFSET_NS`."""
    seconds, name, offset = text.split(":")
    if name not in ("board", "vp"):
        raise argparse.ArgumentTypeError(f"step {text!r}: engine must be board or vp")
    return float(seconds), name, int(offset)


def missing_engines(args: argparse.Namespace) -> list[str]:
    """Engines `--time-raim` or `--step` name without a serial device."""
    present = {name for name in ("board", "vp") if getattr(args, name)}
    named = {*filter(None, args.time_raim.split(",")), *(name for _, name, _ in args.step)}
    return sorted(named - present)


def log(args: argparse.Namespace) -> int:
    if absent := missing_engines(args):
        print(f"no serial device for {', '.join(absent)}", file=sys.stderr)
        return 2
    recorder = Recorder(args.out)
    engines = {}
    if args.board:
        engines["board"] = Engine("board", args.board, recorder, oncore.BN)
    if args.vp:
        engines["vp"] = Engine("vp", args.vp, recorder, FORMATS[args.vp_message])
    module = Module(args.module, recorder) if args.module else None
    counter = Counter(args.counter, instrument.COUNTERS[args.counter_model])
    for engine in engines.values():
        engine.start()
    if module:
        module.start()
    for name in args.time_raim.split(",") if args.time_raim else []:
        engines[name].send(
            oncore.time_raim_command(1, PULSE_ALWAYS_ON, TIME_RAIM_ALARM_LIMIT), "Bn rate 1"
        )
    steps = sorted(args.step)
    start = time.monotonic()
    missing = 0
    try:
        while (elapsed := time.monotonic() - start) < args.seconds:
            while steps and steps[0][0] <= elapsed:
                _, name, offset = steps.pop(0)
                engines[name].send(oncore.pulse_offset_command(offset), f"Ay {offset}")
            try:
                value = counter.read()
            except TimeoutError:
                print("counter: no reading within its timeout; reconnecting", file=sys.stderr)
                counter.reconnect()
                value = NO_MEASUREMENT
            if value >= NO_MEASUREMENT:
                missing += 1
                recorder.write("counter", ti_s="")
            else:
                recorder.write("counter", ti_s=repr(value))
    finally:
        counter.close()
        for engine in engines.values():
            engine.close()
        if module:
            module.close()
        recorder.close()
    counts = ", ".join(
        [f"{e.name} {e.count} Bn" for e in engines.values()]
        + ([f"module {module.count} TIM-TP"] if module else [])
    )
    print(f"done: {missing} readings without a measurement; {counts}", file=sys.stderr)
    return 0


def analyze(args: argparse.Namespace) -> int:
    data = analysis.read_log(args.file)
    for line in [*analysis.sawtooth_report(data), *analysis.step_report(data)]:
        print(line)
    return 0


def only_source(log: analysis.Log, path: Path) -> str | None:
    """A log's one sawtooth source, or None for a log with none; raises ValueError if it has
    several."""
    sources = sorted(log.sawtooths)
    if len(sources) > 1:
        raise ValueError(f"{path}: sawtooth sources {sources}; name one")
    return sources[0] if sources else None


def make_series(args: argparse.Namespace) -> int:
    sides = []
    for path, source in ((args.a_log, args.a_source), (args.b_log, args.b_source)):
        log_data = analysis.read_log(path)
        try:
            source = source or only_source(log_data, path)
        except ValueError as error:
            print(error, file=sys.stderr)
            return 2
        by_second, outside = stability.seconds(log_data, source)
        print(
            f"{path}: {source or 'no sawtooth'}, {len(by_second)} seconds,"
            f" {outside} readings outside the window",
            file=sys.stderr,
        )
        sides.append(by_second)
    joined = stability.series(sides[0], sides[1], args.a_name, args.b_name)
    meta = [
        {
            "label": "Inputs",
            "value": f"{args.a_log.name} ({args.a_name}), {args.b_log.name} ({args.b_name})",
        },
        {"label": "Missing seconds", "value": str(len(joined.missing))},
    ]
    args.out.write_text(json.dumps({"rows": joined.rows, "missing": joined.missing, "meta": meta}))
    print(f"{len(joined.rows)} seconds, {len(joined.missing)} missing", file=sys.stderr)
    return 0


def hat_cells(m: int, allan: float, modified: float) -> str:
    """One clock's ADEV and TDEV from its hat variances; n/a for a negative one."""
    adev = f"{math.sqrt(allan):.2e}" if allan > 0 else "n/a"
    tdev = f"{stability.tdev_ns(m, modified):.2f}" if modified > 0 else "n/a"
    return f"{adev:>12}{tdev:>14}"


def stats(args: argparse.Namespace) -> int:
    data = json.loads(args.series.read_text())
    if data.get("missing"):
        print(
            f"{len(data['missing'])} seconds missing: ADEV, MDEV and TDEV need a gap-free series",
            file=sys.stderr,
        )
        return 1
    rows = data["rows"]
    corrected = sorted(k for k in rows[0] if k.endswith("_corr"))
    columns = {k.removesuffix("_corr"): [r[k] * stability.NS for r in rows] for k in corrected}
    columns["diff"] = [r["diff"] * stability.NS for r in rows]
    names = list(columns)
    taus = [m for m in stability.TAUS if 3 * m <= len(rows)]
    print(f"{len(rows)} seconds; each pulse less the reference, and {names[0]} - {names[1]}")
    print(
        "tau s "
        + "".join(f" {n + ' ADEV':>14} {n + ' MDEV':>14} {n + ' TDEV ns':>14}" for n in names)
    )
    for m in taus:
        cells = []
        for x in columns.values():
            mv = stability.mvar(x, m)
            adev = math.sqrt(stability.avar(x, m))
            cells.append(f" {adev:14.2e} {math.sqrt(mv):14.2e} {stability.tdev_ns(m, mv):14.2f}")
        print(f"{m:5d} " + "".join(cells))
    a, b = names[0], names[1]
    print(f"three-cornered hat: the reference, {a}, {b} (n/a: a negative variance)")
    print(
        "tau s      ref ADEV   ref TDEV ns"
        + f"{a + ' ADEV':>14}{a + ' TDEV ns':>14}{b + ' ADEV':>14}{b + ' TDEV ns':>14}"
    )
    xa, xb, xd = columns[a], columns[b], columns["diff"]
    for m in taus:
        # The pairs are A - ref, A - B and B - ref: the hat gives A's, ref's and B's.
        allan = stability.hat(stability.avar(xa, m), stability.avar(xd, m), stability.avar(xb, m))
        modified = stability.hat(
            stability.mvar(xa, m), stability.mvar(xd, m), stability.mvar(xb, m)
        )
        row = "".join(hat_cells(m, allan[i], modified[i]) for i in (1, 0, 2))
        print(f"{m:5d} {row}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawTextHelpFormatter
    )
    commands = parser.add_subparsers(required=True)
    p = commands.add_parser("log")
    p.add_argument("--counter", required=True, help="the counter: a host or a VISA resource")
    p.add_argument("--counter-model", choices=sorted(instrument.COUNTERS), default="53230A")
    p.add_argument(
        "--board", help="the channel 1 board's serial device; leave out when it has none"
    )
    p.add_argument(
        "--vp", help="the channel 2 receiver's serial device; leave out when it has none"
    )
    p.add_argument("--seconds", type=float, required=True)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--time-raim", default="", help="engines to send @@Bn rate 1: board,vp")
    p.add_argument(
        "--vp-message",
        choices=sorted(FORMATS),
        default="Bn",
        help="the channel 2 receiver's Time RAIM message: Bn (VP), En (8-channel VP)"
        " or Hn (M12+, already sent)",
    )
    p.add_argument("--step", type=parse_step, action="append", default=[])
    p.add_argument(
        "--module", help="a u-blox module's USB or serial device, for its TIM-TP qErr (polls only)"
    )
    p.set_defaults(run=log)
    p = commands.add_parser("analyze")
    p.add_argument("file", type=Path)
    p.set_defaults(run=analyze)
    p = commands.add_parser("series")
    p.add_argument("a_log", type=Path)
    p.add_argument("b_log", type=Path)
    p.add_argument("--a-name", required=True, help="receiver A's key in the series, e.g. ublox")
    p.add_argument("--b-name", required=True, help="receiver B's key in the series, e.g. vp")
    p.add_argument("--a-source", help="A's sawtooth source in its log, if it has several")
    p.add_argument("--b-source", help="B's sawtooth source in its log, if it has several")
    p.add_argument("--out", type=Path, required=True)
    p.set_defaults(run=make_series)
    p = commands.add_parser("stats")
    p.add_argument("series", type=Path)
    p.set_defaults(run=stats)
    args = parser.parse_args()
    run: Callable[[argparse.Namespace], int] = args.run
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
