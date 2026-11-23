# Emulator board bring-up

Results from the first emulator board 1.0.0, 2026-11-06.  Steps are
`docs/bring-up.md`'s.  Logs stay out of the repository.

## Set-up

- USB-C (J3) from a computer's port, out of any instrument; on a jig 1.0.0
  for step 5, its USB on the same computer.
- SWD on J4 through an STLINK-V3 and a TC2030, `probe-rs` 0.31.0.
- `rtt` build, B1121P1114, from 0d9cd32.
- Antenna on J2 through a Raven Industries LA-21-1575-100-T amplifier
  (20 dB, noise figure under 3 dB; smartclock's
  `docs/hardware-investigations.md`) and a splitter powered from its
  first port, shared with a NEO-M8T.  J2 is on the first port.

## Results

| Step | Check | Result |
|---|---|---|
| 2 | +5V, +3V3, current | not measured |
| 3 | Module over USB | VID 1546, `LEA-M8T-0`, TIM 1.10, protocol 22.00 |
| 3 | Fix | 3D, 10 satellites used |
| 4 | SWD | answers; DEV_ID 0x44D, 256 KB, flash blank before the first load |
| 4 | First boot | `module moved from 9600 baud`, `module set up`, no `no answer` |
| 4 | Second boot | `module at 115200 baud`, `module set up`, nothing else in 75 s |
| 4 | D2 (STAT) | toggles with each D6 pulse; PA5 changes every 1.00 s, sampled over SWD |
| 4 | D6 (PPS) | 1 Hz |
| 5 | `@@Cj` at 9600 on the jig | answers, model B1121P1114, serial `EMU1387684` |
| 5 | `@@Ca` | passes, `0x0000`, 0.1 s |
| 5 | Silent at start, and after `@@Ca` until told | no frames in 12 s and 5 s |
| 5 | Z3801A 3543 start-up (`docs/vp-on-jig.md`) | every setting answers |
| 5 | Frames with bad checksums, 130 s after it | 0 |
| 5 | `@@Ba`, `@@Bn` | 60 a minute |
| 5 | `@@Bb` | every 5.00 s, +/-0.02 s, host arrival |
| 5 | Tracked | 6 of 6 channels, all mode 8, in every `@@Ba` |
| 5 | `@@Ba` time | UTC with `@@Aw` 1; same second as the host's |
| 5 | J9, jig's USB only | 6.3 mV peak, 63 mA, with the LEDs' pulse; includes the splitter, amplifier and antenna, fed from J2 |
| 5 | 1 PPS at J1 pin 6 | not measured |
| 5 | `@@Ay`, qErr's sign, against a VP | not done |
| 6 | In an instrument | not done |

Both USB ports connected for step 5: J9 reads low if J3 shares the load.

Defaults that differ from what the VPs hold: `@@Aj` 1, `@@Aw` 1, `@@AB` 2,
`@@Ah` 0, `@@At` 0, `@@Bn` Time RAIM off.  The start-up as run replays
each engine's stored `@@Aw`, `@@At` and Time RAIM state, so those stayed at
the emulator's defaults.

The first boot also logged 717 framing errors, 1 overrun, 1 noise error
and one `no epoch from the module` in 2 minutes.  The second boot logged
none of them.

## Channels

From the Z3801A 3543 start-up on the jig, listening, 2026-11-07 UTC: no
change in `@@Ba`'s channel list in 1,801 frames, 30 minutes.  In the
start-up's first 130 s it changed once after the first selection: SV 20
for SV 6.

## Six and eight channels

79de110 on the board, through the jig, commands sent by hand:

| Check | Result |
|---|---|
| At power-on | `@@Cj` B1121P1114, software 8.4 |
| `@@Ek`, `@@En`, `@@Fa` before `@@Ea` | no answer |
| `@@Ea` 0 | answers |
| After it | `@@Cj` B4121P1115, software 8.8; `@@Fa` `0x0000` |
| `@@Ea` 1, `@@En` 1, 25 s | 26 of each |
| `@@Ea` | 8 of 8 tracked, all mode 8, within 25 s of the switch |

One frame failed its checksum in that run; which one was not recorded.

## The 58503A 3704 image

The 58503A 3704 image in emulation, with the 58503A 3710A01056's
EEPROM, from power-on, its GPS port on the board on the jig at 9600.  The board's
1 PPS is not tested here.  The OCXO warm-up was skipped.

From power-on, the board read first over the jig: B1121P1114, software
8.4, `@@At` 0.  `@@As` and `@@At` sent once each, tracking 8, "Locked to
GPS: stabilizing frequency" at 142 s, TFOM 4, FFOM 1, TI +39.5 ns just
after lock, Self Test and GPS Rcv OK, `:SYST:ERR?` `+0,"No error"`.

The figures in the table below are from an earlier run whose reset is
not confirmed.

| Check | Result |
|---|---|
| Order from the image | `@@Ea`, `@@Fa`, `@@Cg`, `@@Ea`, `@@Cg`, `@@Cj` |
| `@@Ca` | not sent |
| `@@Ar` | once, argument 1; no 65 |
| `@@Ea`, `@@Ek`, `@@En` | sent |
| `:SYST:STAT?` tracking | 8 |
| Mode | "Locked to GPS: stabilizing frequency" at 258 s; survey to hold |
| TFOM, FFOM | 4, 1 at lock |
| 1PPS TI | +21.1 ns, just after lock |
| Self Test, GPS Rcv | OK, OK |
| `:SYST:ERR?` | `+0,"No error"` |

## The Z3801A and Z3805A images, from power-on

The same set-up, the board read first as above.  Warm-up skipped.

| Check | Z3801A 3543 | Z3805A 3543B |
|---|---|---|
| `@@Ea`, `@@Ek`, `@@En`, `@@Fa` | none | none |
| `@@Ca`, `@@As`, `@@At`, `@@Bn` | once each | once each |
| `@@Ar` | once, argument 1 | once, argument 1 |
| Tracking | 6 | 6 |
| "Locked to GPS: stabilizing frequency" | 346 s; 312 s in an earlier run | 312 s |
| TFOM, FFOM | 4, 1 | 4, 1 |
| 1PPS TI | +65.1 ns | +39.4 ns |
| Alarm, GPS lock LEDs | off, on | off, on |
| Self Test, GPS Rcv | OK, OK | OK, OK |
| `:SYST:ERR?` | `+0,"No error"` | `+0,"No error"` |

Z3805A diagnostics, locked: `:DIAG:TEST:RES?` `+0,ALL`, `:SYNC:STAT?`
`LOCK`, TFOM 3, FFOM 1, `:SYNC:TINT?` -6.8 ns, 6 tracked of 9 predicted,
hardware register 0, `:STAT:QUES:COND?` 0, its event register 1 from
power-up, no alarm LED.

The board afterward, read over the jig: B1121P1114, `@@At` 1, `@@Aw` 0,
`@@Ag` 10; `@@Ba`, `@@Bn` 1 a second, `@@Bb` every 5 s; `@@Ba` status
0x08, 6 tracked, all mode 8; Time RAIM on, limit 10, 1 PPS mode 1.

Holdover on this set-up means pulling the board's antenna.

## Idle and the module's save

c43fee7 on a second board (`EMU6050839`), with Idle after `@@Ca`, `@@Fa` and
`@@Cf` (VPCommands.pdf p. 118).  Warm-up skipped.  1 PPS as above.

| Check | Z3801A 3543 | 58503A 3704 |
|---|---|---|
| Power-on to lock | 175 s | 198 s |
| `*TST?` while locked | `+0`; `@@Ca`, `@@Cg` 1, then the start-up | `+0`; `@@Ea`, `@@Fa`, `@@Cg` 1, `@@Ea` 1, `@@Cg` 0 |
| Relocked after it | 173 s | 141 s |
| Self Test, GPS Rcv | OK, OK | OK, OK |
| `:SYST:ERR?` | `+0,"No error"` | `+0,"No error"` |

The Z3801A's figures are on 6b25347, which has Idle but not the save
change.

Module read back over its USB after a boot at 115200: CFG-ANT flags 0x001b,
UART1 115200 and UBX only, the 11 messages at rate 1 on UART1, CFG-TP5 1 s,
200 ms, flags 0xf7, CFG-NAV5 minimum elevation 10, CFG-TMODE2 mode 0 before
an instrument and mode 2 after the Z3801A's `@@At` 1.

The first board's module refused set-up step 13 (CFG-ANT) on boots of
6d5120f and fe66e14, with or without an instrument's run first.  Read
over its USB: CFG-ANT flags 0x0000 and pins 0x0000, the antenna
supervisor off, MON-HW's antenna status and power unknown; the step
asked for supply control with the pins kept, on PIO0.  How the pins were
lost is not known.  The step now sends TIM 1.1x's default pins with
`reconfig` (0xb9f0): on the next boot no refusal, CFG-ANT 0x001b and
0xb9f0, MON-HW antenna OK and powered.

SS against the module: on the 20 dB feed, C/N0 less 20 log10(SS), against a
NEO-M8T on the same splitter, is 7.9 dB for SSG0178541 and 10.7 for
SSG0163878, within 0.2 dB across satellites.

## 58503B 1.01.04

The 58503B 1.01.04 image in emulation, with an erased EEPROM, on the second board with the production build of d81fd8b, which answers `@@Ta`.
Warm-up skipped.  1 PPS as above.

| Check | Result |
|---|---|
| Order from the image | `@@Ea`, `@@Cj`, `@@Fa`, `@@Cg`, `@@Ea`, `@@Cj`, `@@Ta`, `@@Cg`, then the settings |
| `@@Ta` | answered with zeros; polled about every 31 s after |
| `@@Ar` | once, argument 1; no 65 |
| Power-on to lock | 149 s, tracking 8 |
| Self Test, GPS Rcv | Err, OK: Err from the erased EEPROM, whatever the receiver; alarm LED on |
| Locked from power-on | 2.7 h, until `*TST?` |
| `*TST?` while locked | relocked 146 s |
| GPS week boundary, 2026-11-08 00:00 UTC | locked through it; date 2026-11-08 |
| `:SYST:ERR?` | `+0,"No error"` throughout |

## Z3816A 4001

The Z3816A 4001 image in emulation, with a synthetic EEPROM and its time code off, on
the second board with the production build of 02f0c2b.  Warm-up skipped.  1 PPS as above.

| Check | Result |
|---|---|
| Order from the image | `@@Ea`, `@@Cj`, `@@Fa`, `@@Cg`, `@@Ea`, `@@Ta`, `@@Cj`, `@@Ta`, `@@Cg`, then the settings |
| `@@Ta` | answered with zeros; polled after |
| `@@Ar` | once a start-up, argument 1; no 65 |
| Power-on to lock | 141 s, tracking 8, through coarse and fine frequency adjust and phase alignment |
| Self Test, GPS Rcv | OK, OK |
| `*TST?` while locked | relocked 185 s, through leap second determination |
| `:SYST:ERR?`, alarm LED | `+0,"No error"`, off |

## Every image, 2026-11-16

Every image in emulation, on each build it can meet, on the second
board on the jig
(the jig's LEA-M8T, live sky), 095ad35 with smartclock-gps cb968ece.
Each cell: the build flashed over SWD, the image from power-on with its
warm-up skipped and a fresh copy of its EEPROM, lock, 15 minutes locked
with `:SYST:STAT?` read each minute, `*TST?`, relock.  The Z3816A with
its time code off; the Z3817A with a 592 s oven warm-up.

| Image | Build | Lock | Tracking | Relock after `*TST?` |
|---|---|---|---|---|
| Z3801A 3543 | VP | 298 s | 6 | 184 s |
| Z3805A 3543B | VP | 266 s | 6 | 191 s |
| 58503A 3704 | VP | 141 s | 6 | 153 s |
| 58503A 3704 | `vp-six` | 153 s | 6 | 169 s |
| 58503B 1.01.04 | VP | 140 s | 7 | 137 s |
| 58503B 1.01.04 | `vp-six` | 152 s | 6 | 151 s |
| 58503B 1.01.04 | `ut-plus` | 129 s | 8 | 136 s |
| Z3816A 4001 | VP | 144 s | 8 | 157 s |
| Z3816A 4001 | `vp-six` | 156 s | 6 | 171 s |
| Z3816A 4001 | `ut-plus` | 138 s | 8 | 147 s |
| Z3817A 3752 | `z3817a` | 862 s | 8 | 627 s |

In every cell: Self Test all OK, `*TST?` +0, `:SYST:ERR?` `+0,"No
error"` before lock, after the locked stretch and after relock, no
alarm or Time RAIM line and no lock lost while locked, TFOM 4 and FFOM
1 at the end.  The 58503B and Z3816A surveys advanced about 0.8 % a
minute.  The Z3817A locked to TIMEREF2; after `*TST?` it read
"TIMEREF1 invalid" for about 400 s before relocking on TIMEREF2, where
at power-on it went to TIMEREF2 at once.

## Z3801A and Z3805A on the first board

The first board, 6d5120f, two runs of the Z3801A, the images in
emulation.  Warm-up skipped.

| Check | Z3801A 3543 | Z3805A 3543B |
|---|---|---|
| Power-on to lock | 276 s; 343 s in the second run | 342 s |
| Locked from power-on | 7 h, in the second run | not yet |
| `*TST?` while locked | not run in the first run | relocked 209 s |
| `:SYST:ERR?`, alarm LED | `+0,"No error"`, off | the same |

## Production build and the stack

The production build with `-F cortex-m-rt/paint-stack`, 0327738, on the second
board, the first on hardware: the 58503A 3704 locked at 142 s, tracking 8, alarm
LED off, `@@Ar` 1 only.  The stack's peak through start-up to lock, both ports
active: 3.28 KB.  The `rtt` build's, the same run: 3.37 KB.

The production build at 5a3f8b1 (smartclock-gps-ublox), `llvm-size`: flash
103.2 KB of 256 KB (text 92,920 B, data 12,792 B), static RAM 16.9 KB of 36 KB
(data 12,792 B, bss 4,468 B).  Its stack's peak is not measured yet.

## Time RAIM against a VP

`@@Bn` and `@@En` once locked, from the logs, against SSG0178541's and
SSG0163878's `@@Bn` on the same feed:

| Field | Board, Z3801A and 58503A | VPs |
|---|---|---|
| Solution, capability | 0, 0 | 0, 0 once locked; 2, 2 before |
| One sigma | median 2 ns, 8 at most | median 1 ns |
| Sawtooth | -10 to 11 ns | -52 to 52 ns |
| Each channel's time estimate, spread in a frame | about 15 ns | about 130 ns |
| Each channel's time estimate, range | about -0.5 to +0.5 ms, modulo 1 s | 0 to 1 ms |

The time estimate's range differs: the module's fraction is NAV-PVT's
`nano`, which goes negative.  The images only display it (smartclock's
`docs/firmware/gps.md`).

## The board left powered between runs

Z3805A 3543B started on a board left at `@@At` 1, with the Z3801A's
position and at eight channels: `@@As` sent 220 times, no `@@At`, no
`@@Bn`; GPS Rcv Err, alarm lit, "1PPS CLK Invalid: inacc position".  The
board takes `@@As` only with position hold off (VPCommands.pdf p. 54) and
answered with the Z3801A's position; the image wants its own within 2 cm
(smartclock's `docs/firmware/gps.md`).  From power-on the same image sends
`@@As` once.  So a board on a bench is power-cycled between instruments;
in an instrument it powers up with it.

## Signal

Module and NEO-M8T on the same splitter, the splitter's 5 V supply off,
then on:

| | Supply off | Supply on |
|---|---|---|
| AGC (MON-HW) | 5304 | 3744 |
| Jamming indicator | about 77 | 28 |
| Satellites heard | 0 | 15 |
| Highest C/N0, dB-Hz | 0 | 46 |

The NEO-M8T's highest C/N0 with the supply on was 44.

The module's time pulse, D6 and D2 ran with no fix and no signal.

## In the 58503A

The board in the 58503A (3704-C) in place of its B4121P1115, with the
same antenna cable and the same rear 1 PPS cable to the counter's
channel 2; J3 (USB) unplugged.  The reference is the CNS Clock II on
channel 1, in position hold on the same splitter, its mode and delays
read first (`docs/vp-on-jig.md`, "Against a CNS Clock II").  The
counter is set with `bench/tic_setup.py` (each edge's midpoint) and
checked against the scope; the 58503A's own `:SYST:STAT?` is logged
beside it (the `pps-tic-measurement` skill).

1. Read `:GPS:REF:BIAS?` and `:GPS:ADEL?`: the 58503A sends their sum
   as `@@Az`, which moves the board's pulse earlier.
2. Before: the 58503A on its own engine against the CNS, a whole day
   (both drift with daily periods, leapsecond.com/pages/58503-cns2).
3. The board fitted, from power-up through "Locked to GPS", then left
   running a whole day.  From a cold start the lock takes about 2.5
   hours (the start-up loop waits for the oscillator-aging fit's first
   three 2,700 s samples, smartclock `docs/firmware/loop.md`); with the
   oven still warm, minutes.

The offset is judged on the counter less the 58503A's own "1PPS TI",
paired at each poll: that takes out its loop's wander, which a mean of
the counter alone keeps (`docs/vp-on-jig.md`, "The 58503A against the
board").

It passes if:

- it reaches "Locked to GPS" within 4 hours of power-up;
- no alarm, `:SYST:ERR?` entry, or Self Test or GPS Rcv fault once
  out of the power-up stages;
- no holdover once locked;
- the counter less its own reading moves about 82 ns later than in
  step 2 (the VP's offset from the board), to within 30 ns;
- its whole-day scatter against the CNS is no worse than in step 2.

### 2026-11-08 to 11-09

Counter levels: CNS 1.89 V (2.4 ns 10-90% rise), 58503A 1.435 V
(19.6 ns); the scope read the 58503A's edge 98 to 100 ns before the
CNS's at the start, the counter 99 to 101.5 ns.  `:GPS:REF:BIAS` and
`:GPS:ADEL` 0.  The CNS was later found navigating, not holding
(`docs/vp-on-jig.md`); its settings were not changed until 12:47 on
11-09, after the results below.

| Check | Result |
| ----- | ------ |
| Before: the 58503A with its VP against the CNS, 22:21 to 08:21, 18,001 readings | 81.1 ns before (median 79.4), 19.9 ns sd; hourly means -114 to -60 |
| Before: the counter less its own reading, 291 polls | 80.0 ns before, 7.8 ns sd |
| Power-up 09:13, oven warm (a few minutes off) | coarse frequency adjustment with the health monitor's EFC "Err" and the panel's "EFC near limit", 09:18 to 09:22; fine adjustment, EFC OK, 09:24 |
| Lock | "Locked to GPS: stabilizing frequency" at 09:26, 13 minutes; FFOM 0 by 12:22 |
| Tracking | 7 to 8 satellites, every GPS satellite above 10 degrees that apu-0's NEO-M8T hears; signal levels 1 to 2 dB above apu-0's C/N0 (its RG-174) |
| After, FFOM 1, 40 minutes after lock to 12:47 | 3.3 ns after the CNS, 4.5 ns sd |
| After: the counter less its own reading, 54 polls | 2.8 ns after, 3.8 ns sd |
| The move | 82.8 ns later (82.0 ns on the jig) |
| At FFOM 0 | its own reading out to +23.6 ns and back in 30 minutes; the counter with it |

Out of the power-up stages, no alarm, error or holdover through 14:05.
The whole day after the swap, against the CNS as now set, is yet to
judge.

### 2026-11-22, fw-v1.0.0

Board 0001 with fw-v1.0.0 (fe3802acd01d, `vp`), the release image
from CI, in the 58503A.  The CNS at `@@Ay` 999 999 000 ns (its
pulse 1 us early) and `@@Az` 48 ns.  The 53230A: CNS rear on channel
1 at 1.88 V, the 58503A on channel 2 at 1.43 V, 1 MOhm, DC.  The
58503A's status read once a second by its own monitor, not by the
test.  Beside it, board 0005 with the same image on jig 0209 in
position hold at the CNS's surveyed position (TMODE2 fixed, within
1 cm), against the CNS's front 1 PPS on the 53132A: CNS 1.81 V, jig
1.27 V, 1 MOhm, DC; scope CNS 2.4 ns and jig 3.6 ns 10-90% rise.  The
LEA-M8T's TIM-TP polled over J3 for qErr; nothing sent to either
board.

| Check | Result |
| ----- | ------ |
| Lock | "Locked" at 02:31; FFOM 0 from 05:12 |
| Locked, 02:31 to 18:19 | no alarm, holdover or logged error; tracking 6 to 8 |
| The 58503A's own TI, hourly means, 03:00 to 12:00 | -94 to +53 ns: a damped swing after pull-in |
| Its own TI, 13:00 to 18:19 | hourly means +10 to +23 ns; -14 to +38 ns |
| EFC | 40.1 % at lock to 36.0 %, 0.5 %/h falling to 0.13 %/h |
| The counter less 1 us less its own reading, hourly | 46.0 to 48.0 ns medians, 1.6 to 2.8 ns sd (about 47 ns with board 0005, 11-19) |
| Board 0005 on the jig less the CNS, qErr out, 52,279 s | 1034.4 ns median, 1.40 ns spread (6.18 raw); r -0.974 with qErr |

Stability, 14:05 to 20:05 (21,600 s without a gap, the 58503A's loop
settled; `pps_compare.py series` and `stats`, tau0 = 1 s), each pulse
less the CNS (charts and tables:
<https://claude.ai/artifact/BKLhqv7PK5DPNSiXxLh1z2>):

| tau, s | 0005 on the jig, qErr out, TDEV | 58503A with 0001 TDEV | 0005 - 58503A TDEV |
| ------ | ------------------------------- | --------------------- | ------------------ |
| 1 | 1.26 ns | 0.15 ns | 1.25 ns |
| 10 | 1.00 ns | 0.89 ns | 0.44 ns |
| 100 | 0.42 ns | 0.62 ns | 0.63 ns |
| 1000 | 0.16 ns | 4.58 ns | 4.65 ns |

The three-cornered hat puts the 5 to 20 s bump in the CNS again (ADEV
1.7e-10 to 2.1e-10 from 2 to 10 s), and gives the 58503A ADEV 1.0e-10
at 1 s and 9.2e-12 at 1000 s, its TDEV 0.55 ns at 100 s and 4.6 ns at
1000 s: its loop's steering.  0005's 1.26 ns at 1 s includes the
53132A's noise, not separated (the LEA-M8T on the 53230A: 0.27 ns,
`docs/vp-on-jig.md`).  The settling segment, 03:48 to 14:05 (37,009 s),
gives the same to within 0.25 ns up to 200 s; the 58503A 2.89 ns at
500 s and 7.48 ns at 1000 s.

### Bias against UTC

The 58503A's rear 1 PPS, estimated against UTC from the CNS Clock II
(`docs/vp-on-jig.md`, "Against UTC, estimated"): about 34 ns early with
its VP Oncore, about 51 ns late with the board.  No image corrects
either: `:DIAG:TOFF` reads 0, and the board adds no offset of its own.
Each instrument's `:DIAGnostic:TOFFset` (every image, smartclock
`docs/scpi/`; in the EEPROM's calibration block, +-300 ns, either way)
trims it against a reference.

## To check

- Step 2's +5V, +3V3 and current.
- Which set-up steps the module ACKs: the `rtt` build logs only the
  result of the whole set-up.
- Step 5's J9, 1 PPS, `@@Ay` direction and qErr sign.
- Step 6.
- The 58503A with the board against a second reference (a Thunderbolt)
  on the jig's splitter port.
- 0005's 1 s noise on the 53230A, to separate the 53132A's.
