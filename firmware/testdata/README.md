# Test data

## neo-m8t-2026-10-28.ubx

Fifteen minutes of a real module's UBX, for replay: `oncore-host
--module file:...` plays it an epoch a second, `oncore-core`'s
`tests/trace.rs` runs it through the emulator, and `oncore-fw`'s timing
example takes its first two minutes.

- Receiver: a NEO-M8T-0, TIM 1.10, protocol 22.00, the same firmware as
  the board's LEA-M8T-0, on USB under gpsd 3.25, in fixed-position
  timing mode (time-only fixes).
- Recorded 2026-10-28 from about 00:36 to 00:51 UTC, through gpsd's raw
  stream (`?WATCH={"enable":true,"raw":2}`), and cut to whole UBX
  messages with good checksums; gpsd's JSON lines were left out.
- Messages, each once an epoch: NAV-POSECEF, NAV-DOP, NAV-PVT,
  NAV-VELECEF, NAV-TIMEGPS, NAV-CLOCK, NAV-TIMELS, NAV-SAT, NAV-EOE,
  RXM-RAWX, MON-HW, TIM-TP (900 of each; 901 MON-HW); and RXM-SFRBX,
  1650 subframes from every system, the GPS ones about 330 each of
  subframes 1 to 5, so the almanac's full 12.5-minute cycle.  12451
  messages, 911708 bytes.
- Moved from where it was recorded to the SmartClock's default
  position, so it does not locate the receiver: 134372100 mas N,
  -439192200 mas E, 55.00 m MSL (`@@Ad`, `@@Ae`, `@@Af` in the
  Z3801A's and Z3805A's default settings script, smartclockmon), by
  `oncore-host`'s `move_trace` (its doc comment says what each message
  becomes).  The ellipsoid height keeps the recording's geoid
  separation.  RXM-RAWX and NAV-SAT keep only the GPS satellites with an
  ephemeris in the subframes (976 records dropped); NAV-DOP is computed
  again from them.  Checked: every pseudorange correction the engine
  computes, 9640 over 871 epochs, is the original's within 5.7 mm (the
  new position rounded to cm in NAV-POSECEF) and 0.05 mm/s.  The
  original recording is not in the tree.
- SHA-256 9008222af77db50e85fc10dff409e5c14c24f956cff157f43bfccba5351ad3c9.

## golden/

The emulator's whole Oncore output for the trace above, smartclock-gps's
engine answering (`oncore-core/src/emulator.rs`), a log an instrument
image, compared byte for byte by `oncore-core`'s `tests/golden.rs`: a
change that moves a byte fails it, and one meant to is recorded again
with `ONCORE_GOLDEN=update cargo test --test golden` and checked by its
diff.

- `58503a` (3704) and `z3801a` (3543) on the VP (smartclock-gps's
  `VP_SIX`, the 58503A to `VP_EIGHT` by its `@@Ea`); `58503b` (1.01.04)
  and `z3816a` (4001) on the UT Plus (`UT_PLUS_3_1`).  `@@Cj` carries
  the presets' serial number and date: the board's replace them in the
  firmware.
- The board commands its module, so the engine reports hold as the
  module shows it (smartclock-gps `ReceiverControl::Commanded`).  The
  trace's module was in time mode (NAV-PVT fixType 5) throughout, so the
  logs report hold (status bit 3, DOP 0, Time RAIM able to isolate)
  every second, also after a UT Plus image's `@@At 0`.  On the board the
  set-up turns CFG-TMODE2 off and `@@At 0` is passed on to the module,
  so there those seconds report navigating.
- `NAME.script`: the commands the image sent, captured from the image
  through `oncore-host --record-script` with this trace as the module
  (2026-11-13, at 01db9a3; the logs recorded again when the UT Plus's
  `@@En` took TRM0003's unused bytes), `SECOND ID DATA` a line, DATA hex or none,
  `#` comments.  SECOND is the epochs before the command; the test feeds
  it after that epoch's second.  The VP images' stored position was set
  to the trace's (their `@@As`); their learned settings were kept, with
  position hold on from boot (`@@At` 1).
- Each script is one sample of what its image sends: between runs a
  command's epoch moved by up to one or two, and a late group (`@@Aw`,
  `@@Ea` or `@@Ba`, `@@Aw`, `@@Bj` about epochs 545 to 565) came in some
  runs and not others, as did the UT Plus images' late `@@Bj` and
  `@@Bo`; why is not known.
- `NAME.log`: every Oncore frame, in and out, a line, `SECOND in|out
  HEX`, the frame whole from `@@` to CR LF.  Per epoch: answers held
  back (the self-test's 2 s), the alert computation and channel
  selection every `SELECT_EPOCHS`th epoch, the second's continuous
  outputs, then the commands and their answers, as `oncore-app` runs
  them.  The order within a second is the engine's; a real Oncore's is
  not known, and may vary.
- Recorded on smartclock-gps's engine (470e0ec3, 2026-11-16); the same
  byte for byte with the module's messages decoded by
  smartclock-gps-ublox (cb968ece).  Against
  the old engine's logs, the settings' echoes are byte for byte the
  same; the rest differs as follows, each reviewed against the VP and
  UT Plus manuals and the jig's captures (`docs/vp-on-jig.md`):
  - The channels: our selector and smartclock-gps's engine take a
    satellite at the mask angle, as a B4121 did on whole degrees
    (`docs/vp-on-jig.md`, "The mask angle"), where VP p. 27 says
    "greater than".
  - `@@Bb` goes each alert computation, every 6 s, where the old engine
    went every 5 s, and lists the satellites at or above `@@Ag` on the
    VP, as SSG0220999 did at `@@Ag` 0, 10 and 20 (2026-11-15), and at or
    above the horizon on the UT Plus (UT p. 6.23).
  - `@@Ca`/`@@Fa` answer 6 s after, where the old engine answered at
    once (real VPs 5 to 7 s); the contents, all passing, are the same.
    The scripts were captured against the immediate answer, so their
    `@@Cg` 1 comes before it.
  - The UT Plus takes the eight highest satellites (UT p. 6.17), so its
    channels differ from the VP's choice.
  - Visible 0 and bad almanac (`@@Ea`/`@@Ba` receiver status) until
    about epoch 350, when the almanac is decoded from the trace's
    subframes; the module's own almanac is not yet given to the record.
  - Idle: a VP starts idle, as SSG0220999 did from a cleared state
    (2026-11-15), and an idle poll answers the last valid status.
- Time RAIM takes every satellite the module used, on a channel or
  not: `@@Bn`'s and `@@En`'s one-sigma estimate is theirs.
- The VP's two clocks (smartclock-gps a6ebb19d, `docs/vp-on-jig.md`):
  the 58503A's first `@@Ea` poll, idle, reads the VP's power-up time,
  1997-01-01 12:00:00 GPS (11:59:42 UTC); polls after the scripts'
  `@@Aw` 0 read GPS time.  The 58503A idles with `@@Cg` 0 at second 4
  and resumes with `@@Cg` 1 at second 6; idle, its status time holds
  where it went idle (GPS 00:36:41, as SSG0220999 held after `@@Cf`),
  its `@@Bn` at second 6 that time's next second (a real VP's idle
  `@@Bn` not measured), and its `@@Ea` follows the fix again at second
  7.
