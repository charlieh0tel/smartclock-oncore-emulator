# To do

## Firmware

- The board's own 1 PPS delay, TIMEPULSE through U3 and R3 to the
  connectors' center pins (the traces matched in time), taken out in
  CFG-TP5's user delay as a named constant, never seen in an Oncore
  message; `@@Az` applied on top and echoed as sent.  Measure it on the
  scope from the module's 1 PPS to a connector into a fixed load first
  (U3, a 74AHCT1G126: 3.8 ns typical, 5.5 maximum).  After PR #3.
- Fitting a board in a 58503A: its `:GPS:REF:BIAS` is added to `@@Az`,
  so the board's pulse moves earlier by it; read it first
  (`docs/emu-board-bring-up.md`, "In the 58503A").
- `oncore-fw/examples/timing.rs` on a NUCLEO-C092RC: it runs the
  Emulator, smartclock-gps's engine; PLAN.md's figures (2026-10-30)
  are of the engine it replaced.
- An `async` set-up (the port split, ACKs through a `Signal`) only if
  `oncore-app`'s `Setup` keeps growing.
- Run `oncore-host` against a Z3801A or Z3805A on J1's lines.  Against
  apu-0's NEO-M8T through gpsd (2026-10-28) `Ba`, `Bb`, channel
  selection, `Bn` (with TIM-TP's sawtooth), `Bk`, `Bj` and `Ca` work,
  with RXM-RAWX, TIM-TP, NAV-CLOCK, NAV-TIMELS, MON-HW and RXM-SFRBX
  enabled on its USB port in RAM (ubxtool, CFG-MSG; a power cycle
  undoes it).
- `@@Bb`'s cadence against a real VP, once the jig is built: a
  timestamped capture of a VP's line in mode 1, off the jig's tap
  header (J6).  The emulator sends it
  at each satellite alert computation, every 5 s; the reference gives
  no period.  Measured (`docs/vp-on-jig.md`): 6 to 7 s with 6 satellites
  tracked, 2 to 4 s with none.  The emulator's 5 s is unchanged.
- `@@Zr`'s reset: passed on a board (`docs/emu-board-bring-up.md`).
- Whether V_ANT stays on with CFG-ANT's supply control off, as on a
  module with lost pins before the set-up repairs them: the board as the
  splitter's only supply, tracking or not.
- The 58503B 1.01.04 and Z3816A 4001 images: passed from power-on on
  the board, each sending `@@Ta` (answered with zeros) and `@@Ar` 1
  once, no 65 (`docs/emu-board-bring-up.md`); both on their UT Plus
  path with the `ut-plus` build (`docs/emu-board-bring-up.md`, "Every
  image, 2026-11-16").
  The Z3815A 4010 uses a Furuno GT-74 on NMEA, not an Oncore: out of
  scope.
- The switch to eight channels on `@@Ea`, against the images: passed from power-on on the board, 58503A 3704
  at 8 with its master reset, the Z3801A and Z3805A at 6
  (`docs/emu-board-bring-up.md`).  The 58503A 3633 not run.
- A conformance test against the images: first `oncore-host --oncore
  tcp:` (the protocol, init and `:SYSTem:STATus?`'s `Tracking: N`; no
  1 PPS over TCP), passed from boot on 2026-10-30 by the Z3805A 3543B,
  Z3801A 3543 (`Tracking: 6`) and 58503A 3704 (the B4121P1115 then
  chosen by `--model`, since replaced by the switch on `@@Ea`,
  `Tracking: 8`) images, each with GPS Rcv OK (the 58503A 3633 is out
  of scope).  Then apu-0's NEO-M8T through gpsd brought the Z3801A 3543
  and 58503A 3704 images to "Locked to GPS" within 15 minutes
  (2026-10-31), and the STM32 firmware on a NUCLEO-C092RC, the Z3801A
  3543 likewise; TI within a few ns at 45 minutes.  Holdover: with the
  module's stream paused, the Z3801A went to holdover within 2 minutes
  (2026-10-31).  Recovery and a reset mid-run are for the board, whose
  1 PPS comes from the module's time pulse.

## UT Plus

A third engine model, the UT Plus Oncore, for the 58503B 1.01.04 and
the Z3816A 4001, the only images with a UT Plus path (smartclock's
reading: they take it when `@@Cj`'s model line has an `R` as its 12th
character, and at software 3.0 or later they also send `@@Am` and query
`@@Gj`).  Both run on the VP path already.  Chosen at build time on the
board (a `ut-plus` feature) and by `oncore-host --model ut-plus`.

- The model: R5122U1152, the one UT Plus model known from a shipped
  58503B, at software 3.1, the owner's choice (no 3.x identity is
  known; the identity's other lines are placeholders, as nothing in the
  images reads them).  8 channels.
- Its 29 commands (UT Plus engineering note p. 11) and their lengths
  from TRM0003 chapter 6; `@@Am` from the VP reference, `@@Gj` from the
  M12+ User's Guide pp. 152-153.  Tested against the 58503B's and
  Z3816A's UT Plus table entries, as the VP's are against the images'.
- The engine: done (`docs/oncore-messages.md`): `@@Bo`, `@@Gj`, the UT
  Plus's `@@Ea`, `@@Fa` and `@@En` (ten bytes not used, 0, for the VP's
  pulse rate and next-pulse time; TRM0003 pp. 6.93-6.94); `@@Sz` never sent; the automatic survey
  (`@@At` 2) not emulated, which the 58503B and Z3816A never ask for.
  `@@Aq` 0 to 3.  Left: `@@Af`/`@@As` ellipsoid heights only, `@@Ao`
  datums 49 and 50 only (the images send `@@Ao` 49 and no `@@Af`/`@@As`).
- Open: `@@Gj` and `@@Am` on a 3.x UT Plus are the M12+'s and VP's
  formats by inference; `@@Bb` with more than six satellites (a Synergy
  note says the UT Plus sends it several times); whether `@@Ea`'s
  signal byte is C/N0 itself.
- A leap second scheduled in the engine's reports, against the images, 2026-11-12:
  the 58503B and Z3816A on both paths, the 58503A 3704, Z3801A and
  Z3805A.  Every image reads `@@Bj` alone and books the leap second at
  the end of the current quarter, whatever the day.  With
  `2026-12-31,19`, each reads `:PTIM:LEAP:ACC?` +18, `:STAT?` 1,
  `:DATE?` 2026,12,31, `:DUR?` +61, `:PTIM:GPST?` correct, and shows
  "+1 leap second pending"; with `2026-12-31,17`, `:DUR?` +59 and
  "-1 leap second pending".  Across `2026-11-12,19`, the 58503B (UT
  Plus path) never showed the engine's 23:59:60: it ran a second ahead
  until about 00:00:15, then `:ACC?` read +19, the leap second still
  pending for 2026-12-31; the Z3816A (VP path, GPS time) did not step.
- The reported time started at 2026-12-31T23:50:00, a leap second
  scheduled for 2026-12-31 (19), the same seven runs, each twice: every image stayed
  Locked to GPS across the leap second, 1 PPS TI with no step, nothing
  logged, `:STAT:QUES?` +0 throughout.  After it `:PTIM:LEAP:ACC?` +19
  and `:STAT?` 0 within 45 s; `:DATE?`, `:DUR?` and `:PTIM:GPST?`
  answer -230 with nothing pending.  The 58503B (UT Plus path) read
  `+23,+59,+60` on `:PTIM:TIME?` and asked for `@@Bo` again about 165 s
  later; the 58503A and the 58503B (VP path) stepped a second (not
  sampled at :60).  The GPS-time images ran on unbroken.  `@@Bj` is
  polled once, about 2 min after start, never again.  The Z3801A and
  Z3805A screens still showed "+1 leap second pending" 10 min after it.
  The screen shows the image's raw `@@Bj` byte, written only by its
  `@@Bj` decoder; the image asks again 65536 s (about 18.2 h) after its
  last good answer, on entering power-up recovery, or on the console's
  `leapsec`; `:STAT?` clears at the end of the leap minute (smartclock's
  reading of the 3543 and 3543B).  Open: what a real Oncore answers to
  `@@Bj` after the leap second, which decides whether the screen clears.
- Backlog: an `oncore-host` test hook that shuffles the engine's outputs
  within a second, seeded, to check the images take any order; a real
  Oncore's order is not known and may vary.
- `@@Gj`: the images query it only from the pForth console
  (`gps_query_all`, `55 gps_query`; smartclock's reading); both took the
  21-byte reply, no leap scheduled.
- Tested against the images (oncore-host, a gpsd source, 2026-11-12):
  the 58503B on the UT Plus path locked at 134 s, on the VP path at
  129 s; the Z3816A at 138 s and 144 s; every command answered.  Next:
  the board's `ut-plus` build on the jig.

## Board update: U2 is the STM32C091FCP6

**Do not re-order the 1.0.0 BOM as it is.**  `production/oncore-emulator/20261026-1.0.0/`
lists U2 as the STM32C011F6P6 (C5456198).  On the 1.0.0 order U2 was
swapped, before DFM, for an STM32C091FCP6 (C45338507) from our own
JLCPCB stock: 256 KB flash and 36 KB RAM against 32 KB and 6 KB, which
the firmware needs.

- Done 2026-10-28: U2 is the project library's `STM32C091FCPx`,
  STM32C091FCP6 and C45338507, in the schematic and board, with the
  notes and title blocks; ERC and DRC with schematic parity clean.
- At the next order: C45338507's rotation from JLCPCB's placement
  preview (ee_tooling's `parts.toml`, `[unchecked]`).
- Done 2026-10-29: J2's ground holes are 1.5 mm drills, from 1.4
  (`oncore-emulator.3dshapes/README.md`).
- The next release, with a new REV, only if the 1.0.0 boards show a
  major issue.
- The repository's emulator, REV 1.1.0, not ordered, is brought to
  ee_tooling's house style: the schematic on the 2.54 mm grid (JP1 from the
  project library, its pin 2 on the grid); net classes (`90Z_DIFF`,
  the power nets), the physical stackup, the In2 zone locked; GND vias, edge stitching and silkscreen.
  Waived: the USB pair against `90Z_DIFF` (PLAN.md) and stub
  direction; in `hardware/oncore-emulator/kicad-style.toml`, U4's
  supply order (USB's feed reaches VIN past R14 and R16, J1's goes
  through C1) and the mounting holes on no net.  The stackup table
  is on User.Drawings.
- Next board: J4 on the TC2030 footprint with retention holes, for the
  legged cable, in place of `Tag-Connect_TC2030-IDC-NL`.  Check the holes
  clear what is around J4.
- Next board: consider a DIP switch or solder jumpers on spare GPIOs
  for the model (VP, six-channel VP, Z3817A, UT Plus), read at power-up
  in place of the build features (`make flash-ut-plus`,
  `flash-vp-six`, `flash-z3817a`).  Check which of U2's pins are free.

TSSOP20 parts that fit U2's footprint and pin use, with JLCPCB stock on
2026-10-27.  The STM32C09x pinout is from DS14720 (one figure for the
C091 and C092), the C071's from DS14693; all three are in
`third_party/st/`.

| Part | Flash / RAM | JLCPCB | Stock | Notes |
|---|---|---|---|---|
| STM32C091FCP6 | 256 KB / 36 KB | C45338507 | 24 | chosen |
| STM32C092FCP6 | 256 KB / 30 KB | C44847704 | 11 | C091 with FDCAN, unused |
| STM32C092FBP6 | 128 KB / 30 KB | C44521813 | 25 | |
| STM32C091FBP6 | 128 KB / 36 KB | C43080053 | 0 | |
| STM32C071FBP6 | 128 KB / 24 KB | C41835018 | 0 | the `FxP` pinout; its bootloader not checked |
| STM32C071F8P6 | 64 KB / 24 KB | C43650205 | 23 | the `FxP` pinout; its bootloader not checked |
| STM32C051F8P6 | 64 KB / 12 KB | C45338499 | 0 | pinout not checked |
| STM32C051F6P6 | 32 KB / 12 KB | C45066857 | 0 | pinout not checked |
| STM32C011F6P6 | 32 KB / 6 KB | C5456198 | 1043 | the 1.0.0 BOM's; too small |

Approved for U2: the STM32C091 and STM32C092, FB or FC (the C092's
SRAM is 30 KB, DS14720 3.4; the firmware uses 19.1 KB, PLAN.md).  Not the
STM32C071FBP6N (C42321837): its TSSOP20 pinout differs.  The
STM32C09x's ROM bootloader also listens on USART2 (AN2606).

- The module's 1 PPS with no antenna and no fix (seen 2026-11-15 on a
  board in the jig): check what the set-up's CFG-TP5 flags give
  without a fix, and that no pulse goes out before one.

## Check on a module

Checked 2026-10-30 on apu-0's NEO-M8T (TIM 1.10, the board module's
firmware), in RAM: the emulator's CFG-TP5 (user delay -999,999,999 ns,
flags 0xF7, 200 ms) is ACKed and reads back unchanged, so the module
takes a user delay near -1 s and the flag bits are where the
receiver description's bitfield figure puts them (UBX-13003221 R28, p. 273).

- After the refactor: `@@Ba` from an 8-channel engine counts all its
  tracked channels, not only the six it lists.  The 58503A's
  B4121P1115 answers `@@Ba` with the first six of its eight channels
  and tracked 8 (`docs/vp-on-jig.md`); the firmware's count stops at
  the six (smartclock-gps's `Engine::tracked`, by family).
- That NAV-SAT's residuals in position hold track each satellite's
  time error, as Time RAIM assumes.
- `@@Bk`'s clock bias: the M8T's clock runs free up to a millisecond
  (466 us on apu-0), far past the field's 3276.7 m, so it clamps.  What
  a VP reports there, and the sign of its unsigned oscillator offset,
  is not documented, and smartclockmon has no captured `@@Bk`.  Only
  the images' console GPS extension page shows either, as raw counts
  (smartclockmon, `gps_print_extension`).

## Test jig

- The repository's jig, REV 1.1.0, not ordered, is brought to
  ee_tooling's house style, the 1 PPS paths left alone: the schematic on the 2.54 mm grid (JP1
  from the emulator's library); the stackup, its table and the net
  classes; GND vias and stitching.  Waived: stub direction; in
  `hardware/oncore-jig/kicad-style.toml`, J7 and J8's GND across the
  tab.  C1 alone on U2's V3 and U3's supply order pass ee_tooling's
  check since cd6304f.

A stand-in for the instrument's side of the Oncore socket, for a real
VP Oncore or this board, in `hardware/oncore-jig/`: a two-layer JLCPCB
board, one engine each; build as many as a test needs.  Free engines: the
Z3801A's and the Z3805A's, both B1121P1114; the 58503A's B4121P1115
once the board runs in a Z380x.

Rev 1.0.0 ordered from JLCPCB (fab and top-side assembly) on
2026-10-29: `production/oncore-jig/20261029-1.0.0/`, built from be222aa.
Brought up with a VP (B1121P1114, software 8.4) on 2026-11-06, all of
`docs/bring-up.md`'s jig steps: on USB, +5V 5.03 V, J9 200 mA, D1 lit;
`@@Cj` answers, `@@Ca` passes (`0x0000`), 1 PPS on both outputs, RXD1
and TXD1 on J6.  Several parts were rotated in JLCPCB's placement preview and not
recorded, so the committed CPL is not what was built: before any
re-order, recover the corrections from a new JLCPCB quote's preview of
the same files and add them to ee_tooling's `parts.toml`.  When it
arrives: the hand-fitted parts, washers under the engine, then first
power-up.

Rev 1.1.0 (board files, 2026-10-30; not ordered):

- J9's Kelvin pickup: each tap steps straight off the middle of one of
  R6's inner edges (y 81.6, 82.4) into the 0.8 mm gap between its pads,
  0.25 mm either side of center, then turns at a right angle (the gap
  holds one 0.25 mm trace at 0.2 mm clearance): +5V's west, J2-PWR's
  east to a via, back under R6 on B.Cu at 45 degrees, and up beside
  +5V's, so the two run as a pair, 0.25 mm at 0.65 mm pitch, from just
  below R6 to J9.  Net ties NT1 and NT2 staggered 4 to 6 mm down the
  pair, then /SENSE+ and /SENSE-.  In 1.0.0 the ties sat on the pads'
  west sides at their centers, so J9 read some of the pads' and
  solder's drop.
- D1 (power) near the main board's bottom edge, level with the
  I_ENGINE label, POWER below it; R5 above; D1's cathode to the front
  GND pour.
- 1 PPS outputs: all four connectors time-matched.  Each output's trunk
  ends at a junction with equal branches to its BNC and its SMA; output
  A's trunk (RN1) has a hairpin with a round end north of RN1.  Copper
  paths (track lengths, vias not counted):

| Path | mm |
|---|---|
| U4 to U3's bus, 1A0-1A3 (to RN2) / 2A0-2A3 (to RN1) | 19.2 + 2.3 to 6.2 / 19.2 + 1.0 to 4.9 |
| U3 to RN1 / RN2 | 3.2 to 3.5 / 2.2 to 2.7 |
| RN1's nearest pad to J3 (BNC) / J10 (SMA) | 41.67 / 41.67 |
| RN2's nearest pad to J4 (BNC) / J11 (SMA) | 41.27 / 41.27 |

  A's 0.40 mm over B offsets B's longer inputs and RN traces (averaged
  over the four sections).  Not counted: the connectors' own delays,
  BNC against SMA (no figures).  Fit one connector per output, a BNC
  or an SMA: with both, each is a stub on the other.  To measure: U3's output skew within one package (74ACT244 data
  sheet), the outputs against each other.
- To add: a 1 PPS LED on the breakaway, through a pulse stretcher (a
  one-shot), so a pulse of any width shows and the jig shows its
  engine's pulse without a scope.  Fed so the four outputs' matched
  paths and U4's single load on the source are unchanged.

- Schematic: `hardware/oncore-jig/oncore-jig.kicad_sch`, edited in
  KiCad; ERC clean.
- Board: `hardware/oncore-jig/oncore-jig.kicad_pcb`, routed in KiCad;
  DRC with schematic parity clean.  `oncore-jig.kicad_dru` allows the standoff ring to its own
  hole, J2's courtyard over H4's and J1's silk at the edge.
  Locked: the outline, mouse bites, connectors, standoffs and mounting
  holes.  USB-C, U1 and U2 in line, so D+ and D- run straight into the
  bridge (U1 mirrored in the schematic: the connector on pins 6 and 4).  The jig 102.55 x 70.80 mm, a
  10 mm margin round the engine's outline so its corner holes stand well
  clear of the standoffs; the driver 35 x 70.80 mm on the right across a
  2 mm slot, held by three tabs with mouse bites: 8 mm at the ends; 12 mm in
  the middle, carrying +5V and GND at 1.0 mm and 1 PPS straight across
  at J7's and J8's 2.54 mm pitch, three bites each side and
  one between each pair of traces, with
  no pour between J7 and J8 (a locked rule area).
  Rounded corners and a grounded M3 hole in each corner of both halves
  (a 5 mm pad stitched by eight vias, as on the emulator).  GND pours on
  both layers of each half, locked, stopping at the slot; the edges,
  the slot's included, stitched by GND vias 1 mm in every 3 mm; the
  pours stitched to each other on a loose 6 mm grid, clear of silk.  The
  engine's outline in silk, broken round the standoff rings.  The
  standoffs on `oncore-jig:Sinhoo_SMTSO30110CTJ-B`, Sinhoo's land
  pattern (`oncore-jig.3dshapes/README.md`).  The SMAs (J10, J11) show
  KiCad's Molex 73251-2200 model as a stand-in for BAT Wireless's.
  J2's courtyard overlaps H4's, as the engine's J1 sits by its hole.
  J1 flush with the edge.  U2 on
  `oncore-jig:QFN-24-1EP_4x4mm_P0.5mm_EP2.6x2.6mm_CenterVia`: its
  exposed pad's paste clear of one via at its center.  J9 reads R6 at
  its pads' inner edges (Kelvin): net ties NT1 and NT2 there put it on
  its own nets, /SENSE+ and /SENSE-, a diff pair (netclass Sense) off the engine's
  current path.  The middle tab's
  +5V and GND necked to 0.3 mm across the bites.
- Socket: the instrument's side of J1, a 2x5 female at 2.54 mm, SMD,
  placed by JLCPCB: BOOMELE 2.54-2*5P (C261072; C843271 the same
  drawing), on `oncore-jig:PinSocket_2x05_P2.54mm_Vertical_SMD_BOOMELE`
  (BOOMELE's land pattern; pin 2's pad 0.3 mm from H4's ring).  Pinout
  and position in `docs/oncore-ten-pin.md`, under the Oncore's outline
  (82.55 x 50.80 mm) with SMT M3 standoffs at its four holes, placed
  by JLCPCB: Sinhoo SMTSO30110CTJ-B, 11 mm (C2916372; 7300 at JLCPCB
  on 2026-10-29; PEM's SMTSO-M3 all 0).  The standoff is at least the
  engine's seated height: its connector body (about 2.36 mm, AMP
  104326-05, VP engineering notes Appendix A; our J1's 2.54 mm) on the
  socket (8.5 mm body on 0.6 mm feet: 9.1 +/-0.2 mm): about 11.5 mm,
  so shim the standoffs with about 0.5 mm of M3 washers until the
  engine seats without bowing.  The socket's contact depth is not on
  its drawing: check the engine's pins seat on a sample.  Our board needs J1 fitted (Samtec TSW, 8.38 mm below the
  board).
- USB-C (5.1 k on CC1 and CC2) and a CH9102F (C2858418, QFN-24; 9549
  at LCSC on 2026-10-29, $1.28 each) with VIO on 5 V: TXD and RXD at
  the engine's levels, no translator; its built-in unique ID is the USB
  serial number, telling jigs apart.  R7, 10 k, pulls the engine's
  TXD1 up: a TTL high (2.4 V min, VP engineering notes) past the
  CH9102F's VIH (2.5 V at VIO 5 V, data sheet 6.2).  A TX, RX and GND
  tap header, for monitoring only.
- +5 V to pin 2 from USB VBUS or a screw terminal, by solder jumper;
  the terminal, Phoenix MPT 0,5/2-2,54 (KiCad
  `TerminalBlock_Phoenix_MPT-0,5-2-2.54_1x02_P2.54mm_Horizontal`), not
  fitted.  R6, 0.1 ohm 1% (PSA FBF05FTPR100, C2929805), in series
  with pin 2, read across a 2-pin header (J9) in mV: 1 mV is 10 mA.  A
  VP takes 275 mA at most (VP engineering notes), an Oncore antenna up
  to 80 mA (UT+ notes), the driver 100 mA into two 50 ohm loads: about
  455 mA of USB's 500.  USB power is a convenience: no inrush limit
  (about 30 uF on VBUS), and the engine can sit below its 4.75 V;
  measure a receiver on J5 from a bench supply, its voltage checked at
  J2.  JP1's 1-2 is copper: cut it before bridging 2-3.  A power LED.
  No BATT (pin 1 open).
- 1 PPS driver, on a breakaway (mouse bites) with two M3 holes, for
  use elsewhere: 1 PPS, +5V and GND cross the tab, with through-hole
  pads each side (J7, J8: 1 +5V, 2 1 PPS, 3 GND) for wires after the
  break.  The input through a link (R3, room for a damping resistor)
  and a pull-down to a 74AHCT1G126 (U4, C163712), so the source sees
  one gate.  It drives a 74ACT244, four sections in parallel through a
  4x180 ohm array (RN1), a 50 ohm source (check against an open and a
  50 ohm load; change the array's value to trim), to a through-hole
  BNC; the other four the same (RN2) to an SMA.  Both connectors
  fitted by hand.  Its 74ACT244 on a 5 V branch through a ferrite
  bead, 10 uF and 100 nF at the part.
- Antenna: ours, through our active GPS splitter.
- Tests, in order: VP against VP, for how much two engines differ
  (sawtooth, `@@Bk`, `@@Ce`, answer timing); then the board against a
  VP, B1121P1114, before the board goes into a Z380x; then the
  B4121P1115 against the firmware after an `@@Ea`.  Swap the engines between
  jigs and average to remove each jig's offset.
- First power-up of the board on the jig: supply current, idle levels,
  1 PPS level and polarity, antenna supply.  On a VP: its TXD1's high
  level at U2.
- Prior art: TAPR's TAC-2, a carrier for the VP and UT+.

## After going public

- Share the Oncore code with smartclockmon (an Oncore simulator for
  its harness): a public crate both depend on by git, not a submodule.

## Could do

- Move the `pps-tic-measurement` skill (`.claude/skills/`) and the
  tools it runs (`bench/tic_setup.py`, `pps_compare.py`, `analysis.py`)
  to a repository shared across projects, such as ee-instruments: they
  measure any 1 PPS on the bench's 53230A and scope, nothing of this
  project's.  Then take them back here as ee_tooling's are.
- Maybe: the engine's own second, when the module is silent, runs on
  the STM32's HSI, good to about 1%, so its messages drift 10 ms a
  second from the second they name; a VP keeps them on its TCXO.
  Measure the HSI's second against the module's epochs while they come,
  and run on that.  It matters only while the module is silent, when
  its 1 PPS is usually gone too.
- WMM2025's successor's coefficient file, after 2030.  Past its
  five years WMM2025 extrapolates; `@@Bk`'s magnetic variation, which
  only the images' console shows, is all it feeds.

### Future board changes

- Reset and bootloader entry from the jig's USB serial port.  The jig's
  CH9102F (5 V I/O) reaches only socket pins 8 and 9 (USART1, the ROM
  bootloader's port); its DTR and RTS are not connected.  Pins 1, 4
  and 5 are open on the emulator (`docs/oncore-ten-pin.md`).
  - Cheapest: jig DTR to socket pin 5 (unused on an Oncore), through a
    jumper so it is off with a VP fitted; on the emulator, pin 5 to
    NRST through an open-drain stage or divider (NRST's 5 V tolerance
    to check, STM32C091 data sheet).  With firmware that jumps to the
    ROM bootloader when it finds RX held in break at reset, this
    recovers hung application code without SWD or an option-byte
    change; not a flash that breaks the boot code.
  - Fuller: DTR to NRST and RTS to BOOT0, for `stm32flash -i` reset
    sequences.  Needs NBOOT_SEL cleared so BOOT0 counts, and a series
    resistor on PA14-BOOT0, which is SWCLK too.  RTS has no safe pin:
    pin 4 is a VP's VPP (flash programming) and pin 1 its backup
    battery input.
  - Before either: how each SmartClock wires socket pins 1, 4 and 5
    (ask sc); one tied to anything would hold the board in reset or in
    the bootloader in the instrument.
  - Without a board change: the firmware's knock, `@@Zb` (PLAN.md,
    "Bootloader re-entry"), which needs the firmware alive.
