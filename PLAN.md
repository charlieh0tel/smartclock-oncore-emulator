# oncore-emulator

Rev 1.0.0, built and run in a 58503A: a modern u-blox timing receiver
behind a small microcontroller that speaks the Motorola Oncore binary
protocol, as a drop-in replacement for the GPS engine inside HP /
Symmetricom SmartClock receivers (58503A, Z3801A, Z3805A and kin) that
take the Oncore with a right-angle antenna jack.  Hosts that plug a
straight jack into the motherboard (Z3816A, KS-24361) are not drop-in.

The firmware study these references point to is in
[smartclockmon](https://github.com/charlieh0tel/smartclock).

Commit hashes cited here, in `TODO.md`, in `production/`'s source
packages' COMMIT files and by the board tags are from this project's
private predecessor repository and do not resolve in this one.

## Why

- **Sensitivity.**  Much greater than the Oncore's.  A current module
  with an amplified antenna may hold satellites where an Oncore on
  the same feed does not.
- **Rejection and selectivity.**  Possibly better against
  interference and out-of-band signals; not measured.
- **Channels.**  Many more than the Oncore's eight.  The firmware
  still sees only as many as its 8-channel messages carry.
- **Constellations.**  GPS alone; SBAS, GLONASS, Galileo, BeiDou,
  QZSS and IMES off (CFG-GNSS at each boot).  u-blox advises QZSS on
  whenever GPS is, against cross-correlation (UBX-13003221 R28, p. 208,
  UBX-CFG-GNSS); it is off by choice.
- **1 PPS quantization.**  Likely finer-grained.  The Oncore's 1 PPS
  carries a deterministic error from "the granularity of the clock
  generating the 1PPS signal", which the receiver reports as the
  negative sawtooth so a user can remove it (VP Oncore Command
  Reference, Time RAIM).  No SmartClock image applies it: the only
  reader of the field is the Time RAIM page (smartclockmon
  `docs/firmware/README.md`).  So all of it enters the measured
  interval, the white phase noise the stability curves show out to
  about 500 s on a 58503A (`docs/stability.md`).  Motorola gives the
  VP and UT+ under 50 ns 1σ in position hold; Synergy Systems gives
  its u-blox based replacement under 25 ns (tech note #484-D).
- **Antenna current.**  The bias, 5 V as the Oncore's or 3 V (JP1),
  goes through the LEA-M8T's V_ANT: under 50 mA, 100 mA absolute
  maximum, cut on a short (UBX-15025193 R10, 4.1, 4.2, 1.20.2).  The
  UT+ notes allow an Oncore antenna 15 to 80 mA (`third_party/motorola/`).
- **Availability.**  The Oncore engines are long out of production.
- **Week rollover.**  The engines predate the 1999 and 2019
  rollovers, and one still in service reports a date 1024 weeks early
  (smartclockmon `docs/scpi/undocumented.md`,
  `:DIAGnostic:IDENtification:GPSystem`).  The images take the date
  from `@@Ba` and `@@Ea` as given and compare no week of their own
  (smartclockmon `docs/firmware/58503a-3704.md`, "Time Reset and the
  Oncore's time"), so the emulator sends the module's date, the true
  one.  The module resolves a broadcast week into the 1024 weeks from
  its rollover base (CFG-NAVX5 wknRollover), set to the firmware's
  build week, since no date is earlier: each build reads dates right
  for about 19.6 years from its build.  TIM 1.10's and 1.11's own
  base, 1867, fails from 2035-06-03.

## Shape

- **GNSS:** u-blox's LEA-M8T (others in Choosing the module).  It
  runs no user code; the firmware is u-blox's.
- **Translator:** an STM32C091FCP6 (Cortex-M0+, 256 KB flash, 36 KB
  RAM, TSSOP20) with two USARTs: UBX from the module, Oncore binary to
  the instrument.  Its TSSOP20 pinout is the STM32C011F6's for every
  pin the board uses (DS14720 against DS13866); the C091 bonds further
  ports to pins 1, 15, 19 and 20.  KiCad has no symbol for it, so the
  project library's `STM32C091FCPx` is the C011's with those four pins
  renamed.
- **1 PPS:** the module's time pulse straight to the instrument's 1 PPS
  pin through a 3.3 V to 5 V level shifter.  The microcontroller never
  touches the edge, so it adds no jitter.
- **USB:** the module's own USB port brought out, for the full
  protocol (UBX, NMEA, raw measurements) to gpsd or u-center, and for
  module firmware updates, with no translator code.  VDD_USB comes
  from VBUS through its own 3.3 V regulator, enabled by the module's
  3.3 V supply, as u-blox requires: the module's D+ pull-up shows only
  to a powered host, and only while the module is powered.  The port
  is USB 2.0 full speed, 12 Mb/s (UBX-15025193 R10, 1.17.2).  The
  `90Z_DIFF` class says 0.30 mm tracks 0.15 mm apart, about 92 ohm on
  L1 over L2 with the pour 0.3 mm away (a 2-D field solve); the routed
  pair is 0.2 mm wide, as ordered, and is not re-routed: at this speed
  most layouts work.  The class stays as marked; the difference is
  waived.
- **Flashing the STM32:** SWD on a Tag-Connect TC2030, and the ROM
  serial bootloader for `stm32flash` on a 6-pin FTDI TTL-232R-3V3
  header (USART1, not fitted; 3.3 V only, since the bootloader turns
  on PA10's pull-up, which the STM32 forbids above 4 V), with BOOT0
  held high by a solder jumper.  The pin counts only once option bit
  NBOOT_SEL is 0; the factory setting ignores it, and a blank part
  boots the bootloader anyway (AN5673).  No USB on the STM32.  A
  blank or bootloader-mode STM32 in the instrument listens on J1 pin 9
  (USART1 RX): a 0x7F in the instrument's stream starts the
  bootloader, which answers on pin 8.
- **Power:** 5 V from the Oncore socket, regulated to 3.3 V.  On the
  bench, D8, a Schottky from VBUS, powers the board from USB-C; it
  can only feed an instrument from USB, never the other way.  Nothing
  but the USB host limits that current (D8 is rated 1 A): use a
  computer's port, not a charger, when the board is in an instrument
  that is switched off.  On USB alone +5V is VBUS less D8's drop, about
  4.4 to 4.7 V, under U3 and U6's 4.5 V minimum on a weak port, so J1's
  output levels are not guaranteed on the bench.
- **Antenna bias:** 5 V as the Oncore supplied it, or 3 V from the
  module's VCC_RF by a solder jumper, through u-blox's 10 ohm R_bias,
  27 nH and 10 nF (UBX-15030060 R07, Figures 11 and 12).
  Once the firmware has saved UBX-CFG-ANT (TIM 1.1x ships with short
  detection off), the module switches V_ANT off when it sees a short,
  retries, and reports the antenna's state in UBX-MON-HW, which the
  STM32 reads; R_bias limits the current until it does, about 0.5 A
  at 5 V, so it is a 0.25 W or better anti-surge part as u-blox's
  parts list asks.  A TPS2553 current limit
  was dropped: its range stops above the module's 100 mA absolute
  maximum, the -1 latches off, and its fault flag, with a 7.5 ms
  deglitch, would rarely see a short the module has already cut.
- **Board:** the Oncore's outline, 82.55 x 50.80 mm, with its four
  3.2 mm holes 3.0 mm from the edges, and within its height: the
  Oncore's parts reach 7.18 mm above the board (UT+ notes; VP 7.35
  max), this board's about 3.2 mm (J3).  Below, the Oncore has its
  shield S2 (6.9 mm) and J1 (8.2 mm); this board has only J1 and J2
  (6 mm body).  J1's heights are in `docs/oncore-ten-pin.md`.  The holes are plated, with 5.0 mm pads (the most the edge
  allows), and float, as the Oncore's do (checked on one).  J1 sits at the Oncore's position
  and takes a 2x5 header on either side: on the bottom to plug in as
  the Oncore did, or on top for a ribbon jumper down to the
  motherboard.  Seen from above, both ends of a straight-through IDC
  ribbon then mate the same way, so pin n meets pin n.  Pin 1 is
  marked on both sides, with the header's outline on top.  J1 is
  wired as Motorola's table has it: pin 8 (TXD1) is the receiver's
  output and pin 9 (RXD1) its input.  Synergy's description column
  has pin 8 the other way (open question 3).  R25 holds the transmit line at idle while the STM32 is in
  reset, halted or blank.  The antenna is a right-angle
  through-hole MCX jack on the bottom, as on the Oncore, with its pin
  at the Oncore jack's position and its face at the left edge; MCX
  mates with the OSX plug on the instrument's cable.  USB-C sits on
  the same edge below it, and the FTDI header on the lower edge,
  backed off so its pins stay inside the outline.  In the instrument
  the board is buried under the power supply, so both are for the
  bench.
- **Layers:** four, 1.6 mm, on JLCPCB's standard JLC04161H-7628
  stackup.  Layer 2 is unbroken ground under the module and the RF
  trace, which makes the 50 ohm J2 to RF_IN run 0.36 mm wide (0.2104
  mm of 7628 prepreg, Er 4.4, less a little for solder mask) instead
  of about 2.9 mm on two layers, and leaves routing easy around the
  module.
- **Ordering:** `jlcpcb-release` (`hardware/ee_tooling`, from
  `jlcpcb.toml`) builds each board's package in
  `production/BOARD/YYYYMMDD-REV/` (see
  `.claude/skills/jlcpcb-release/`).
  JLC04161H-7628 with impedance control, since the RF
  trace's width assumes it; the board's physical stackup is
  JLC04161H-7628's, impedance-controlled, with thicknesses and Er
  (prepreg 4.4, core 4.6, mask 3.8; KiCad keeps a mask's Er only with a
  material, so `Liquid Ink`) from JLCPCB's impedance page
  (<https://jlcpcb.com/impedance>), from rev 1.1.0; 1.0.0's records
  only the 1.6 mm thickness.  ENIG finish: the RF trace is bare copper.  J2 is the
  one through-hole part JLCPCB fits, on the bottom: confirm its
  bottom-side through-hole assembly when ordering.  JLCPCB has no model
  for J2: an order remark, in English and Chinese, says it goes on the
  bottom with its face out past the left edge, as the bottom silk shows
  (text in `.claude/skills/jlcpcb-release/SKILL.md`).  J2 and J3 overhang
  the left edge (x = 100): no rail or V-score on that edge.
- **Layout:** the schematic is edited and the board routed in
  KiCad, and schematic changes reach the board with Update PCB from
  Schematic.  TP2, a Keystone 5019 loop by J1 for a scope probe's
  ground clip, is not fitted.
- **Routing:** by hand in KiCad, every net connected.  Layer 2 is a
  GND plane and layer 3 a +3V3 plane, with GND pours on top and
  bottom; all four zones are drawn larger than the board and clipped
  by its outline.  GND and +3V3 pads reach their plane by a short
  N/S/E/W track to a via (two for pads 2.5 mm or longer), except U1's
  ground pins 13 and 14, which reach the top pour through its thermal
  spokes.  GND vias
  stitch the pours together 1 mm in from the edge every 3 mm (2 mm
  along the RF corner by J2, and on under J5's overhanging pins), and
  on a 5 mm grid inside, wherever both outer pours are solid copper
  and no part is in the way.  The RF trace is
  placed by hand and locked, keeps 3W (1.08 mm) to the pour and other
  nets, is fenced by ground vias along both sides, and has D7 on it
  beside J2.  It has no solder mask, except 0.4 mm dams either side of
  D7's pad and before U1 pin 16, so solder stays on those joints.  No other net's vias come within 0.4 mm of the module's
  pads.  Silkscreen text is 0.8 mm high with a 0.15 mm stroke.
- **Assembly:** JLCPCB, passives no smaller than 0603 except the RF
  input's ESD diode (PESD5V0F1BL, SOD-882: 0.4 pF, 5.5 V stand-off,
  above the 5 V antenna bias on RF_IN).  The schematic carries each
  part's manufacturer, part number and LCSC number, all in JLCPCB's
  stock on 2026-10-25 except J1 and the cell.  The LEA-M8T is JLCPCB's
  C2763452, a LEA-M8T-0 (TIM 1.10), of which JLCPCB had only 8: check
  the count before ordering.  JLCPCB places it with u-blox's T-shaped
  paste (UBX-15030060 R07, Figure 6), so ask for the 150 um stencil.
  Why the -0 and not the -1 is in `docs/ublox-lea-m8t-version.md`.
  J4, JP1, JP2 and TP1 are footprints only, out of the BOM.  J1 is
  marked not fitted and the backup cell is not fitted: both are
  ordered loose and fitted by hand, J1 on whichever side the instrument
  needs.  J1 is a Samtec TSW-105-07-G-D (DigiKey), whose 5.84 mm mating
  length is the Oncore's; CJT's A2541WV-2x5P (LCSC C225520, 6.0 mm) is
  the LCSC alternative.  The connector's pinout, position and heights
  are in `docs/oncore-ten-pin.md`.  J5 and TP2 (the Keystone 5019, out
  of stock) are not fitted.  The STAT LED
  is yellow-green: a green's 3.1 V barely lights from 3.3 V.
- **Backup supply:** the module's V_BCKP tied to VCC by default.  The
  option not fitted is a Seiko MS621FE-FL11E rechargeable lithium cell
  (5.5 mAh, about 13 days at the module's 17 uA; rated -20 to +60 C,
  narrower than the module's -40 to +85 C), floated at about
  3.2 to 3.3 V through a BAT54J Schottky and 1k: at float current the
  Schottky drops only 0.1 to 0.15 V.  The cell wants 2.8 to 3.3 V,
  3.1 V recommended, so this is the top of its range; a silicon
  diode's 0.6 V would leave it at the bottom.  Its reverse leakage (2 uA maximum at 25 V,
  25 C) matters only while the board is unpowered, when the instrument
  has cooled and D3 sees about 3 V: a few percent of the module's
  backup current.  No SMD supercap comes close: the 0.22 F parts
  are end of life and the active ones hold minutes.  The
  cell is not reflowable (third_party/seiko/).  No known instrument
  drives the Oncore's BATT pin, so it is not used.

## What the translator does

- Answers, and sends, only the messages the target image expects.
  Nothing else goes on the wire, unsolicited or not.  smartclockmon
  `docs/firmware/oncore/models.md` lists them for the six Oncore
  images, and `<image>.txt` beside it gives each image's message
  table, request scripts and polling lists.
- Fills them from the module: time, satellites, position, receiver
  status; honors position hold, the survey, and cable delay; fills the
  T-RAIM fields.
- Presents a week number the firmware handles.
- Carries the module's own 1 PPS quantization error into the Oncore's
  sawtooth field.  The firmware displays it, which also shows whether
  anything applies it.

The firmware must also set the hardware up, since the module's and
the MCU's defaults are not the Oncore's:

- **1 PPS** goes from the module to J1 without passing through the
  MCU, so its shape is the module's.  UBX-CFG-TP5 sets it to the
  Oncore's: a 200 ms (+/-1 ms) rising-edge pulse from power-up, not
  the TIM default of 100 ms only once locked; antenna cable delay 0
  (the default is 50 ns) until @@Az sets it; aligned to UTC, not GPS
  time.  Saved to the module's BBR and flash.
- **Antenna supervisor:** UBX-CFG-ANT with short detection, power-down
  on short and recovery all on, and TIM 1.1x's default pins (switch
  PIO16, short PIO15, open PIO14) with `reconfig` set, saved.  TIM 1.1x ships with them off
  (UBX-13003221 R28, C.1), and R1 only limits a short's current
  until the module cuts V_ANT.
- **USART1** on TSSOP20 pins 16/17 needs the SYSCFG PA11/PA12 remap
  to PA9/PA10.
- **RESET_N** (PA4) is driven open-drain only: u-blox says never drive
  it high.  The firmware holds it low 10 ms at each start: the reset
  clears the module's RTC time but not BBR (UBX-15030060 R07,
  1.5.1), so after the MCU restarts with the module powered
  the module reports no time of its own before a fix.
- **Bootloader re-entry:** the factory NBOOT_SEL = 1 ignores BOOT0,
  so once the part is programmed JP2 does nothing.  The firmware jumps
  to the ROM bootloader at the bench's knock, `@@Zb` with argument
  `LOAD`, on the instrument's port (J1, through the jig's CH9102F, or
  J5): it answers, writes a request word in `.uninit` RAM and resets;
  the next start, before anything is set up, takes the request, holds
  the module in reset (RESET_N low), maps system memory at 0, and
  jumps (`oncore-fw/src/bootloader.rs`).  SYSCFG's pin remaps stay at
  reset: the bootloader puts USART1 on pins 16 and 17 itself, and with
  them set beforehand it left PA9 (TX) off pin 16 (read over SWD).  It
  also takes PA4 for its SPI port, releasing the module.
  `bench/stm32boot.py --knock` sends the knock, then flashes
  (AN3155, 8E1), as `stm32flash` 0.7 does not know the STM32C091's ID,
  0x44d.  A blank board's bootloader answered over the jig, the module
  running, and took the first flash; then, programmed, the knock and
  a serial flash of 090c1f9, which ran after a power cycle (2026-11-15).
  `stm32boot.py write --go` starts the new firmware with the
  bootloader's Go (AN3155): at start, before anything is set up, the
  firmware finds the bootloader's state (system memory still at 0, or
  USART1's clock on) and resets, so it runs from reset state.  Two
  rounds of knock, flash and Go on a board, each answering `@@Cj`
  after, the registers then the firmware's own (read over SWD,
  2026-11-15).  A blank part's first flash needs a power cycle
  instead, for its EMPTY flag.  A reset or power cycle also leaves the
  bootloader for the firmware.  A board without that
  firmware needs SWD.  After the first flash the
  EMPTY flag needs a power cycle or option-byte reload (AN5673).  The
  STM32C091's ROM bootloader also listens on USART2, the module's
  link (AN2606, STM32C091xx/92xx), and takes the first interface to
  send it 0x7F, hence the module held in reset; a blank or
  bootloader-mode part with the module running may answer only over
  SWD.
- **PA15** shares TSSOP20 pin 19 with PA14-BOOT0/SWCLK on the STM32C091
  and is never driven.
- **RAM:** UBX messages such as NAV-SAT (8 + 12 bytes per satellite)
  and RXM-RAWX (16 + 32) are parsed as they stream in.

## Firmware

Rust, on Embassy, in `firmware/`: a workspace of the crates that build
on a host (`oncore-core`, `oncore-app`, `oncore-host`; one `cargo test`
runs them all, with shared lints and one `rustfmt.toml`), and
`oncore-fw`, outside it, which builds for the STM32.  It targets the Z3801A (3543) and
Z3805A (3543B) first, whose message tables are the same apart from
addresses, then the 58503A (3633, 3704).  No board exists yet; the code
is written and tested on the host.

The emulator presents one Oncore model, chosen at build time, its
personality.  The VP reference's model numbers (p. 2) and the engines
on the bench (smartclockmon `docs/firmware/gps.md`) give two:

| Model | What it is | Software | Seen in |
|---|---|---|---|
| B1121P1114 | 6-channel VP (B1) | 8.4, options IB | Z3801A, Z3805A |
| B4121P1115 | 8-channel VP (B4) | 8.8, options IB | 58503A |
| R5122U1152 | UT Plus, 8 channels | 3.1 | 58503B, Z3816A (their UT Plus path) |

The Z380x images speak only the six-channel messages.  The 58503A
images take either and switch to the eight-channel ones once the
engine sends `@@Ea`.  The model sets the `@@Cj` identity and the
channel count, and so which messages it answers.  One firmware serves
both: the engine is the B1121P1114 from power-on and the B4121P1115
from the first `@@Ea`, in RAM until power-off.  The UT Plus is the
other personality, chosen at build time (TODO.md, "UT Plus"): the
58503B and Z3816A take their UT Plus path when `@@Cj`'s model line has
an `R` as its 12th character, and at software 3.0 or later send `@@Am`
and query `@@Gj` (smartclock's reading of the images).  Its model is a
shipped 58503B's (smartclock) at software 3.1; no 3.x identity is
known, so the identity's copyright years, date and options are
placeholders, which those images do not read.  Its lengths are tested
against those images' tables; `@@Sz`, which they frame at 7 bytes and
TRM0003 gives at 8, is never sent.  The 58503A polls `@@Ea`
at power-up, before its `@@Cj`, and the Z380x images do not hold it.  A
reset of the STM32 on a 58503A goes back to six channels; it is not
expected.  `@@Zr` with the argument `BOOT`, which neither the reference
nor the images use, resets the STM32 on the bench, in every build: a
break would not do, since a host's TX can sit low while it resets.  The
module is not reset.  Checked against the images
(`docs/emu-board-bring-up.md`).  How the images talk to the engine is
smartclockmon's `docs/firmware/gps.md`; the messages and what the images
settle are in `docs/oncore-messages.md`.

Settings the instrument sends (the `@@A` messages):

- **Mapped to the module:** initial position and height, mask angle,
  application type (dynamic model), position hold, altitude hold, UTC
  or GPS time, 1 PPS offset and cable delay, through UBX-CFG-NAV5,
  CFG-TMODE2 and CFG-TP5.  The engine takes a position, to start from
  or to hold, only within the manual's ranges (pp. 17, 21, 25, 54),
  refusing one outside them.  The GMT offset is applied by the
  emulator.
- **Not given to the module:** time and date (`@@Aa`, `@@Ac`).  An
  instrument's time can be wrong, a SmartClock's by the 1024 weeks of
  its own rollover, and time aiding worse than its stated accuracy "may
  lead to substantially degraded receiver performance" (receiver
  description, UBX-MGA-INI-TIME_UTC); the module, reset at each start,
  takes time only from the satellites.  The engine sets its own clock
  from them until the module's first fix.
- **Channel selection:** the emulator has the Oncore's 6 (or 8)
  channels and fills them from the satellites the module tracks by the
  Oncore's rules: `Ah`/`Ai` (automatic, manual, highest in sky), `Aj`
  (xDOP type), `Ak` (hysteresis), `Ar` (Best-4 or all in view) and
  `Am` (ignore list).  The per-channel
  messages work on the channel set, Time RAIM on every satellite the
  module used; the module still
  navigates on every satellite it tracks, ignored ones included: the
  M8 has no setting to leave one out (CFG-GNSS chooses whole systems).
- **No module equivalent:** `Aq` (ionosphere and, on the UT Plus,
  troposphere models: no CFG-NAV5 or CFG-NAVX5 setting turns the
  module's off), `An` (almanac update), `AD` (smoothing thresholds),
  `Ax` (measurement epoch offset) and `Ci` (NMEA output) are stored and
  answered and change nothing, the owner's exceptions
  (`docs/gps-setting-exceptions.tsv`, held by `record.rs`'s test
  against smartclock-gps's `IMAGE_HOLDS`; `DECLARED` makes the pulse,
  so the pulse settings are held too).  The
  images set each to what the module does anyway.  So are `Al` and `AC`, the
  3-D to 2-D and 2-D to no-fix DOP thresholds: the module chooses its
  fix mode itself, against its own DOP masks (CFG-NAV5 pDop and tDop,
  25.0), and takes no threshold between 3-D and 2-D.
- **Datum:** WGS-84 only.  Any `Ao` is answered as datum 49 with
  WGS-84's parameters (in `@@Ap`, as the images take it); `Ap` is not
  implemented.  Every image selects 49, and none decodes the `Ap`
  answer.

Satellite reports (`@@Ba`, `@@Bb` and their eight-channel forms):

- **Tracking mode**, from the module's quality indicator and whether it
  holds the ephemeris: searching 0, acquired 1, detected but unusable
  2, code locked without ephemeris 7, locked with ephemeris 8.  Mode 8
  alone puts a satellite in the images' position set; 1 to 4, 5 to 6
  and 7 to 8 are the status screen's bands (smartclockmon, from the
  images' C).
- **Signal strength** is the Oncore's 0 to 255 "proportional to SNR",
  which the images print raw, without units.  It is proportional to
  amplitude, C/N0 = 20 log10(SS) + K, and the emulator takes K = 3
  dB-Hz for both models, a healthy engine's, within about 2:
  - The exponent: fitted 2.09 (0.23 dB RMS) for the bench 58503A's
    B4121P1115 and 1.98 for the Z3805A's B1121P1114, from the bench's
    2026-10-03 logs (the u-blox's C/N0 bands against each engine's SS
    at the same quantiles, same antenna and day), with slope 1.05
    between the two over 4105 same-minute pairs.  So one scale.
  - The offset: an engine stops tracking at a fixed SS, about 20 on
    both bench units.  Motorola puts the VP's tracking threshold at
    -143 dBm (VP Oncore engineering notes), about 29 dB-Hz with a noise
    figure of 1.5 to 2.5 dB (assumed).  So K = 29 - 20 log10(20), about
    3.  Full scale, 255, is then about 51 dB-Hz, about the strongest a
    good antenna gives.
  - The bench units against it: the 58503A measures K 3.5, healthy
    within error; the Z3805A measures 14.4, about 11 dB deaf, a fault
    in that unit, since its SS still stops near 20 (with an 11 dB
    different scale it would stop near 6).
  - The measured K includes the NEO-M8T's own C/N0 calibration, worth
    1 to 2 dB.
- **Doppler** (`@@Bb`): measured (RXM-RAWX) for tracked satellites,
  less the module's oscillator drift (NAV-CLOCK, 1.57542 Hz per ns/s at
  L1), since the Oncore's Doppler is the satellite's motion alone; for
  visible ones not tracked, computed from the almanac, as the Oncore
  computes it.  On apu-0's NEO-M8T the drift was -840 ns/s, about
  -1.3 kHz; over the trace the two agree within 2 Hz for all twelve
  tracked satellites, which fixes the correction's sign.
- **Navigation message** (`nav`, IS-GPS-200N): from the module's
  RXM-SFRBX, each GPS satellite's HOW alert and anti-spoof flags,
  subframe 1's URA index and health, the ephemeris and clock
  (subframes 1 to 3, once their issues of data agree), and the almanac
  (subframe 5 pages 1 to 24, subframe 4's for 25 to 32, and page 25's
  reference time), and subframe 4 page 18's UTC parameters (GPS less
  UTC's fraction, the leap seconds now and after, and the week and day
  of a change, whose week means something only while one is scheduled,
  20.3.3.5.2.4), with position and velocity from Table 20-IV in `f64`
  (libm), one implementation for both.  `@@Ba`'s channel flags take alert,
  anti-spoof, unhealthy (module or subframe 1) and inaccurate (URA
  index 7 and up, a nominal URA over 16 m); parity errors never reach
  the emulator, since the module passes on only subframes whose parity
  holds.  `@@Bk`'s URAs are subframe 1's, 63 until decoded.

The protocol's source is the VP Oncore Command Reference
(`third_party/motorola/VPCommands.pdf`).  Where it is ambiguous,
smartclockmon's reading of the Z380x images settles it.  The emulator
must run with those images, so the tests check against them too.


- **`oncore-core`:** the translation, `no_std` with no allocation,
  tested on the host with `cargo test`.  Oncore binary framing, the
  message IDs and the deframer come from smartclock's `smartclock-gps`
  (its `oncore` module), a git dependency on its `main` branch, the
  first step of moving the engine there (the engine models after its
  PR 3).  UBX framing and decoding come from smartclock's
  `smartclock-gps-ublox`, on the same branch: a `Streamer` gives whole
  messages up to NAV-PVT's 92 bytes, NAV-SAT and RXM-RAWX a header and
  a block at a time, taken only once their checksum holds, and
  `Message::parse` and `subframe` decode every field, floats as their
  bits.  `oncore-core` keeps what it makes of each message (`gnss`,
  `nav`, `sky`) and which messages it sends the module, and with
  what values (`module`); smartclock-ubx's encoders write them.
  `record` fills a smartclock-gps `Record` from the receiver's state
  (`receiver::Receiver`: the module's messages, the channels'
  satellites, the corrections, Time RAIM) each second, for
  smartclock-gps's `Engine` to speak the Oncore from; that engine
  encodes every field of each message and the record supplies at least
  those the images read (AGENTS.md).  The engine's settings drive the
  rest: `control` turns those the module has a counterpart for, as
  `Engine::receive`'s events name them, into `module::ModuleRequest`s;
  `receiver::ReceiverSettings` holds those the receiver acts on for
  itself (the channel selection, Time RAIM, the fix: with the altitude
  held, an HDOP over `@@AC`'s threshold gives it up, VP p. 15).  A
  satellite's tracking comes from NAV-SAT's quality indicator
  (UBX-13003221 p. 397): searching, acquired as code lock, unusable as
  AGC set, and code locked or better, "time synchronized", as frame
  sync, ephemeris once any of its ephemeris subframes is read, position
  once it holds the ephemeris; nothing the module says marks carrier
  lock, bit sync or message sync.  The fix is propagated only for the
  module's dead reckoning, which the LEA-M8T never does (CFG-NAV5's
  drLimit is reserved, p. 232).  The oscillator offset is the drift's
  magnitude (VP pp. 98, 137: unsigned), unverified until a real VP's
  `@@Bk` is captured.  `record::DECLARED` declares the fields it fills
  (all but the corrections' age, which NAV-PVT lacks) and, until the
  swap, no settings; its test holds it to smartclock-gps's checks
  against `UNFILLED`.
  The models, each message's length (checked against the images'
  tables), the settings with their defaults and ranges, the clock and
  its UTC and local time labels are smartclock-gps's `Engine`'s.  The
  receiver's second (`receiver::Receiver::second`) runs once an epoch,
  at NAV-EOE: with that epoch's NAV-TIMEGPS (the same iTOW) when that
  epoch's NAV-PVT has a fix (gnssFixOK), else with no time.  Without a
  fix the module reports its RTC's time as valid, after a restart of
  its own too (UBX-13003221 R28, "Time Validity"), so its time is
  never taken then.  A leap second the module has scheduled
  (NAV-TIMELS: GPS week and day, the leap seconds before it) falls at
  the end of that GPS day in UTC (IS-GPS-200, 20.3.3.5.2.4), the
  record's leap event.  Time is held as GPS time, seconds from the GPS
  epoch, signed (page 18's and the orbits' times either side of their
  reference go negative), with the fraction of the second apart in
  nanoseconds.  Truncated weeks (page 18's 8-bit `WNt`, the almanac's)
  are taken as the full week nearest the time they are read at.
  `module` turns each module request into UBX: CFG-NAV5
  (mask angle, dynamic model, altitude hold in MSL height; its mask
  names only the request's fields, the rest at the set-up's values),
  CFG-TMODE2 (position hold, ellipsoid height), CFG-TP5 (the Oncore's 200 ms
  rising pulse, UTC or GPS grid, delays) and MGA-INI-POS_LLH (initial
  position).  Heights convert with the separation of the module's last
  fix; a hold whose height needs it (an MSL position hold, an ellipsoid
  altitude hold) is withheld until the first fix gives it, then sent,
  and so sets no receiver mode before; a hold already in the module's
  datum, or a hold off, goes at once.  `gnss` reads NAV-TIMEGPS (GPS time, leap seconds), NAV-PVT
  (fix, both heights, velocity), NAV-DOP, and, a block at a time,
  NAV-SAT and RXM-RAWX into `sky`, the GPS satellites by PRN
  (elevation, azimuth, C/N0, residual, quality, ephemeris, health,
  measured Doppler).  A message whose checksum fails changes nothing.
  `channels` fills the 6 or 8 channels: candidates are the GPS
  satellites with an orbit, healthy or of unknown health, at or above
  the mask angle and not ignored, the 12 highest.  Automatic selection
  evaluates every combination of 4 (Best-4) or of the channel count
  (all in view) by the `@@Aj` xDOP, in `f32` with a sine table, keeps
  the nine best for `@@Bc`, and changes the satellites giving position
  only when the best beats them by more than the `@@Ak` hysteresis.
  The search runs in steps so the firmware can serve its UARTs between
  them (924 combinations for 6 of 12).  `@@Ba`, smartclock-gps's
  encoder from the record: local time of the measurement epoch (the nearest second,
  as the module labels the epoch; the fraction is NAV-PVT's `nano` when
  the epoch is after the second and 0 when before, since the module's
  free-running clock carries its epoch across the second and the
  seconds must run on one a second, as an Oncore's do), the fix, velocity,
  the xDOP of the satellites giving position (0 in position hold),
  visible and tracked counts, the channels and the receiver status.
  The engine sends it polled or every 1 to 255 s on the GPS second
  (`Emulator::second`), and in idle mode answers a poll with the last one sent
  while fixing, or the current one before any (p. 70; the images poll
  it in idle mode before `@@Cg` starts fixing).  The channel flags for momentum alert,
  anti-spoof and accuracy come from the satellites' subframes (`nav`,
  below).  `@@Cj` is each model's identity in the
  layout the 58503A's B4121P1115 sends (smartclockmon
  `crates/smartclockd/src/journal.rs`): the copyright line, then nine
  label lines padded to 26 characters, options `IB`; the serial number
  and manufacture date are the emulator's own: `EMU` and seven digits
  from the STM32's unique ID, and the firmware's build date as the
  engines give it (year digit, month A to L, day; `build.rs`,
  `SOURCE_DATE_EPOCH` for a reproducible build).  PRN 32 is reported
  like any other, as a real engine does; only the 58503A's tracking
  event log, which packs a satellite in 5 bits, cannot tell it from
  PRN 10.  `@@Bj` is the coming
  leap second from NAV-TIMELS.  `@@Ca` and `@@Fa` pass the Oncore's
  hardware tests and fail every channel correlation test once the
  module has been silent 3 s.  `@@Bb` lists the visible satellites,
  the 12 highest at most, by PRN (the images bisect it), with
  Doppler, elevation, azimuth and
  health (removed by `@@Am` or not); `@@Bc` the selection's best
  combinations; `@@Bd` the almanac's status, its week, toa and
  satellites, the EEPROM almanac the RAM's, as a real engine's is once
  copied (p. 78), though the emulator keeps none;
  each polled or sent on a change: `@@Bb`, the satellite alert
  computation's result (p. 74), on each computation, every 5 epochs
  with channel selection after it (p. 37), that changes it; a real
  VP's period is not documented.  `@@Bk` gives the
  DOPs (each 0 outside its fix modes), NED velocity, ECEF position
  (NAV-POSECEF), the least-squares inverse normal matrix of the
  satellites giving position (east, north, up, time) and clock bias
  and drift (NAV-CLOCK; the drift's magnitude, the field being
  unsigned), polled or every 1 to 255 s.  Its magnetic
  variation is the World Magnetic Model 2025's declination at the fix
  and UTC date (`wmm`; the reference's engine used WMM-95), from NOAA's
  coefficient file, which `oncore-core`'s `build.rs` turns into a
  table; it matches all of NOAA's test values, declination to 0.01
  degree and the field to 0.01 nT.  The B4121P1115 also answers
  `@@Ea` and `@@Ek`, the same layouts with eight channels and eight
  URAs.  It sends `@@Ea` only when asked, as an engine does, so a
  58503A learns of eight channels as it would with its own engine
  (smartclockmon `gps.md`, "Six or eight channels").
- **Time RAIM** (`traim`, VP reference pp. 102-106): each satellite
  the module used (NAV-SAT svUsed), on a channel or not, since all of
  them make its 1 PPS, measures
  the receiver's time offset by its NAV-SAT pseudorange residual, in
  ns; the solution is their mean, and its one-sigma estimate their
  standard deviation over the root of their count (the reference says
  only that it comes from the residuals' spread).  The reference's
  table gives the satellites the alarm limit needs to detect and to
  isolate a fault; isolation also needs position hold (p. 56).  The
  satellite that disagrees most with the solution of the others is a
  fault when it disagrees by more than the alarm limit: isolated and
  removed when possible, else the solution is in ALARM.  UNKNOWN when
  Time RAIM is off, the limit is under 250 ns, or the satellites are
  too few.  An isolated satellite leaves this solution and its report
  (its channel's time zero, if it has a channel), not the module's, which cannot be told to
  drop one: the 1 PPS still comes from it, so the solution is ALARM,
  not OK, and mode 3 turns the pulse off (the module's pulse goes on
  and off by smartclock-gps's `Engine::pulsing`, the pulse status
  `@@Bn` reports), until the module's own RAIM
  drops it (UBX-13003221 R28, 31.2.3, NMEA GBS) and, its NAV-SAT svUsed
  clear, it leaves these measurements.  A VP would correct its own
  pulse and stay OK.  `@@Bn` (and
  `@@En`) carry the settings, the 1 PPS rate (1) and the time of the
  next pulse, pulse status and time base, both statuses, the
  estimate, the negative sawtooth (TIM-TP's qErr, negated, in ns) and
  each channel's satellite with its estimate of the fraction of the
  second.  The 1 PPS control mode goes to CFG-TP5: off; on; on only
  once the module has GNSS time (for "while tracking at least one
  satellite"); or on only while the solution is OK, switched as the
  status changes.  The pulse status reported is the pulse's: in mode
  2, on only while NAV-SAT shows a satellite tracked.
- **Pseudorange corrections** (`differential`, VP reference pp. 89-90):
  `@@Bh` asks for them, polled or every 1 to 255 s, and the engine
  answers with `@@Ce`, the last whole set of corrections, a second old,
  with its own measurement time, while no more than 10 s old (a set
  with no satellite leaves the last aging); the engine computes them a
  satellite a step between I/O, as it selects channels, each from an
  ephemeris within its fit interval, 2 h either side of toe
  (IS-GPS-200, 20.3.3.4.3.1).  Each holds the measurement
  time and, for each channel's
  satellite, the range from the module's position (NAV-POSECEF, the
  held position in position hold) to the satellite, from its ephemeris
  at transmission and turned by the earth's rotation in transit, less
  the RXM-RAWX pseudorange with the satellite's clock (polynomial,
  relativistic term, L1 group delay) taken out; the same for the range
  rate against the Doppler; and the ephemeris' IODE.  The measurement
  time is NAV-CLOCK's epoch less its bias; rcvTow is 15 ms earlier on
  TIM 1.10.  The receiver's clock comes out of the pseudoranges as
  rcvTow less that time, to the whole millisecond the corrections'
  mean puts it at (the module steps its clock in milliseconds), and
  out of the rates as their mean (NAV-CLOCK gives the drift only to the
  ns/s, 0.3 m/s).  What remains is the atmosphere's delay and the
  broadcast orbit's and clock's errors.  Over the trace, every
  correction is between -60 and +10 m, the most negative low in the
  sky; at its last epoch twelve fit a bias and an elevation mapping
  within 1.4 m rms; 98% of the rates are within 0.16 m/s of zero.  The
  B4121P1115 sends a second `@@Ce` for channels 7 and 8 when they have
  corrections (p. 89).  The images show the corrections' mean and
  standard deviation on a diagnostic page (smartclockmon
  `structures.md`).
- **Module set-up** (`module::setup`, a message a step): UART1 to
  115200 baud, UBX only; the time pulse; each message the engine reads
  once a second (NAV-TIMEGPS, PVT, DOP, SAT, TIMELS, POSECEF, CLOCK,
  RXM-RAWX, MON-HW, TIM-TP), RXM-SFRBX each subframe decoded, which
  the module sends only when enabled (UBX-13003221 R28, section 10), and
  NAV-EOE last; the antenna supervisor (CFG-ANT: supply
  control, short detection, power down on a short, recovery, and the
  default pins); one measurement a second on GPS time (CFG-RATE);
  continuous power (CFG-RXM); GPS only (CFG-GNSS); every
  CFG-NAV5 and CFG-NAVX5 field, the engine's power-on mask angle and
  dynamic model and TIM 1.1x's defaults for the rest; position hold off
  (CFG-TMODE2); the WGS-84 datum (CFG-DAT, as a user datum with
  WGS-84's ellipsoid and no shift: the message sets no other kind).
  Each step sets every field the engine depends on, so
  nothing rests on what the module has saved.  Then, only
  for a module found at 9600, a save to battery-backed RAM and flash: a
  module at 115200 has saved already, and the set-up goes to its RAM at
  each boot, so its flash is written once.  The set-up's time pulse
  is the power-up one, on, on UTC and with no delays, so the save holds
  none of the instrument's settings; those go to RAM at its end.  A module heard at 115200
  gets the port step too, for its other port settings.  One not heard
  is moved from 9600, and on each set-up after a failure from the next
  of 38400, 19200 and 57600, since a module saved at another rate
  keeps it through a reset; the rate to try next carries over a module
  reset, the silent one included.  A step refused or unanswered fails
  the set-up: not saved, logged, and the module reset (RESET_N) and set
  up again from the start, the instrument served meanwhile, 30 s later,
  the wait doubling with each failure in a row up to 8 minutes, and back
  to 30 s after one that succeeds.  The time pulse is the VP's
  (engineering notes p. 7): 1 Hz, the time mark on the rising edge,
  200 ms high, through U3, which does not invert.  Open-circuit
  detection is off and not planned: it needs a sense circuit across
  R_BIAS driving EXTINT1 (ANT_DET_N; UBX-15030060 R07, 2.4.4,
  Figure 13), which the board does not have.  MON-HW gives the engine
  the antenna state, TIM-TP the next pulse's quantization error.  The VP has no
  antenna status message.
- The UBX messages sent to the module (CFG, MGA-INI) are encoded by
  smartclock-ubx's encoders, every field per the receiver description
  (UBX-13003221 R28); what the module sends is decoded by
  smartclock-ubx too, `no_std`, no
  allocation, no floating point.  Neither uses the `ublox` crate
  (0.10): its all-messages decoder costs 3.1 KB of flash, it maps
  positions to `f64`, which is soft float on the M0+, its CFG-TMODE2
  scales latitude by 1e-5 where the receiver description gives 1e-7,
  and it has no CFG-CFG.  Test cases come from the Command Reference and
  the message tables in smartclockmon `docs/firmware/oncore/`;
  smartclockmon's simulator speaks SCPI, not Oncore.
- **`oncore-app`:** the emulator running, `no_std` on
  `embedded_io_async` ports and `embassy_time`, the same on the STM32
  and on a host.  One task serves both ports from the start, and the
  module's set-up runs between the instrument's commands as a state
  machine: `run` listens 1.5 s for the module at 115200 baud (its saved
  set-up), else sends step 0 at 9600 and gives it 100 ms to change
  rate, then sends the set-up a message at a time as each ACK comes
  (1 s a try, three tries).  Its deadlines are checked whatever woke
  the loop, so instrument traffic cannot hold them off.  Nothing else
  goes to the module meanwhile: at the end `Emulator::configure` gives it
  every setting as the instrument left it, the evaluated 1 PPS state
  (T-RAIM's gating) and the initial position once the instrument has
  set one.  So a slow or absent module never leaves the instrument
  unanswered; `@@Ca` reports the channels failed once the module has
  been silent 3 s.  After the set-up, module requests are queued and
  sent one at a time, each awaiting its ACK (1 s a try, three tries; a
  NAK sends it again at once); one given up is logged and every setting
  goes to the module again (`Emulator::configure`).  MGA-INI goes
  without waiting, its ACK off.  The queue holds `configure`'s requests
  and a pass's others; one that does not fit is logged.  The module's messages feed the
  receiver, NAV-EOE ends each epoch and runs the emulator's second
  (`oncore-core/src/emulator.rs`: Time RAIM, the record filled,
  smartclock-gps's engine's continuous messages), the engine answers
  the instrument's bytes, which `run` also frames for its log and the
  bench's reset, `@@Zr`; answers the engine holds back (the self-test's
  2 s) go at its deadline.  The engine starts at power-up as the
  identities `run` is given (`Identities`, smartclock-gps's presets with
  the board's serial number and manufacture date), and channel
  selection starts every 5 epochs, after the satellite alert
  computation (`@@Bb`), and runs 8 combinations at a time
  when no byte is waiting, and a 1.5 s timer runs the second
  if the module falls silent.  A second without an epoch whose NAV-PVT
  has a fix reports no fix, in position hold too.  The record's
  receiver mode is the module's: holding on a NAV-PVT fixType 5 (time
  only), navigating on 2D or 3D, kept through epochs without a fix and
  unknown once the module is lost; an acknowledged CFG-TMODE2 sets it
  first (fixed: holding, off: navigating), since an Oncore holds with or
  without satellites.  The board commands its module, so the engine
  reports hold only as the module shows it (smartclock-gps
  `ReceiverControl::Commanded`; `AsIs`, a module only listened to, is
  `Uncommanded`).  The set-up leaves CFG-TMODE2 off, so the module
  navigates until an image holds.
  Only an epoch ends
  the module's silence, not its other messages.  The almanac, the almanac
  Doppler and the magnetic variation take the time counted on from the
  last fixed epoch, as a VP's clock runs, so they outlast the fix; a
  measured Doppler needs no time.  After 3 of those seconds the engine
  drops what the module measured each second (NAV-PVT, NAV-SAT, DOP,
  NAV-CLOCK, RXM-RAWX, TIM-TP), so no report carries a fix, a signal
  or a Time RAIM solution from before; the last position, the leap
  seconds and the navigation data stay.  After 10 the module is reset
  (`ModulePort::reset`, RESET_N) and set up again, the UBX parser
  starts afresh.  The STM32's independent watchdog (8 s, stopped while a
  debugger halts the core) is fed each pass of the loop
  (`Host::alive`), so a hang or a panic resets the board.  A write that
  finds too little room in a UART's transmit buffer is tried again
  every 2 ms: embassy-stm32 0.6.0's buffered write then waits without a
  waker, and with both ports quiet would wait for good.  Answers and module requests are
  gathered, then written; a pass reads the instrument 7 bytes at a time,
  the shortest command, so it builds one answer at most and `@@Cj`'s
  294 bytes always fit.  An answer or request that does not fit is
  logged.  Its tests run it between a fake module, or
  a silent one, and a fake instrument.  With the module silent the second runs
  each second, after 1.5 s without an epoch.  On the STM32C0 at 48 MHz
  (`oncore-fw/examples/timing.rs`, the trace's second minute, on a
  NUCLEO-C092RC, 2026-10-30), longest and mean an epoch: the module's
  messages 8.0 and 5.9 ms, the engine's second 5.3 and 5.1 ms, the
  alert computation 2.1 and 0.4 ms, a selection step 18 and 186 ms, a
  correction step (one satellite) 37 and 355 ms; about 55% of each
  second.  A step holds the instrument's answers back up to 37 ms.
- **Test data:** `firmware/testdata/neo-m8t-2026-10-28.ubx`, fifteen
  minutes of a NEO-M8T-0 (TIM 1.10, as the board's module) with every
  message the engine reads and the GPS subframes, moved to the
  SmartClock's default position by `oncore-host`'s `move_trace` (its
  README).
  `oncore-core`'s `tests/trace.rs` runs it through the emulator and
  checks each epoch's `@@Ba` and `@@Bn`; `oncore-host --module file:`
  replays it.
- **`oncore-host`:** `run` on a PC (tokio): `--module serial:PATH` (set
  up as on the board), `file:PATH` (a raw UBX recording such as
  `gpspipe -R` writes, played an epoch a second) or `gpsd:HOST:PORT`
  (a gpsd device's raw stream, listened to only); `--oncore PATH`, a
  serial port at 9600 such as a USB adapter on J1 or a pty, or
  `tcp:HOST:PORT`, raw bytes to a server standing in for the
  instrument, with no 1 PPS.
  A recording or a gpsd device is not configured, so it must already
  carry the messages the engine reads; a recording's run ends a second
  after its last epoch.  `--record-script PATH` writes every command the instrument sends, a
  line `SECOND ID DATA`, SECOND the module's epochs read before it: an
  instrument's image, with a recording for the module, gives the
  command script it sends, to feed an engine again in a test.
  `--initial-config` powers the engine up as an instrument left its
  receiver configured, for an instrument that cannot command its
  receiver: the Z3817A (a Motorola-BTS timing module, firmware 3752)
  refuses every engine request internally, so it cannot send `@@Ea` and
  would get nothing.  A VP keeps its configuration, the fix mode
  included, over a power cycle (`docs/vp-on-jig.md`, "The mask angle"),
  and smartclock-gps's engine takes it as `Options::saved`
  (`oncore-core`'s `emulator::Configuration`): the host takes
  `--initial-config`, the firmware the `z3817a` feature, the 58503A's,
  smartclock-gps's `CONFIGURATION_58503A`, with the engine deaf.  A
  preset is a SmartClock's own configuration (smartclock
  `docs/firmware/oncore/`, the init scripts; `gps.md`, "Time RAIM", the
  hold scripts): `58503a` (3704, script `0x53`) is eight channels,
  `@@Ea` every second, `@@Bb` on each change and `@@En` every second,
  fixing; `z3801a` (3543, `0x48`) and `z3805a` (3543B, the same scripts)
  the same at six, with `@@Ba` and `@@Bn`.  How the Z3817A's receiver
  was set up is not known; `z3817a` is `58503a`, the owner's choice.  A Time RAIM setup is the
  SmartClocks' factory setting in position hold (`nv.c`, the same in the
  58503A, Z3801A and Z3805A): the algorithm on, 1000 ns, the pulse
  always on.  On the Z3817A, `@@En` with Time RAIM on arms a watchdog:
  past 36 s without one, its GPS record fails (smartclock's reading of
  3752); the engine sends `@@En` and `@@Ea` on the same second, so they
  stop together.  The Z3817A qualified on `@@Ea` every second alone
  (2026-11-11), and on `58503a` (bf6aef8, 18 min, 2026-11-11): GPS valid,
  locked to TIMEREF2, TFOM 3, FFOM 0, `:HARDware:CONDition?` 0; `@@Bb`
  filled El/Az for the tracked and three untracked satellites and
  `:SOUR:GPS:SAT:VIS:PRED:COUNt?` (-230 with `@@Ea` alone, then 10 to
  12); `@@En` with Time RAIM on gave no Time RAIM error and its
  watchdog did not trip.
  Events go to stderr.
- **`oncore-fw`:** the STM32C091FC binary (`thumbv6m-none-eabi`, set in
  its `.cargo/config.toml`), at 48 MHz from the HSI (embassy's
  default divides it by 4): USART2 (PA2/PA3) to the module and USART1
  (PA9/PA10, remapped onto pins 16/17) to J1, both starting at 9600 8N1,
  under `oncore_app::run`; events go out over defmt RTT.  The module's
  receive buffer is 2 KB, a second's messages at 115200, and the
  instrument's transmit buffer 1 KB, so a long answer at 9600 does not
  hold up the module.  STATUS (PA5) lights for 100 ms on each 1 PPS rising edge (PA0).
  RESET_N (PA4) is open drain, held low 10 ms at start, then released.
- Build with `cargo build --release` in `oncore-fw`; `cargo run
  --release` flashes over SWD with probe-rs.  The image carries its
  build identity, `ONCORE-BUILD`, the commit (`git describe --always
  --dirty`), the features and the date, NUL-padded to 96 bytes in
  flash and logged over RTT at start; the instrument never sees it
  (`@@Cj` stays a real Oncore's).  `tools/board_serial.py`, run by
  each `make flash` target, prints it from the board's flash before
  flashing.  `bench/stm32boot.py build` prints it through the ROM
  bootloader, after the knock, for a board with no debug probe.  The
  Makefile sets `SOURCE_DATE_EPOCH` to the commit's date, so a commit builds the same image every time and a board's
  flash can be compared with it byte for byte.  `make images` builds
  each model's production image; CI keeps them from every run as an
  artifact and publishes a tag's as its GitHub release, so the image
  flashed is one CI built.  The release profile
  optimizes for size.  `ut-plus` builds the UT Plus (R5122U1152) in
  place of the VP (`make flash-ut-plus`).  `vp-six` builds the
  six-channel VP alone, which the first `@@Ea` leaves six, for a
  58503A, 58503B or Z3816A with a B1121P1114 (`make flash-vp-six`).  `z3817a` sets the VP up at
  start as a 58503A sets up its own, for a Z3817A, which cannot command
  its receiver (`--initial-config`, below; `make flash-z3817a`).  The default features are the B1121P1114 and
  `rtt`, events and panics over RTT (defmt) for a debug probe; the
  production build leaves `rtt` out, logs nothing and halts on a panic:
  `cargo build --release --no-default-features`.  `nucleo` leaves out USART1's remap from PA11 and
  PA12 to PA9 and PA10, for the NUCLEO-C092RC, whose LQFP64 has PA9 and
  PA10 pins of their own (the remap would turn them analog; RM0490,
  SYSCFG_CFGR1) and whose PA11 and PA12 go to its USB-C connector
  (solder bridges SB6 and SB7 off); it also starts the stack at the
  C092's 30 KB end, where embassy-stm32's memory map is the C091's 36
  KB (`build.rs`).  The STM32C091FC has 256 KB of
  flash and 36 KB of RAM; with the navigation message, orbits, magnetic
  model and pseudorange corrections the production build measures
  103.2 KB of flash (text and data) and 16.9 KB of static RAM (data and
  bss), the `rtt` build 115.7 KB and 17.2 KB (`llvm-size`, Rust 1.99.1,
  smartclock main cb968ece), where the STM32C011F6 it replaced has 32 KB
  and 6 KB.  smartclock-gps-ublox's decoders, every field of each
  message, are about 7.5 KB of that flash.  The ephemerides' subframes,
  decoded, are 2.5 KB of that RAM.  The
  emulator's state (`oncore_app::State`, 11 KB) is a static built at
  compile time, its initial values copied from flash at reset, so it
  never passes through the stack (AGENTS.md, Stack and RAM).  The
  stack's peak was 3.2 KB on b5880f3 (3.5 hours) and 3.4 KB on 3fbab6f,
  whose loop inlines the UBX parser (15 minutes): the `rtt` and
  `nucleo` build on a NUCLEO-C092RC, 2026-10-31, both ports active,
  apu-0's receiver through gpsd on the module port and the Z3801A 3543
  image on the instrument's, through its start-up to
  lock; `-F cortex-m-rt/paint-stack` and the painted words read back
  with `probe-rs read`.  The same run on Rust 1.99.1 (2026-11-01): 3.3 KB,
  where 1.98.1's build of the same code read 3.4 KB.  The B4121P1115 build against the
  58503A 3704 image: locked in 15 minutes, 3.4 KB.  On the board, 2026-11-06,
  the 58503A from power-on to lock, both ports active: 3.28 KB, the `rtt`
  build 3.37 KB.  Under a stress
  run in place of the instrument (every command, in range, out of range
  and as a query; ten `@@Cj` at once; a break and bytes at 4800 for
  framing errors; `@@Cf`; 24 rounds in 10 minutes): 3.6 KB.  That is a
  watermark, not a bound.  The deepest chain traced in the production
  disassembly (2026-10-31) is about 3.4 KB: `run`, `tick`,
  `send_extension` (the WMM), `cos`'s argument reduction and a double
  multiply; an interrupt on top makes about 3.5 KB, 3.75 KB in the
  `rtt` build with its UART-error logging.  Panic formatting is not
  bounded.  `make stack` requires 6 KB of room for the stack in every
  build (AGENTS.md, Stack and RAM): the C091 leaves 19.0 KB, the C092
  13.0 KB.

## Choosing the module

Datasheets are in `third_party/u-blox/`.  Prices were checked
2026-10-12 against distributor pages and move; "checked" means seen
on a distributor or u-blox document that day, "unchecked" means taken
from a secondary write-up and not confirmed.

### u-blox

| Part | Bands | 1σ time pulse | Status | Price (single unit) |
|---|---|---|---|---|
| ZED-F9T-20B | L1 + L2 or L5 (one pair at a time), GPS/GAL/BDS | 5 ns absolute, 2.5 ns differential, jitter ±4 ns | Mass production, datasheet R04 2026-05-28 | Digi-Key EU about €172 (checked); a $199 USD listing was a test storefront with no stock; US price not confirmed |
| LEA-F9T-10B / -20B | L1/L2 or L1/L5 | about 5 ns (unchecked) | Active | Digi-Key US $259, about 1,900 in stock (checked); EU about €235 |
| LEA-F9T-11B / -21B | as above | not checked | New at Mouser and Digi-Key | about €233-242 at Mouser (checked); how they differ from -10B/-20B not found |
| NEO-F10T-00B | L1 + L5 (GPS, GAL, BDS, NavIC) | 10 ns; 5 ns with jitter removed; jitter ±8 ns; 16 ns resolution | Mass production, datasheet R06 2026-04-28 | $160 (checked on aggregators; Digi-Key page says discontinued at Digi-Key) |
| ZED-F9T-00B / -10B | L1/L2 or L1/L5 | 5 ns | -10B marked NRND at Mouser; -20B replaces both | not checked |
| NEO/LEA-M8T | L1 only | under 20 ns | u-blox's series page says no longer available; distributors still stock LEA-M8T-0 and no EOL notice found | $96.15 at Digi-Key (reported by the maintainer, not seen by me) |

Corrections to what was first written up:

- The F9T-20B's band choice is L1+L2 or L1+L5, configured at run
  time, not all three at once (UBXDOC-963802114-12681 R04, footnote
  11).  No GLONASS
  appears in its acquisition table.
- It is rated -40 to +85 °C.  The +105 °C rating belongs to the
  LEA-F9T-10B, not the ZED.
- The 10 ns NEO-F10T figure is confirmed in its datasheet
  (UBX-22022576 R06); it is
  roughly twice the F9T's.  It has one UART and no USB.
- GLONASS and L5 on the F10T, and Galileo OSNMA on the F9T-20B: not
  found in the datasheet text read, so not asserted.
- The 5 ns is absolute only after entering the antenna, cable and
  front-end delay.  Uncalibrated, expect tens of ns of bias.
- None of these parts takes an external reference oscillator.  The
  1 PPS sits on the internal TCXO grid, so the TIM-TP qErr
  (sawtooth) correction is what the translator carries into the
  Oncore sawtooth field.

### Not u-blox (unchecked unless noted)

- **Septentrio mosaic-T:** takes an external 10 MHz reference, so the
  sawtooth problem goes away; 5 ns, event input under 20 ns;
  500 mA module maximum (checked).  SparkFun $849.95, $722-807 in
  volume (checked); GNSS Store price not checked.
- **Trimble/Protempis RES 720:** L1/L5, under 5 ns, TSIP; price on
  request.
- **Furuno GT-100 / GT-9001:** headline 4.5 ns is stability, absolute
  is about ±40 ns; good multipath rejection.
- **Quectel LC99T / LC29T:** 13.6 ns 1σ; cheap.
- **SkyTraq PX1120T:** L1 only, 5 ns claimed from a preliminary
  datasheet.

An L1/L5 patch antenna such as the ANN-MB1 cannot serve the L2
configuration; choose the band plan first, then the antenna.

## Open questions

Numbered as cited elsewhere (the schematic's notes, `docs/`).

3. The Oncore socket's connector, pinout, supply and 1 PPS level and
   polarity, against the VP and UT+ engineering notes
   (`third_party/motorola/`).  Synergy (TN 484-D, TN 502-C) gives the
   VP, UT+ and GT+ one pinout and labels pin 8 TTL TxD, but describes
   it as "Receiver Command Input", against Motorola's "primary output"
   (UT+ notes); the UT+ notes' idle levels (pin 8 high, pin 9 low) fit
   Motorola's, and the board is wired that way.  Measure the
   instrument's idle levels to confirm it.

A bug in the emulator shows in every log afterwards as oscillator
behavior, so the first test is one receiver, one antenna, its week of
MDEV and ADEV before and after.
