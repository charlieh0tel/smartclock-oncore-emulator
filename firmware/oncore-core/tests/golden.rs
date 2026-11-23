//! The emulator's whole Oncore output, byte for byte, against golden
//! logs: the trace (`../testdata/neo-m8t-2026-10-28.ubx`) replayed as
//! `oncore-app` runs it, through smartclock-gps's engine as each image's
//! Oncore with the command script captured from its image
//! (`../testdata/golden/*.script`), every frame in and out logged
//! (`../testdata/golden/*.log`, `testdata/README.md`).  A change that
//! moves a byte fails here; one meant to is recorded again with
//! `ONCORE_GOLDEN=update cargo test --test golden`.

use smartclock_gps::engine::ReceiverControl;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use oncore_core::emulator::Configuration;
use oncore_core::emulator::Emulator;
use oncore_core::emulator::SELECT_EPOCHS;
use oncore_core::gnss::Gnss;
use oncore_core::identity::Identities;
use oncore_core::module::Module;
use smartclock_gps::engine::Output;
use smartclock_gps::oncore::MessageId;
use smartclock_gps::oncore::OVERHEAD;
use smartclock_gps::oncore::encode;
use smartclock_gps::units::Milliseconds;
use smartclock_ubx::message::NAV_EOE;

const TRACE: &[u8] = include_bytes!("../../testdata/neo-m8t-2026-10-28.ubx");
/// The golden files' directory, from this crate's.
const GOLDEN: &str = "../testdata/golden";
/// The environment variable, and its value, that records the logs again.
const UPDATE_VAR: &str = "ONCORE_GOLDEN";
const UPDATE: &str = "update";
/// An epoch's length, as the engine's clock reads it.
const EPOCH_MS: u64 = 1000;
/// The longest frame the engine sends, `@@Cj`'s 294 bytes, with room.
const FRAME_ROOM: usize = 512;
/// Hex: a byte's digits, and the radix.
const HEX_DIGITS: usize = 2;
const HEX_RADIX: u32 = 16;
/// A script line's comment mark.
const COMMENT: char = '#';

/// One command of a script: the epochs before it, fed after that
/// epoch's second (0 before the first), its ID and data.
struct Command {
    second: usize,
    id: MessageId,
    data: Vec<u8>,
}

/// Parses a script: `SECOND ID DATA` a line, DATA hex or absent; blank
/// lines and `#` comments skipped.
fn script(text: &str) -> Vec<Command> {
    text.lines()
        .map(|l| l.split(COMMENT).next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut fields = l.split_whitespace();
            let second =
                fields.next().and_then(|s| s.parse().ok()).unwrap_or_else(|| panic!("{l:?}: a second"));
            let id = match fields.next().map(str::as_bytes) {
                Some(&[a, b]) => MessageId::new([a, b]),
                _ => panic!("{l:?}: a two-letter ID"),
            };
            let data =
                fields.next().map_or_else(Vec::new, |h| hex(h).unwrap_or_else(|| panic!("{l:?}: hex")));
            Command { second, id, data }
        })
        .collect()
}

/// Bytes from hex digits, two a byte.
fn hex(s: &str) -> Option<Vec<u8>> {
    s.len().is_multiple_of(HEX_DIGITS).then_some(())?;
    (0..s.len())
        .step_by(HEX_DIGITS)
        .map(|i| u8::from_str_radix(s.get(i..i + HEX_DIGITS)?, HEX_RADIX).ok())
        .collect()
}

/// The log being written: each frame a line, `SECOND in|out HEX`.
struct Log {
    second: usize,
    text: String,
}

impl Log {
    /// A whole frame, `direction` `in` or `out`.
    fn line(&mut self, direction: &str, frame: &[u8]) {
        write!(self.text, "{} {direction} ", self.second).unwrap();
        for b in frame {
            write!(self.text, "{b:02x}").unwrap();
        }
        self.text.push('\n');
    }

    /// The engine's clock at this second.
    fn now(&self) -> Milliseconds<u64> {
        Milliseconds(self.second as u64 * EPOCH_MS)
    }
}

impl Output for Log {
    fn send(&mut self, frame: &[u8]) {
        self.line("out", frame);
    }
}

/// The trace through an emulator answering as `identities`, with
/// `commands` fed, as `oncore-app` runs it, channel selection and the
/// corrections run whole: the log.
fn replay(identities: Identities, commands: &[Command]) -> String {
    let mut emulator =
        Box::new(Emulator::new(identities, Configuration::FACTORY, ReceiverControl::Commanded));
    let (mut gnss, mut module) = (Gnss::new(), Module::new());
    let mut log = Log { second: 0, text: String::new() };
    let feed = |emulator: &mut Emulator, log: &mut Log| {
        let second = log.second;
        for c in commands.iter().filter(|c| c.second == second) {
            let mut frame = [0u8; FRAME_ROOM];
            let n = encode(c.id, &c.data, &mut frame).expect("a frame fits");
            assert_eq!(n, c.data.len() + OVERHEAD);
            log.line("in", &frame[..n]);
            let now = log.now();
            emulator.command(&frame[..n], now, log, &mut |_| {});
        }
    };
    feed(&mut emulator, &mut log);
    for &b in TRACE {
        if gnss.push(b, &mut *emulator, &mut module) != Some(NAV_EOE) {
            continue;
        }
        log.second += 1;
        let now = log.now();
        emulator.poll(now, &mut log);
        if log.second % SELECT_EPOCHS as usize == 1 {
            emulator.alert();
            while emulator.background() {}
        }
        emulator.second(now, &mut log, &mut |_| {});
        while emulator.background() {}
        // What the instrument sent during the second, before the next
        // epoch.
        feed(&mut emulator, &mut log);
    }
    log.text
}

/// Replays `name`'s script through `identities` and compares its log,
/// or records it with `ONCORE_GOLDEN=update`.
fn check(name: &str, identities: Identities) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    let commands = script(&fs::read_to_string(dir.join(format!("{name}.script"))).expect("the script"));
    let got = replay(identities, &commands);
    let path = dir.join(format!("{name}.log"));
    if std::env::var(UPDATE_VAR).is_ok_and(|v| v == UPDATE) {
        fs::write(&path, &got).expect("the log written");
        return;
    }
    let want = fs::read_to_string(&path).expect("the golden log");
    if let Some((n, (g, w))) = (1..).zip(got.lines().zip(want.lines())).find(|(_, (g, w))| g != w) {
        panic!("{name}.log line {n}:\n  got  {g}\n  want {w}");
    }
    assert_eq!(got.lines().count(), want.lines().count(), "{name}.log: line count");
}

#[test]
fn hp58503a_vp() {
    check("58503a", Identities::VP);
}

#[test]
fn z3801a_vp() {
    check("z3801a", Identities::VP);
}

#[test]
fn hp58503b_ut_plus() {
    check("58503b", Identities::UT_PLUS);
}

#[test]
fn z3816a_ut_plus() {
    check("z3816a", Identities::UT_PLUS);
}
