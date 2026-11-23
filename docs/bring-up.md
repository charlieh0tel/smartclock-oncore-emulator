# Bring-up

Jig 1.0.0 first, with a real VP Oncore as the known engine; then the
emulator board 1.0.0 on the jig; then in an instrument.  Parts and nets
are the schematics'; J1's pins are in `docs/oncore-ten-pin.md`.

## Jig

1. Fit by hand: J3 to J6, J9 to J11 (one of BNC or SMA per output).
   Shim the standoffs (H1 to H4) with M3 washers, about 0.5 mm, so the
   engine seats on J2.  JP1 1-2: +5V from USB.
2. USB alone, no engine: U2 (CH9102F) enumerates; its serial is the
   chip's unique ID.  +5V at J2 pin 2.  D1 lit.
3. A VP Oncore on J2.  J9 in mV is the engine's current in tens of mA
   (1 mV is 10 mA): a VP 275 mA at most, plus its antenna's.  On a weak
   USB port use a bench supply on J5: cut JP1 1-2, close 2-3.
4. At 9600 8N1 on U2's port: `@@Cj` answers; `@@Ca` passes.  `@@Ca`
   leaves a VP in Idle, silent until `@@Cg` 1 (VPCommands.pdf p. 107).
   1 PPS at J3/J4 or J10/J11 into 50 ohm, about 2.5 V.  J6 taps the
   engine's RXD1 (pin 1) and TXD1 (pin 2) for a capture (`@@Bb`'s
   cadence, TODO.md), GND on pin 3.

## Emulator board

1. Check U1's and U2's orientation, D8's polarity, J2 on the bottom.
   Fit J1 on the side the host needs (the jig: bottom).  No cell.
2. USB-C (J3) from a computer's port, the board out of any instrument:
   +5V about 4.4 to 4.7 V (D8's drop), +3V3 from U4, VDD_USB from U10.
   Record the current.
3. The module alone over USB (VID 1546): MON-VER says TIM 1.10
   (`docs/ublox-lea-m8t-version.md`); a fix with an antenna on J2.  The
   STM32 blank, the module is at its defaults.
4. SWD on J4 (TC2030):
   `probe-rs info --chip STM32C091FC`, then `make flash-rtt` (the `rtt`
   build, staying attached).  RTT: `module moved from
   9600 baud` on the first boot, no `no answer`, then `module set up`;
   at the next boot `module at 115200 baud`.  Framing errors before the
   first boot's move.  D2 (STAT) flashes on each 1 PPS, D6 pulses on
   it.  Results: `docs/emu-board-bring-up.md`.
5. On the jig: J9's current against the VP's.  Between instruments, back
   to power-on with `@@Zr` and the argument `BOOT` (`docs/oncore-messages.md`)
   or a power cycle: channels, position hold and rates are in RAM.  At 9600 on U2's port,
   the board answers as the VP did (`@@Cj`'s serial is `EMU` and
   digits from the STM32's unique ID).  1 PPS at J1 pin 6: 5 V, rising
   on the second, 200 ms high.  Against a VP on a second jig, on a
   counter: `@@Ay`'s direction and qErr's sign (TODO.md, "Check on a
   module").
6. `make flash` (the production build) and fit it in an instrument:
   `:DIAG:TEST:RES?` gives `+0`, `:SYST:STAT?` reaches "Locked to GPS".
   Then holdover and recovery: the antenna off, the instrument to
   holdover; on again, back to "Locked to GPS".  A reset of the STM32
   mid-run (the probe, or NRST): the instrument rides through.  The
   module takes the set-up and `configure`'s settings: read them back
   with u-center or `ubxtool` over the module's USB.
