---
name: pps-tic-measurement
description: Measure the time interval between 1 PPS signals on the bench's 53230A and 53132A counters, set up from the MSO-X 4054A scope's view of each edge (levels at each edge's midpoint), checked against the scope, logged with pps_compare.py, and reported with ADEV, MDEV, TDEV and a three-cornered hat on a comparison page. Use when comparing 1 PPS outputs (the board, a VP Oncore, a SmartClock, a CNS Clock II or any reference) or when the user asks for a TIC or 1 PPS measurement.
---

# 1 PPS time-interval measurement

The counter and the scope are mine to set up for an agreed test; the
receivers and instruments on the other end are not (AGENTS.md,
"Hardware"): only read-only queries to those without asking.  Cables,
terminations and which signal is on which channel are the owner's.

## Before measuring

1. Ask, or confirm, what is on counter channels 1 and 2 (TI runs from a
   channel 1 edge to the next channel 2 one).  The scope sees the same
   lines, in 50 ohm; the counter sits mid-line at 1 MOhm.
2. List the serial devices (`ls /dev/serial/by-id/`) before naming any:
   jigs and receivers come and go.
3. Run `bench/tic_setup.py --scope HOST --counter HOST --dry-run` and
   report its table: each edge's base, top and 10-90% rise from the
   scope, the level at its midpoint, and how far a single 1.0 V level
   would put TI off the midpoints.  A slow edge (a SmartClock's 1 PPS is
   about 20 ns, the jig's about 3 ns) is crossed early at a low level.
4. Ask whether the midpoint is the right reference for each source (a
   data sheet may time its pulse at a TTL threshold, about 1.4 V); pass
   `--fraction` otherwise.  The practice:
   - the midpoint of the rising edge: ITU-T G.703 (04/2016) section 19
     ("the significant event occurring on the midpoint of the leading
     edge", 50 ohm, 10-90% rise under 5 ns), which G.8271 Annex A takes;
     IRIG 200-04 section 3.2.2 (50 percent amplitude points); Keysight
     5991-1943EN ("50% amplitude point on pulse edge").  The default.
   - a fixed 1.0 V, DC, 50 ohm, rising edge: BIPM's GNSS equipment
     calibration guidelines, Annex 1 section 2, for time-transfer
     receivers; the crossing moves with rise time, so BIPM asks for the
     rise time too.  Use it to match such a calibration.
   - none found says how to compare edges of different rise times.
   Whichever is used, record the level, the load and each rise time
   with the result.
5. Run `bench/tic_setup.py` without `--dry-run`: it sets
   `CONF:TINT (@1),(@2)`, each channel's level, rising edges, DC,
   1 MOhm, one sample, immediate trigger and `SYST:TIM 5` (the default
   1 s times out on 1 PPS), reads the levels back and takes readings.
6. Check one reading against the scope at 100 ns/div or finer: the order
   of the edges and the rough interval must agree ("always check a TIC
   with a 'scope").

## Measuring

- `bench/pps_compare.py log` once a second to CSV (no positions), with
  each receiver's sawtooth where it has one: `--time-raim board` for the
  board's `@@Bn`, `--vp DEVICE` for a VP's `@@Bn`, `--vp-message Hn` for
  an M12+'s `@@Hn`; leave `--vp` out for a source with no serial link.
- Run it in the background and check it records within seconds; do not
  start a long run in the middle of another on the same counter.
- Both receivers in position hold on the same surveyed position, on the
  same antenna path, for a comparison of their pulses.
- A source still settling (a SmartClock in "stabilizing frequency")
  drifts: report the run in segments, and its own reported TI beside it.

## Reporting

- `bench/pps_compare.py analyze FILE`: readings are unwrapped into half
  a second either way, so a channel 2 edge just ahead of channel 1's
  reads negative.  Report the median, the spread with and without each
  sawtooth, and each sawtooth's correlation and the sign it gives.
- Account for each path's fixed delays (buffers and drivers from their
  data sheets, cable lengths at their velocity factor, a reference's own
  stated internal delay) in a table, and say which are estimates.
- Record results in `docs/vp-on-jig.md` (or the relevant doc), with the
  counter's levels and the scope's edges.

## Two receivers against one reference

The joint run (`docs/vp-on-jig.md`, "Three receivers on one antenna"):
the reference (the CNS Clock II's rear 1 PPS) teed into both counters'
channel 1, each receiver's 1 PPS on its counter's channel 2, every line
then on to the scope, terminated there.  The CNS cancels out of
receiver A - receiver B.

1. Same antenna and matched cables for every receiver; all in position
   hold at the same position (the CNS's, read from GPSTime's raw log;
   ellipsoid height for the u-blox, `@@As` type 0 for a VP).  Positions
   go to the receivers only, never into a file, a page or a message.
2. Check the reference stays early of every receiver (the CNS's 1 us,
   `@@Ay` 999 999 000), and read its `@@Ay` and `@@Az` from GPSTime's raw
   log; record both with the run.
3. `tic_setup.py` on each counter: the same level on both starts (the
   reference edge's 50% point), each stop at its own edge's; 1 MOhm, DC.
4. Both logs at once, detached:
   - `pps_compare.py log --counter <53230A> --module <u-blox USB, by-path> ...`
   - `pps_compare.py log --counter <53132A VISA> --counter-model 53132A --vp <VP> --vp-message En ...`
   Two u-blox modules share one `/dev/serial/by-id` name: use
   `/dev/serial/by-path`, and check NAV-PVT (a fix, or none for a board
   with no antenna) before trusting which is which.
5. Watch for gaps while it runs (a missing second, a reading outside
   0 to 5 us); ADEV, MDEV and TDEV need none.  Fix the cause and rerun
   rather than patch a gap.
6. `pps_compare.py series A B --a-name A_KEY --b-name B_KEY --out FILE.json`,
   then `pps_compare.py stats FILE.json`.  The keys name each receiver's
   columns (`board`, `smartclock`, `ublox`, `vp`); a log with no sawtooth
   (a SmartClock's pulse) goes in as it is.  Each reading is placed at the
   second of the pulse it measured, so a slow read is no gap.
7. For a source still settling, cut the series where its own reported TI
   stops swinging (a 58503A's loop takes about 11 h after lock) and report
   the settled segment; give the settling one beside it with its own TI.

## The comparison page

A comparison page is an Artifact of the Data type: every figure is worked
out on the page from one per-second file, the `series` output, as dataset
`series`.  One page per run:

| Page | Pairing (`a`, `b`, reference) |
| ---- | ----------------------------- |
| https://claude.ai/artifact/BKLhqv7PK5DPNSiXxLh1z2 | `board` (0005 on the jig), `smartclock` (58503A with 0001), CNS Clock II; 2026-11-22 |
| https://claude.ai/artifact/L5ue1ygEgRwf5JkzwZyp14 | `ublox` (LEA-M8T), `vp` (VP Oncore), CNS Clock II; 2026-11-18 (names written into its script) |

The first is the template.  Its script's first block, `window.lv.PAIR`,
holds each receiver's series key, name and counter, and the reference's
name and how early its pulse is set (`earlyNs`); nothing else in the
script names a receiver.  A receiver with no sawtooth shows as such.

For a new run:

1. Create a Data Artifact (`Artifact` publish with the Data type's
   `type_url`, a `title`, `auto_open: "after_first_write"`).
2. Upload the `series` file to it (`Artifact` publish with `asset: true`)
   and note its `/_blob/<id>`.  Round values to 0.001 ns to keep it small.
3. Read the template's `files/index.html` (`ArtifactData` get), set
   `PAIR`, and rewrite the page's words for the run: the title, the
   window, the setup, the receivers' names in the headings, legends and
   table heads, and each `data-where="receiver=..."` to the names in
   `PAIR`.
4. `ArtifactData` batch: `dash/meta` (`title`, `lang`), `datasets/series`
   (`source: {kind: "file", url, name, format: "json"}`, `fixed: true`,
   `updated: {at, by}`) and `files/index.html` (`{text}`).
5. Read `data/users/me/check` until it passes; fix what it names.

For a new window of the same run, steps 2 and 4's `datasets/series`
only, pinned to the version read, and the window's words.
