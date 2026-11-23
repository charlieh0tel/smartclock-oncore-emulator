# VP Oncores on the jig

Results from jig 1.0.0 with the two B1121P1114 engines, 2026-11-05 and
2026-11-06, and the 58503A's B4121P1115, 2026-11-08 to 2026-11-20.  Steps are `docs/bring-up.md`'s.  Logs stay out of the
repository: `@@Ba` carries the bench's position.

## Engines

Both B1121P1114, software 8.4, options IB, `@@Cj` read on the jig.

| Serial | Manufactured | From |
| ------ | ------------ | ---- |
| SSG0178541 | 6J25 | Z3801A 3542A01548 |
| SSG0163878 | 6G09 | Z3805A 3625A01487 |

Both are deaf without gain, and track with 20 dB (Tracking, below).
Instrument history is in smartclock's `docs/bench.md` and
`docs/firmware/gps.md`.

## The 58503A's engine

B4121P1115, 8 channels, software 8.8 (98-P36830P, 06 Aug 1996), options
IB, serial SSG0220999, manufactured 7D01: the bench 58503A's, on the
jig 2026-11-08.  Its settings, as the 58503A left them, read with the
reference's query forms only (nothing set), positions left out:

| Setting | Value |
| ------- | ----- |
| Sent unprompted at power-up | `@@Ea`, `@@En`, `@@Bb`: the output rates survived power-down |
| `Aa`, `Ac` (before tracking) | the time of day right; the date 2007-03-19 |
| `Ac` (tracking) | 2007-03-25: 1024 weeks early |
| `Ab` GMT offset | 0 |
| `Aw` | 1, UTC |
| `Ag` mask angle | 10 degrees |
| `Ah` satellite select | 2 |
| `Aj` xDOP type | 0 |
| `Ak` xDOP hysteresis | 1.0 |
| `Al` 3-D to 2-D threshold | 6.0 |
| `AC` 2-D to 0-D threshold | 12.0 |
| `Am` ignored | none |
| `An`, `Aq`, `Ar` | 1, 1, 1 |
| `At` position hold | 1, on |
| `Av` altitude hold | 0, off |
| `AB` application | 4 |
| `Ax`, `Ay`, `Az` | 0, 0, 0 |
| `AA`, `AE` | 0, 0 |
| `AD` | 0x0020 |
| `AJ` | 90 |
| `Ao` with `0xff` | no answer |
| `Bn` (query) | rate 0, Time RAIM on, alarm limit 10 (1.0 us), pulse mode 1 |

Tracking on the jig's feed (its gain ahead of the distribution
amplifier, no inline amplifier), about 10 minutes after power-up:
8 tracked, all mode 8, 10 visible, signal strengths 44 to 218.

`@@Ba` polled once (`@@Ba` 0; its rate was 0 and stayed 0): the
6-channel message, 68 bytes, the first six of its eight channels in
`@@Ea`'s order, tracked 8.

## 1 PPS against the board

The 58503A's engine in position hold on its own stored position, the
board (production build, 7bbd6b3) in position hold on the same position
(`@@As`, `@@At`), 0.2 m from the board's fix.  The 53230A measures TI
from the board's pulse (channel 1) to the engine's (channel 2), 1 MOhm,
1.0 V, `SYST:TIM` 5 s; the scope (50 Ohm) agrees: the engine's pulse
first.  `bench/pps_compare.py`, both engines' `@@Bn` once a second.

| Check | Result |
| ----- | ------ |
| Median TI, 10 minutes, 301 readings | 999,999,916.7 ns: the engine's pulse 83 ns before the board's |
| TI's spread | 31.5 ns; 3.0 ns with both sawtooths taken out |
| The engine's sawtooth against TI | r +0.98: its pulse late by its sawtooth |
| The board's sawtooth (-qErr) against TI | r -0.24, the same meaning as the engine's |
| Board `@@Ay` +500 ns | TI 500 ns less: the board's pulse later |
| Engine `@@Ay` +500 ns | TI 500 ns more: the engine's pulse later |

`@@Ay` moves the pulse later on a VP, as the firmware takes it.  Both
`@@Ay` were set back to 0 and the engine's `@@Bn` to its rate 0 after
(read back with the query forms).

## The 58503A against the board

The 58503A (3704-C) with this engine back in, in position hold on its
own stored position, its rear 1 PPS on channel 2; the board on channel 1
in position hold on the same position.  Both counter levels 1.0 V: on
these edges (3.6 ns and 19.6 ns, 10-90%) that reads about 3.4 ns short
of the midpoints.  A power-up with the warm-up run; "Locked to GPS:
stabilizing frequency" at 15:09.  Its own `:SYST:STAT?` polled every
2 minutes beside the counter.

| Check | Result |
| ----- | ------ |
| Its 1 PPS against the board's, 15:40-17:21, 3,026 readings, the board's sawtooth out | 61.5 ns before, 4.8 ns sd; fifths -61.0, -63.5, -65.9, -59.5, -61.0 |
| Its own "1PPS TI relative to GPS", 47 polls | median +6.8 ns, trimmed mean +14.8 ns (single readings, the engine's sawtooth in them) |
| Before it settled: the counter against its own reading | its own 78 to 84 ns above the counter's, the engine's offset from the board |
| FFOM | 1 ("stabilizing frequency") throughout: the start-up loop waits for the oscillator-aging fit's first three 2,700 s samples (smartclock `docs/firmware/loop.md`) |

With its engine 82 ns before the board (above) and its own pulse
+7 to +15 ns after its engine, it would be 67 to 75 ns before the
board; it is 61.5 ns before.  The few ns left are its own output stage
and the 1.0 V level, not measured.

At FFOM 0 (from 18:02), four hours from 17:59, counter levels at each
edge's midpoint:

| Check | Result |
| ----- | ------ |
| Its 1 PPS against the board's, 12,698 readings | 110.2 ns before, 43.2 ns sd; quarters -157, -99, -80, -112 |
| Its own "1PPS TI relative to GPS", 48 polls | mean -45.4 ns; the counter follows it, r +0.97, slope 0.97 |
| The counter less its own reading, at each poll | 64.6 ns before, 10.3 ns sd |

On the 500 s loop the 58503A's pulse wanders tens of ns against its
receiver's over tens of minutes; an hour's mean can be some 45 ns off.
The counter less its own reading takes the wander out.

## Set-up

- Antenna through a powered splitter that also feeds a working receiver.
  No inline amplifier, except where the tracking table says: a Raven
  Industries LA-21-1575-100-T (20 dB, noise figure under 3 dB;
  smartclock's `docs/hardware-investigations.md`).
- USB power, through a serial server at 9600 8N1.
- Each engine's clock had reset when powered on the jig, to 2007-01-01
  or 2026-01-01; set from UTC with `@@Ac` and `@@Aa`, so about 18 s off
  in GPS time mode (`@@Aw` 0).
- Z3801A 3543 start-up sent on each engine: script `0x48` (no `@@Cf`),
  then `@@Aw`, `@@Ag`, `@@Az`, `@@Am`, `@@At` and `@@Bn` at the engine's
  stored values, 1 PPS mode 1.  No `@@As`: each held the position it
  came with.  The start-up changed none of the engines' settings, except
  `@@At` to 0 and back to 1 on SSG0178541.  `@@As` to the surveyed
  position, sent to both on 2026-11-06, changed nothing: both held it.

## Results

| Check | SSG0178541 | SSG0163878 |
| ----- | ---------- | ---------- |
| +5V, JP1 pin 2 | 5.03 V | not measured |
| J9 | 20 mV, 200 mA | 20 mV, 200 mA |
| D1 | lit | not checked |
| `@@Cj` | answers | answers |
| `@@Ca` | passes, `0x0000`, 7.1 s | passes, `0x0000`, 5.2 s |
| After `@@Ca` | Idle until `@@Cg` 1 | Idle until `@@Cg` 1 |
| 1 PPS, both outputs | good, on a scope | good, on a scope |
| J6 RXD1, TXD1 | good, on a scope | not checked |
| Position held | surveyed position, `@@At` 1 | surveyed position, `@@At` 1 |
| `@@Bd` | valid, week 136, t 57, 32 SVs, RAM = EEPROM | the same |
| `@@Ba` frames with bad checksums | 0 of 40,655 | 0 of 59 before the start-up |

Stored settings, read with each setting's query form, the same on both
engines: `@@Ab` 0 0 0, `@@Ag` 10, `@@Ah` 2, `@@Aj` 0, `@@Ak` 10, `@@Al`
60, `@@Am` none, `@@An` 1, `@@Ao` 49, `@@Aq` 1, `@@Ar` 1, `@@At` 1,
`@@Av` 0, `@@Aw` 0, `@@Ax` 0, `@@Ay` 0, `@@Az` 0, `@@AB` 4, `@@AC` 120,
`@@AD` 0 32; `@@Bn` rate 1, Time RAIM on, limit 10, 1 PPS mode 1.

### Almanac

Each engine's `@@Bb` against a NEO-M8T's satellite list at the same
minute, GPS satellites above the horizon: every satellite the engine
listed is on the NEO-M8T's list, within 1 degree in elevation and
azimuth.  The NEO-M8T's extra satellites were at 5 degrees or lower.
The NEO-M8T is on its own antenna.

### Tracking

| Engine | Time | Tracked | Channels |
| ------ | ---- | ------- | -------- |
| SSG0178541 | about 11.3 hours | 0 in every `@@Ba` | code search, but SV 28 in mode 3 at SS 14 in one frame, 26,852 s into one run; back in search by the next run, seconds later |
| SSG0163878, no amplifier | 4.1 hours to first acquisition | 1, for 16 minutes | SV 7, mode 5, SS 15 to 18 |
| SSG0163878, 20 dB inline | about a minute to reacquire | 6 within 2 minutes | 5 in mode 8; SS 36 to 112, SV 7 at about 80 |

| SSG0178541, 20 dB in the feed | tracking when first read | 6 | 6 in mode 8 |
| SSG0163878, 20 dB in the feed | tracking when first read | 6 | 6 in mode 8 |

The amplifier first sat between the splitter and SSG0163878, fitted while
the engine was searching, 7 minutes after it lost SV 7.  For the last two
rows both engines shared the feed, each on its own jig and splitter port.

Mean SS in mode 8, one minute, 2026-11-06 22:54 UTC, 20 dB in the feed:

| SV | SSG0178541 | SSG0163878 | Difference |
| -- | ---------- | ---------- | ---------- |
| 7 | 95 | 68 | 2.9 dB |
| 14 | 82 | 58 | 3.0 dB |
| 17 | 97 | 70 | 2.8 dB |
| 20 | 57 | 40 | 3.1 dB |
| 22 | 89 | 64 | 2.9 dB |
| 30 | 130 | 93 | 2.9 dB |

Difference is 20 log10 of the SS ratio.  Each engine had its own splitter
port and cable, so part of the difference may be the path.  Swapping them
would separate the two.

### Position hold

SSG0220999, `@@At` 1, its `@@Ea` read every 30 s for 30 minutes from
antenna connection, 2026-11-16: its current DOP 0.0, DOP type 0 and
receiver status `0x08` (position hold; no fix bit) in every frame, from
the first satellite tracked, through the almanac (status `0x09`, bad
almanac, until 15 minutes in), to 6 and 7 satellites in mode 8 with 10
and 11 visible.  smartclock-gps's Engine sends the same while holding
(`Engine::solution`).

## `@@Bb` cadence

`@@Bb` mode 1.  Arrival times are the host's, through the serial server,
to about a second.  `@@At` 1 except in the first row.

| Engine | Tracked | `@@Bb` intervals | Satellites listed | Mean interval |
| ------ | ------- | ---------------- | ----------------- | ------------- |
| SSG0178541, `@@At` 0 | 0 | 70 | 9 | 3.19 s |
| SSG0178541 | 0 | 49 | 9 | 3.20 s |
| SSG0178541 | 0 | 60 | 9 | 3.22 s |
| SSG0178541 | 0 | 467 | 8 | 2.27 s |
| SSG0163878 | 0 | 6 | 8 | 2.33 s |
| SSG0163878 | 0 | 93 | 9 | 2.69 s |
| SSG0163878 | 0 | 911 | 10 | 3.24 s |
| SSG0163878 | 0 | 93 | 11 | 3.28 s |
| SSG0163878, signal lost | 0 | 27 | 10 | 4.3 s |
| SSG0163878 | 1 | 266 | 10 | 3.46 s |
| SSG0163878 | 6 | 67 | 12 | 6.76 s |
| SSG0163878 | 6 | 55 | 10 | 6.87 s |

- Intervals counted have the same count at both ends.
- With 6 tracked the interval is 6 or 7 s.  With fewer it is shorter.
- While tracking, Doppler changes in every frame, elevation and azimuth
  in some.
- Intervals seen, none tracked: 1 to 4 s, 5 s with the signal lost.
- No frame was identical to the one before.
- Each start-up run had one frame listing 0 satellites.
- VPCommands.pdf p. 75 gives mode 1 as "when visibility data changes".
  The emulator sends it at each alert computation, every 5 s.


## The mask angle

SSG0220999 (B4121P1115, 8.8) on the jig, 2026-11-15 and 16 UTC, `@@Ag`
set in RAM and put back to 10, the start conditions read by query
(`@@Ag` 10, `@@Ah` 2).  Elevations are `@@Bb`'s whole degrees.

| `@@Ag` | `@@Bb`'s lowest listed | Channels (`@@Ea`) |
| ------ | ---------------------- | ----------------- |
| 10 | 21 (no satellite from 10 to 20) | not read |
| 20 | 21 | not read |
| 0 | 0, 6, 8 | not read |
| 19 | 19: PRN 19 at 19, PRN 5 at 11 and 29 at 17 gone | PRN 19 kept about 100 s, until it set to 18 |
| 73 | 73: PRN 11 at 73, everything lower gone within 11 s | PRN 11, 13 and 12, all mode 8, 2 minutes |

- `@@Bb` lists the satellites at or above `@@Ag`, not a fixed 10
  degrees: SSG0163878's `@@Bb` floor at 10 was its `@@Ag` 10.
- A satellite at the mask, in whole degrees, stays on a channel: VP
  p. 27's "greater than" is not visible at that resolution.
- `@@Bb` answers a poll from its last alert computation: 8 s after a
  change it still listed a satellite below the new mask.

After `@@Cf` and a power cycle without backup power, 2026-11-16: no
output unasked, the clock at 1997-01-01 12:00:00 and not running,
receiver status 0x01 (almanac bad), `@@Bb` empty; the settings read
`@@Cf`'s: `@@Ah` 0, `@@Aj` 1, `@@At` 0, `@@AB` 2, `@@Aw` 1, `@@Ag` 10.
Before `@@Cf` it had 9 visible, 7 tracked, status 0x08, and a power
cycle without backup kept the almanac (2026-11-08), so `@@Cf` dropped
it.  Restored from the saved state, it tracked again and had the
almanac back 25 minutes later.

Its saved state restored (settings, hold position, `@@Ea`, `@@En` with
Time RAIM, `@@Bb`, `@@Cg` 1), then a power cycle without backup power,
2026-11-16: `@@Ea` and `@@En` every second and `@@Bb` unasked, fixing
(all 8 channels mode 8 within about a minute, the date and time from the
satellites, status 0x08), `@@En` Time RAIM on, limit 10, pulse mode 1,
and all 20 queried settings as saved.  The settings, the output rates,
the Time RAIM setup and the fix mode are kept without backup power; the
clock is not.

`@@Cf` then `@@Ac` and `@@Aa`, no antenna, no power cycle, 2026-11-16,
each step watched 30 s:

| After | `@@Ea` poll's date and time | `@@Aa`/`@@Ac` queries |
|---|---|---|
| `@@Cf` | the year kept, 01-01 12:00:00, not advancing | advancing 1 s/s from out-of-range values (hour 64, year 2054) |
| `@@Ac`, a date | that date, 12:00:00, not advancing | advancing from 12:00:00 on that date |
| `@@Aa`, a time | that time, not advancing | advancing from that time |

Without satellites neither command makes `@@Ea`'s time advance;
`@@Ac` sets the time of day to 12:00:00.  The saved state was restored
and read back as saved.

`@@Cf` again, the same day, no antenna, after `@@Aa` had set the
queries' clock and the restore's `@@Cg` 1:

| | `@@Ea` poll | `@@Aa`/`@@Ac` queries |
|---|---|---|
| Before `@@Cf` | advancing with the queries' clock, 0 visible, 0 tracked, status 0x09 | advancing |
| After `@@Cf` | the time at `@@Cf`, date kept, not advancing, status 0x01 | advancing, not changed by `@@Cf` |

No output unasked after `@@Cf`.  So `@@Cf` stops `@@Ea`'s time where it
is; the first run's 01-01 12:00:00 came with the queries' clock out of
range (hour 64).  The saved state was restored and read back as saved.

## Against a CNS Clock II

A CNS Clock II, serial 404442: revision M by the manual's serial ranges
(CNS Clock II manual, 2024, p. 124; revision L is 404399 to 404425, M
404426 on).  Its `@@Cj` (read 2026-11-19): "Copyright 2021 Synergy Sys. LLC", "SFTW
P/N # 160S1287-SW", "SOFTWARE VER # 7.03", "SOFTWARE REV # FW V3.54",
"SOFTWARE DATE 15-Feb-22", "MODEL # SSR-M8F+ X", "HWDR P/N #
16884483G-1", "SERIAL # 40373400933", "MANUFACTUR DATE 1871": a u-blox
M8F behind Synergy's Motorola-protocol firmware.
From revision L the 1 PPS comes from an OCXO the receiver steers
through a DAC, not from the receiver's pulse through a delay line
(Pazamickas and Hambly, "High-accuracy Time and Frequency in VLBI",
IVS TOW 2023, `third_party/cns/`, slides 51-52); its `@@Hn` sawtooth reads 0.  Its serial port is the USB
one, 9600 baud, Motorola binary.  On the same splitter as the board,
its antenna cable 150 to 180 mm shorter (0.76 to 0.91 ns at a velocity
factor of 0.66); the other receivers' cables from the splitter, and
the counter cables, are of one length.

| Check | Result |
| ----- | ------ |
| CNS on its own antenna, the board on the splitter | the CNS's pulse 67.4 ns after the board's, 3.1 ns sd with the board's sawtooth out |
| On the splitter, the CNS navigating in 3-D (`@@Gd` 0) | 13.1 ns after, 6.8 ns sd |
| On the splitter, the CNS in position hold on the board's hold position (`@@As`, `@@Gd` 1; ellipsoid height from the board's geoid separation, -30.04 m) | 9.20 ns after, 591 readings; 1.45 ns sd with the board's sawtooth out (r -0.97) |

Both counter levels were 1.0 V; the CNS's edge rise time was not
recorded.  No delay was entered (`@@Az` and `@@Ay` read 0).

Its settings changed on 2026-11-09:

| Time | Setting |
| ---- | ------- |
| 12:44 | found navigating in 3-D with SBAS corrections (GPSTime 1.1.1, receive only); when its hold was lost is not known |
| 12:47 | GPSTime "Precision Timing": 0-D (position hold), GPS only, all in view, highest in the sky, TDOP, iono and tropo on, mask 5 degrees, static, 1 PPS of 200 ms |
| 12:47 | GPSTime's default internal receiver delay, 55 ns on a new install, sent as its "UTC Correction" |
| 12:53 | internal receiver delay 48 ns (the manual's revision M figure); antenna and measurement cable delays 0 |

### Three receivers on one antenna

The board's LEA-M8T on a jig, an eight-channel VP on a jig and the CNS,
all in position hold at the CNS's hold position (the board by CFG-TMODE2,
ellipsoid height; the VP by `@@As` type 0 and `@@At` 1), on the splitter
with antenna cables of one length and coax.  The CNS's rear 1 PPS is
teed into both counters' channel 1 (equal legs), then to the scope;
the LEA-M8T's 1 PPS to the 53230A's channel 2, the VP's to the 53132A's,
each then to the scope on cables of one length.  Inputs 1 MOhm, DC; both
starts at 1.88 V, the stops at 1.16 V (LEA-M8T) and 1.21 V (VP), each
edge's 50% point.  The CNS at `@@Ay` 999 999 000 ns (its pulse 1 us early,
so no receiver leads it) and `@@Az` 48 ns.  `bench/pps_compare.py log`
with `--module` (the LEA-M8T's TIM-TP over its USB) and `--vp-message
En`, then `series` and `stats`; 3600 seconds without a gap, 2026-11-18.

| Quantity | Result |
| -------- | ------ |
| LEA-M8T - VP, sawtooths out | +79.9 ns median, 3.1 ns sd; 15-minute means 78.3 to 81.4 ns |
| The same on the scope, raw, 60 shots | 77.3 +- 3.8 ns |
| LEA-M8T - CNS, less the 1 us | +31.5 ns (sawtooth out; r -0.98 with -qErr) |
| VP - CNS, less the 1 us | -48.4 ns (sawtooth out; r +0.99, its pulse late by its sawtooth) |

The CNS's `@@Az` stepped 48 -> 0 -> 48 ns (GPSTime closed, the receiver
read directly, 2026-11-19 03:07:20Z and 03:12:20Z): VP - CNS, sawtooth
out, 956.8 ns before, 910.2 ns at 0 and 957.0 ns after (5-minute
medians, the first 40 s after each step left out; sd 3 to 4 ns).  So
the 48 ns moves the CNS's pulse 46.7 ns earlier, as the manual says, and
it was in effect; the move slews over about 30 s, the CNS steering its
OCXO.

The start's tee legs delay the CNS's edge at both counters, so both
figures against the CNS read about 1.5 ns early (estimated from about
300 mm of coax); LEA-M8T - VP is free of them.  Each counter's channel
1 to channel 2 skew is in each figure, not measured.  The 58503A's
B4121P1115 read 82 ns before the board's pulse (above).

| tau, s | LEA-M8T TDEV | VP TDEV | LEA-M8T - VP TDEV |
| ------ | ------------ | ------- | ----------------- |
| 1 | 0.27 ns | 1.67 ns | 1.67 ns |
| 10 | 0.98 ns | 1.55 ns | 1.20 ns |
| 100 | 0.36 ns | 0.62 ns | 0.59 ns |
| 1000 | 0.07 ns | 0.39 ns | 0.39 ns |

Each receiver less the CNS, sawtooth out.  The three-cornered hat
(`stats`) puts the 5 to 20 s bump in the CNS (ADEV about 2.2e-10 from
2 to 10 s, its OCXO steering) and gives the LEA-M8T 0.05 to 0.3 ns TDEV
from 1 to 1000 s, the VP 0.4 to 1.8 ns; the VP's MDEV falls about as
tau^-1.25 from 10 to 1000 s, the LEA-M8T's about as tau^-1.6.

### Delays after each receiver's antenna input

| Path | Delay | Source |
| ---- | ----- | ------ |
| CNS revision M: the receiver, the PPS generator and the drivers | 48 ns, measured by CNS on its test bed | CNS Clock II manual (2024), "Setting the Internal Receiver Delay", p. 124 |
| Board: U3, 74AHCT1G126 | 3.8 ns typical, 5.5 maximum | TI data sheet, 5.6, CL = 15 pF |
| Jig: U4, SN74AHCT1G126 | 3.8 ns typical, 5.5 maximum | TI data sheet, 5.6, CL = 15 pF |
| Jig: SN74ACT244, four sections in parallel | 6.5 ns typical, 9 maximum | TI SCAS517G, 5.6 |

The jig's R3/R4 divider into U4 adds an RC delay, not known.  A delay
left out makes a pulse late: the manual's delays "arrive at the end of
the 1PPS cable LATE", and each one entered moves the pulse early (pp. 31,
52-53), as `@@Az` does on a VP ("output the 1PPS output pulse
earlier in time", VP reference p. 68).  The internal receiver delay is
added to the cable delays as the "UTC Correction ... used by the GPS
receiver".

### Against UTC, estimated

At each receiver's antenna input (the antenna and its cable to the
splitter, common to all, not counted), from the CNS uncorrected (48 ns
after UTC), its shorter cable (0.8 ns), and the drivers' typicals
(14 ns on the jig, U3 3.8 ns):

| Receiver | Against UTC |
| -------- | ----------- |
| Board's LEA-M8T, at the module's pin | about 24 ns late (48 - 9.2 - 0.8 - 14) |
| The board at its edge (after U3) | about 28 ns late |
| The 58503A's B4121P1115, at its pin | about 54 ns early (82 ns before the board's pulse, through the same jig drivers) |
| The 58503A with its VP, rear 1 PPS (`docs/emu-board-bring-up.md`) | about 34 ns early (48 - 81 - 0.8) |
| The 58503A with the board, rear 1 PPS, FFOM 1 | about 51 ns late (48 + 3.3 - 0.8) |

The relative figures are measured to a few ns; the absolute ones rest
on CNS's 48 ns for its revision M and on the drivers' data-sheet
typicals.  The 58503A's rear pulse sits 20 to 27 ns after its
receiver's with either receiver.  The board's U3 can be measured on the
scope from the module's 1 PPS to J1.

### What other sources say

| Receiver | Statement | Source |
| -------- | --------- | ------ |
| CNS Clock II | internal receiver delay by serial range: 8 ns (CNS Clock), 53 ns estimated (II, revisions B to H), 97 ns (I to K), 48 ns (L, M) | CNS Clock II manual (2024), p. 124; Pazamickas and Hambly 2023, slide 54 |
| CNS Clock II, revisions A to K | the receiver's pulse through a programmable delay line, set for the next pulse; a delay line's bias "T(bias) = 10 + (255 * 0.15 / 2) nsec = 29.2 nsec" for 0.15 ns steps | Hambly and Clark, "Low Cost, High Accuracy GPS Timing", IVS TOW 2011, Haystack, slides 28, 32; Pazamickas and Hambly 2023, slide 51 |
| CNS Clock II, revision L | three units against CNS's reference: +3.7, +1.5, -1.1 ns (300 s averages) | Pazamickas and Hambly 2023, slide 62 |
| M12+ V2.0 | four units against USNO's tick: -0.6, -0.2, +5.3, +3.4 ns, "Overall bias +2.0 nS"; "Gold" receiver "average DC offset = -0.6 ns" | Clark and Hambly, "Timing for VLBI", IVS TOW 2003, slide 14; Hambly and Clark 2011, slide 34 |
| M12+ V1.0 and V2.0 | four units against USNO's tick, 2002: firmware V1.0 "each about 84 nanoseconds early", within "a few nanoseconds" of each other; V2.0, Motorola's update from those results: unit A "0.2 nanoseconds early" over 12 days, C and D "late by about 5 nanoseconds" against A, from "a variation in group delay for the M12+ IF filters that came from two different production lots" | Hambly and Clark, "Critical Evaluation of the Motorola M12+ GPS Timing Receiver vs. the Master Clock at the United States Naval Observatory", PTTI 2002 (`third_party/cns/`), pp. 110-111, 113-114 |
| M12+ | raw 1 PPS quantized at twice its 16.367 MHz clock, "30.55n sec pk-pk"; the receiver's sawtooth correction each second leaves "about 5 nanoseconds pk-pk" | Hambly and Clark 2002, p. 111 |
| CNS Clock with an M12+, USNO test bed | a 53132A started by USNO's tick (21.5 ns distribution amplifier, 4.8 ns cable), stopped by the CNS Clock's 1 PPS; antenna to the M12+ 202.4 ns (31.7, 122.2, 12.0, 18.1, 13.5, 4.1, 0.8); after it 17.6 ns to about the CNS Clock's output and 4.6 ns to the counter (spans read off the figure); a 1 us bias added to the M12+; 1198.3 ns in all, taken out of every figure after | Hambly and Clark 2002, p. 113 (Figure 2) |
| Oncore firmware | "ONCORE firmware evolution from 5.x ⇒ 6.x ⇒ 8.x ⇒ 10.x has been accompanied by about 40 nsec of 'DC' timing offsets"; no sign or per-version figure | Clark and Hambly 2003, slide 12; Hambly and Clark 2011, slide 33 |
| Low-cost timing receivers | against a maser, mean time "varied over a range of about 150 nanoseconds"; "This occurred even between various firmware releases from a single manufacturer on the same hardware" | Hambly and Clark 2002, p. 109 |
| M12+, M12M, LEA-6T | against a maser, 2012: the 6T with a Motorola emulator about 50 ns, the 6T native about 80 ns, the M12M about 30 ns before the M12+ in its CNS Clock (read off the plot; the CNS Clock's own delay in the M12+'s figure) | Pazamickas and Hambly 2023, slides 59-60 |
| Motorola M12+ and M12M | "All the Motorola samples (including the 4 receivers in the 2002 test) appear to agree with UTC(USNO) to better than ±10 nsec" | Clark and Hambly, "It's About Time", IVS TOW 2009, Haystack, slide 36 |
| i-Lotus M12M | "show a bias errors up to ~30 nsec" against CNS's reference receiver | Clark and Hambly 2009, slide 36 |
| VP Oncore | 1 PPS "130 ns observed (1σ) with SA on", "<50 ns observed (1σ)" in position hold; no bias figure | VP Oncore Engineering Notes, pp. 2, 7 |
| SmartClock | receiver bias "usually estimated by the GPS designer and proper compensation is provided ... usually less than 20 nanoseconds" | Kusters, "The Global Positioning System and HP SmartClock", HP Journal, December 1996, p. 2 |
| 58503B | "<110 ns with respect to UTC (USNO MC)", 95%, "properly installed, calibrated, and locked to GPS"; testable only "at NIST"; "cable delays, Receiver bias, position" the installer's; "require no calibration" | 58503B Operating and Programming Guide, 097-58503-13 Issue 2, pp. D-23, E-4, E-5 |
| 58503A | `:GPS:REFerence:BIAS`, 0 to 999 ns in the EEPROM's calibration block, added to `:GPS:ADELay` in `@@Az`: it can only move the pulse earlier; in no HP manual | smartclock `docs/firmware/gps.md`, "The cable delay" |
| 58503B and CNS Clock II | 12 days on one antenna: the 58503B about twice as noisy; both with daily periodicities, "averaging for times less than a day or not a multiple of a day is not a good idea" | leapsecond.com/pages/58503-cns2 (2006) |
| LEA/NEO-M8T | time pulse accuracy "≤ 20 ns", clear sky | NEO/LEA-M8T data sheet, UBX-15025193 R10 |
| LEA/NEO-M8T | the RF group delay is the receiver's own ("read-only"); the antenna cable and user delays are the user's, to calibrate "against a reference-timing source" | u-blox 8/M8 Receiver Description, UBX-13003221 R28, pp. 72, 74 |
| Thunderbolt | "UTC 20 nanoseconds (one sigma)"; USNO testing "shows that the PPS is on time compared to UTC/GPS with no cable" | Thunderbolt data sheet; ThunderBolt User Guide v4.0 (35326-30), p. 5-9 |

No source found measures a SmartClock's or a VP Oncore's 1 PPS against
a UTC(k), or calibrates an M8T against one.  This 58503A's
`:GPS:REF:BIAS` and `:GPS:ADEL` read 0.

## Status reports the images read

SSG0220999 on the jig, `@@As` set to the CNS Clock II's position (about
a meter away), each second's `@@En` (VPCommands.pdf p. 140) logged:
byte 19 pulse status, byte 21 solution status, byte 22 Time RAIM
status.

| Test | Date | Pulse | Solution | Time RAIM status |
| ---- | ---- | ----- | -------- | ---------------- |
| Time RAIM off (`@@En` 01 00 0014 01), position hold, 60 s | 2026-11-19 | 1 | 2 | 2 |
| Time RAIM off, hold off, 59 s | 2026-11-19 | 1 | 2 | 2 |
| Time RAIM on, alarm 1000 ns, position hold, 30 s | 2026-11-19 | 1 | 0 | 0 |
| Time RAIM on, position hold, antenna disconnected 233 s | 2026-11-20 | runs | 2 while no satellite was heard, then 0 again | not read |

With Time RAIM off the engine reports both statuses as 2, not what it
could have done; smartclock-gps's Engine sends the same (smartclock
#171).  With the antenna disconnected the 1 PPS ran throughout: the
53132A, CNS front on channel 1 and the VP on channel 2 (readings
include the CNS's 1 us advance), read 855 to 2387 ns (median 1174)
while disconnected, against 895 to 1008 ns (median 952) before and
after (`bench-runs/pps/vp-antenna-off-20261120.*`, private).

`@@Ea`'s fraction of the second (bytes 11 to 14, ns) is not zero: in
position hold, tracking 4 to 6, it read 12,726 to 848,058 ns over a
minute, rising 12.86 us a second; before tracking, 13.05 us a second.
The board sends NAV-PVT's `nano` there, 0 when negative (PLAN.md,
`@@Ba`).

`@@Cf` (set to defaults) answers with an unsolicited `@@Cg` 00: the
engine goes to idle mode, its default, which it keeps in nonvolatile
memory (p. 118).  Idle, it tracks nothing, sends no periodic output and
no 1 PPS, answers a polled `@@Ea` with a frozen time, and comes up idle
after a power cycle; `@@Cg` 01 restores position fix mode.  Each image
that sends `@@Cf` follows it with `@@Cg` 01 (smartclock's
`docs/firmware/gps.md`).

## To check

- SSG0163878: J6, D1, +5V, and whether it tracks.
