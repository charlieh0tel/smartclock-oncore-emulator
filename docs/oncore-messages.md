# Oncore messages the emulator implements

The messages a VP Oncore in a Z3801A (3543), Z3805A (3543B) or 58503A
(3633, 3704) is sent, and what each needs from the engine.  Formats
are the VP Oncore Command Reference's
(`third_party/motorola/VPCommands.pdf`).  How the images frame, match
and time out messages, and what they do with Time RAIM, is in
smartclockmon's `docs/firmware/gps.md` on its `main`, the single account
of how these images talk to their Oncore: "Emulating the engine" and
"Channel mode and signal strength" above all.  That account, read from the images' C and
ROMs, settles what the reference leaves ambiguous; the points it
settles are listed below.

## Models

| Model | Channels | Software | Instruments |
|---|---|---|---|
| B1121P1114 | 6 | 8.4, options IB | Z3801A, Z3805A |
| B4121P1115 | 8 | 8.8, options IB | 58503A |

The firmware is the B1121P1114 from power-on and the B4121P1115 from the
first `@@Ea`, until power-off.  The 58503A polls `@@Ea` at power-up,
before its `@@Cj`, and switches to eight-channel messages once one
answers (smartclock's `docs/firmware/gps.md`, the order partly inferred
there); the Z380x's images do not hold `@@Ea`.  Software 8.8 keeps the
58503A from sending its second `@@Ar` (argument 65, undefined in the
reference), which goes only to 8.4.

## The messages

Models: "both" is the two VPs, B1121 and B4121; "UT Plus" the
R5122U1152 (`Identities::UT_PLUS`, smartclock-gps's `UT_PLUS_3_1`),
which takes only the messages so marked, its lengths tested against the 58503B's and Z3816A's tables.  It never
sends `@@Sz`, which those tables frame at 7 bytes and TRM0003 gives at
8.  Its `@@Ea` (TRM0003 pp. 6.18-6.20) has no MSL height (0), the
antenna's state (too little current, open; too much, short) beside the
DOP type in byte 37, C/N0 itself, dB-Hz, as each channel's signal, a
"used for time" channel bit (bit 1, a satellite giving position not
isolated by Time RAIM), the visible count at a 0 degree mask and poor
geometry at DOP over 12; its `@@Fa` (p. 6.43) reports the antenna in
bits 15 and 14 and leaves it acquiring, not idle.  Its automatic survey
(`@@At` 2, `@@Ea`'s bit 5) is not emulated: the 58503B and Z3816A never
ask for it.  `@@Gj`'s date and time are the day after the change's at 00:00:00,
as an M12 reported 2017-01-01 for the leap second at the end of
2016-12-31 (the time inferred), and zeros with none scheduled.

Lengths are whole messages, `@@` to LF.  "Rate" is the reference's
output rate byte: 0 sends the message once, 1 to 255 every that many
seconds.

| ID | What | Command | Answer | Models |
|---|---|---|---|---|
| `Aa` | time of day | 10 | `Aa`, 10 | both, UT Plus |
| `Ab` | GMT offset | 10 | `Ab`, 10 | both, UT Plus |
| `Ac` | date | 11 | `Ac`, 11 | both, UT Plus |
| `Ad`, `Ae` | latitude, longitude | 11 | the same, 11 | both, UT Plus |
| `Af` | height | 12 | `Af`, 15 (GPS and MSL heights) | both, UT Plus |
| `Ag` | mask angle | 8 | `Ag`, 8 | both, UT Plus |
| `Ah` | satellite select | 8 | `Ah`, 8 | both |
| `Ai` | manual satellite select | 9 | `Ai`, 9 | both |
| `Aj` | xDOP type | 8 | `Aj`, 8 | both |
| `Ak`, `Al` | xDOP hysteresis, 3-D to 2-D threshold | 9 | the same, 9 | both |
| `Am` | satellite ignore list | 12 | `Am`, 12 | both, UT Plus |
| `An` | almanac update | 8 | `An`, 8 | both |
| `Ao` | datum | 8 | **`Ap`**, 25 | both, UT Plus |
| `Aq` | atmospheric correction: off, ionosphere; on the UT Plus also troposphere, both (TRM0003 p. 6.29; the 58503B and Z3816A send both) | 8 | `Aq`, 8 | both, UT Plus |
| `Ar` | position fix algorithm | 8 | `Ar`, 8 | both |
| `As` | position-hold position | 20 | `As`, 20 | both, UT Plus |
| `At` | position hold on/off | 8 | `At`, 8 | both, UT Plus |
| `Au` | altitude-hold height | 12 | `Au`, 12 | both (58503A sends it) |
| `Av` | altitude hold | 8 | `Av`, 8 | both |
| `Aw` | UTC or GPS time | 8 | `Aw`, 8 | both, UT Plus |
| `Ax` | measurement epoch offset | 9 | `Ax`, 9 | both |
| `Ay`, `Az` | 1 PPS offset, cable delay | 11 | the same, 11 | both, UT Plus |
| `AB` | application type | 8 | `AB`, 8 | both |
| `AC` | 2-D to 0-D HDOP threshold | 9 | `AC`, 9 | both |
| `AD` | correction thresholds | 9 | `AD`, 9 | both |
| `Ba` | 6-channel position, status, data | 8, rate | `Ba`, 68 | both |
| `Bb` | visibility | 8, mode | `Bb`, 92 | both, UT Plus |
| `Bc` | 6-channel xDOP table | 8, mode | `Bc`, 82 | both |
| `Bd` | almanac status | 8, mode | `Bd`, 23 | both |
| `Bh` | pseudorange correction output | 8, rate | **`Ce`**, 52; a second for channels 7 and 8 | both |
| `Bj` | leap second pending | 8 | `Bj`, 8 | both, UT Plus |
| `Bk` | 6-channel position extension | 8, rate | `Bk`, 69 | both |
| `Bn` | 6-channel Time RAIM | 22 | `Bn`, 59 | both |
| `Ca` | 6-channel self test | 7 | `Ca`, 9 | both |
| `Cf` | set to defaults | 7 | `Cf`, 7 | both, UT Plus |
| `Cg` | idle or position fix | 8 | `Cg`, 8 | both |
| `Ci` | switch I/O format | 8 | none | both |
| `Cj` | receiver ID | 7 | `Cj`, 294 | both, UT Plus |
| `Ea` | 8-channel position, status, data | 8, rate | `Ea`, 76 | B4121, UT Plus |
| `Ek` | 8-channel position extension | 8, rate | `Ek`, 71 | B4121 |
| `En` | 8-channel Time RAIM; the UT Plus's has ten bytes not used, 0, for the VP's pulse rate and next-pulse time (TRM0003 pp. 6.93-6.94) | 22 | `En`, 69 | B4121, UT Plus |
| `Fa` | 8-channel self test | 7 | `Fa`, 9 | B4121, UT Plus |
| `Bo` | UTC offset, GPS less UTC, s, 0 until known (TRM0003 pp. 6.80-6.81) | 8, mode | `Bo`, 8 | UT Plus |
| `Gj` | leap second pending (M12+ User's Guide pp. 152-153; above) | 7 | `Gj`, 21 | UT Plus |
| `Ta` | DCXO Offs, Warp and Step: not in the reference; the 58503B 1.01.04's, query five `0xff` (below) | 12 | `Ta`, 12, zeros | both |
| `Zr` | the bench's reset, argument `BOOT`: not an Oncore message, in neither the reference nor the images | 11 | `Zr`, 11, then the STM32 resets | both, UT Plus |

`Bb`, `Bc` and `Bd` take a mode, not a rate: 0 sends the message once,
1 again whenever it changes, `Bb` checked at each satellite alert
computation (every 5 epochs) and listed by PRN.  `Bd`'s EEPROM almanac is the RAM's, as a
real engine's is once its background copy is done (p. 78); the
emulator keeps no almanac in nonvolatile memory.  No image reads
`Bd`'s record: only its decoder touches it, and nothing but the
console asks for it (smartclockmon, the hand-written C and a ROM scan
of all four images).
`Bh`'s corrections are a reference station's, against the module's
position (PLAN.md, "Pseudorange corrections").  `Bd` and `Bh` are in
every image's table, sent only from the console.

## `@@Ta`, the 58503B's DCXO

From the 58503B 1.01.04's ROM (smartclock's reading, message table
entry 0x39 at 0x58ae0; not run on an instrument yet).  It sends `@@Ta`
only to an engine whose `@@Cj` model starts with `B` (a VP), after
`@@Cj` in its init and in its polling list, and takes any well-framed
12-byte `@@Ta` as the answer: Offs (two bytes, big-endian), Warp (one),
Step (two, big-endian).  Without one it sends three times, 10 s apart,
then restarts its init, without limit.  The values feed only its
status screen ("DCXO FREQ  Offs %d  Warp %d  Step %d") and an
undocumented SCPI query, `DCXO`, which returns an error when all three
are zero.  The module has no DCXO, so the engine answers zeros to a
query or a set.  Check: the 58503B's boot gets past `@@Cj`.

## What the images settle

- **`@@Ao` is answered by `@@Ap`**, where the reference gives `@@Ao`.
  The images take only `@@Ap`, and have no length for `@@Ao`.
- **`@@Bh` is answered by `@@Ce`**, as the reference gives; the images
  frame `@@Ce`, 52 bytes, and have no length for `@@Bh`.
- **Never send `@@Ao`, `@@Bh` or `@@Ci`.**  Their table length is 0,
  and the framer would run on for about 65530 bytes.
- **`@@Cg` is 8 bytes**, with its mode byte; the reference says 7.
- **Idle (p. 118):** after `@@Ca`, `@@Fa` or `@@Cf` the engine sends nothing
  on its own until `@@Cg` 1, and answers commands; `@@Ba` and `@@Ea` polls
  get the last position.  The images send `@@Cg` 1 after each.  `@@Cf`
  also sets every continuous output to polled (p. 116); the self test
  keeps them.
- **`@@Ci` 0**, the Z380x's first command, is a format the reference
  does not define.  The engine stays in binary and does not answer.
- **Echoes are byte for byte**, checksum included, with the exceptions
  for `Ab`, `Af`, `As` and `Cg` in `gps.md`, "Answer matching".
- **`@@Cj`:** each line starts with its label, and `OPTIONS LIST`
  holds `I`, or the images send no Time RAIM.
