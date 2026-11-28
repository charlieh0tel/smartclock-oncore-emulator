# Bench tools

Run with `uv run` from this directory.  Device paths and hosts are
arguments; none are kept here.

## `pps_compare.py`

1 PPS outputs on 53230A or 53132A counters: start on channel 1, stop on
channel 2, cables of equal length.  For two receivers against one
reference, each counter starts on the reference, teed to both, and stops
on its receiver (`docs/vp-on-jig.md`, "Three receivers on one antenna").  The counter is set to `CONF:TINT (@1),(@2)`, one sample,
immediate trigger, before a run.

- `log`: the counter's TI once a second and every `@@Bn` from both jigs,
  to CSV, with no position.  `--time-raim board,vp` turns on `@@Bn` at
  rate 1; `--step SECONDS:board|vp:NS` sends `@@Ay` during the run.
  Both are RAM settings on the engine.
- `analyze`: TI unwrapped into half a second either way; each engine's
  sawtooth against TI, matched by host time, and its meaning: a VP's
  correlation shows its pulse late by its sawtooth, and the board's
  is judged against that; and the median TI after each `@@Ay` step.
  `--vp` is optional (a reference with no serial link), and
  `--vp-message En` reads an 8-channel VP's `@@En` and `--vp-message Hn`
  an M12+'s `@@Hn` instead of `@@Bn`.  `--module DEVICE` polls a u-blox
  module's UBX-TIM-TP over its USB or serial port on its own thread and
  logs each pulse's qErr (`ubx.py`); it only polls.
- `series A_LOG B_LOG --a-name ublox --b-name vp --out FILE.json`: two
  `log` files whose counters start on one reference and stop on a
  receiver each, joined by the host second of the pulse each reading
  measured (a slow read keeps its second) into the per-second series a
  report reads: each receiver's TI, its sawtooth and the corrected TI
  (a module's pulse late by -qErr, an Oncore's by its sawtooth; a log
  with no sawtooth, a SmartClock's pulse, as it is, sawtooth 0), and
  A - B, the reference cancelling.  A reading outside 0 to 5 us (a
  start taken on the reference's falling edge reads 0.8 s) leaves its
  second missing, and the file lists the missing seconds
  (`stability.py`).
- `stats FILE.json`: overlapping ADEV, MDEV and TDEV of each corrected
  series and of A - B, tau0 = 1 s, and a three-cornered hat giving the
  reference's and each receiver's ADEV and TDEV.  It refuses a series
  with a missing second.

## `stm32boot.py`

Flashes an STM32 through its ROM serial bootloader (ST AN3155, 8E1):
`id` reads the product ID, `write` mass-erases, writes a binary from
`llvm-objcopy -O binary` at 0x08000000 and reads it back.  For parts
`stm32flash` 0.7 does not know, the STM32C091 (0x44d) among them.

- `--knock` first starts the emulator board's bootloader over the
  instrument's port (J1 through the jig, or J5): the bench knock,
  `@@Zb` `LOAD`, at 9600 8N1, answered with the same frame.  The board
  must already run firmware that takes it (PLAN.md, "Bootloader
  re-entry").  A blank board starts its bootloader itself.
- `build` reads flash from its start until it finds the build
  identity (`ONCORE-BUILD`, commit, features, date) and prints it, as
  `firmware/tools/board_serial.py` does over SWD: the check for a
  board with no debug probe.  About 20 s with the knock for a 110 KB
  image; a flash with none is read to its end, about 50 s, and exits 1.
- `--go` starts the firmware after `write` or `build` (the emulator's
  resets itself out of the bootloader's state); without it,
  power-cycle the board.  A blank board's first flash needs a power
  cycle, not `--go`.  The knock restarts the board: bench only, never
  in an instrument mid-run.
- `--relay` reaches a board in a SmartClock through the instrument's
  console relay, started first with `smartclock-cli passthrough --baud
  9600 --framing 8N1 --then-baud 19200 --then-framing 8E1`, DEVICE the
  instrument's port: 22 zeros (the board's longest command, so a
  command the frozen instrument had begun ends before the knock), the
  knock and its echo at 9600 8N1, a 20 ms break (timed with the break
  condition: pyserial's `send_break` holds at least 0.25 s on Linux)
  that switches the relay and the port to 19200 8E1, the relay's `E`,
  the bootloader at that rate, always Go
  (the board's power stays on), and a last break that resets the
  instrument.  On any error it stops with the instrument still frozen.
  `id` and `build` only: it refuses `write`, since a failed write in a
  closed instrument needs SWD.  Not yet run on hardware.

An update: `uv run python stm32boot.py write --knock --go DEVICE
oncore-fw.bin`.  Its build: `uv run python stm32boot.py build --knock --go DEVICE`.

## `tic_setup.py`

Sets the 53230A up for a 1 PPS time interval from the scope's view of
each edge: reports each channel's base, top and 10-90% rise, and sets
each channel's level at the edge's midpoint (`--fraction`), with the
measurement timeout 1 PPS needs.  `--dry-run` reports only.  The
procedure is the `pps-tic-measurement` skill
(`.claude/skills/pps-tic-measurement/`).

What it checks: `TODO.md`, "Check on a module".

## `almanac.py`

Loads a GPS almanac into a VP Oncore with `@@Cb`, the 34 pages of
subframes 4 and 5 from a u-blox module's RXM-SFRBX in a raw UBX file
(`firmware/testdata/` holds a whole one).  `show` checks the file has
every page from one almanac; `send` sends them, subframe 5 page 1
first, and waits for each page's `@@Ch` echo.  The VP stores the
almanac in its EEPROM.  A VP whose almanac was cleared (`@@Cf`) starts
tracking from it without collecting one from the sky.

## `ephemeris.py`

GPS satellite positions from broadcast ephemerides (RINEX 3
navigation data, its GPS records; IS-GPS-200 20.3.3.4.3), and the
timing error of a receiver holding a wrong position.  `hold-error`
takes the true and held positions (`LAT,LON,HEIGHT`, WGS 84,
ellipsoidal height) and a span of UTC unix seconds, and writes CSV:
per step, the satellites above the mask at the true position and
the all-in-view error, the mean of each satellite's (range from
true - range from held) / c, plain and weighted by sin^2(elevation);
positive is a late pulse.  `--per-satellite` writes each satellite's
elevation, azimuth and error instead.  Daily files are on BKG's
`igs.bkg.bund.de/root_ftp/IGS/BRDC/YEAR/DOY/`; the `_S_` (streamed)
file of the current day is fuller than the `_R_` one.

What to check: positions agree with an IGS SP3 orbit within a few
meters.
