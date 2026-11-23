# Firmware

Rust on Embassy.  `PLAN.md` has the design.

- `oncore-core`: the emulated Oncore, `no_std`: the record that
  smartclock-gps's Oncore engine answers from, built from the module's
  UBX as smartclock-gps-ublox decodes it; the module's set-up, channel
  selection, Time RAIM, the GPS almanac and ephemeris, pseudorange
  corrections and the magnetic model.
- `oncore-app`: the emulator running between the module and the
  instrument, on the STM32 or a host.
- `oncore-fw`: the STM32C091 binary.
- `oncore-host`: the emulator on a PC, with the module on a serial port,
  a recording (to its end) or gpsd, and the instrument on a serial port, a pty or
  TCP; `--model vp|vp-six|ut-plus` the engine; `--initial-config 58503a|z3801a|z3805a|z3817a` powers the engine
  up as those instruments leave their receivers configured, for one that
  cannot command its receiver (a Z3817A); `--record-script PATH` writes every command the
  instrument sends with the module's epochs before it.
- `testdata/`: fifteen minutes of a NEO-M8T, for replay.

```
cargo test                     # every host crate, with the trace replay
cargo run -p oncore-host -- --module file:testdata/neo-m8t-2026-10-28.ubx --oncore /dev/pts/N

cd oncore-fw
cargo build --release          # with RTT logging (defmt)
cargo run --release            # flash over SWD (probe-rs), staying attached
cargo build --release --no-default-features   # production
cargo run --release --example timing   # the emulator's work, timed

cd ../..   # the repository root
make lint                      # formatting, clippy on every build, stack frames 1.5 KB at most
make flash                     # the production build onto the board on the probe
make flash-rtt                 # the logging build, staying attached for its log
make flash-ut-plus             # the production build as the UT Plus (58503B, Z3816A)
make flash-vp-six             # the production build as the six-channel VP alone
make flash-z3817a              # the production build for a Z3817A, set up as a 58503A's
make images                    # each model's production image, ELF and .bin, and SHA256SUMS
```

The firmware presents a B1121P1114 (six channels, the Z380x's) from
power-on and a B4121P1115 (eight, the 58503A's) from the first `@@Ea`;
`-F vp-six` the B1121P1114 alone, `-F ut-plus` the UT Plus
R5122U1152 (58503B, Z3816A), `-F z3817a` a Z3817A's.
`-F nucleo` builds for a NUCLEO-C092RC: the instrument's port (USART1) on
PA9 and PA10, the module's (USART2) on the ST-LINK's virtual COM port,
the stack at the C092's 30 KB end; flash it with `probe-rs run --chip
STM32C092RCTx`.

## Updating a board

A release image (`oncore-fw-*.bin` from the GitHub release, or `make
images`) goes on over J3 or the jig's USB through the STM32's ROM
bootloader, with no debug probe: `bench/stm32boot.py write --knock
--go DEVICE IMAGE.bin` (`bench/README.md`).  Bench only, never in an
instrument.  Over SWD: `make flash`.
