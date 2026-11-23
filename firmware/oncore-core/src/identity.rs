//! The board's own serial number and manufacture date, which `@@Cj`
//! reports, and the GPS week of its build.

use core::iter::repeat;
use core::str::from_utf8;

use smartclock_gps::engine::identity::Identity;
use smartclock_gps::engine::identity::UT_PLUS_3_1;
use smartclock_gps::engine::identity::VP_EIGHT;
use smartclock_gps::engine::identity::VP_SIX;

use crate::civil::MONTHS_PER_YEAR;
use crate::civil::UnixSeconds;
use crate::civil::days_from_civil;
use crate::module::DEFAULT_WEEK_ROLLOVER_BASE;
use crate::units::GpsSeconds;
use crate::units::GpsWeek;

/// The serial number's and the manufacture date's lengths, characters.
pub(crate) const SERIAL_LEN: usize = 10;
pub(crate) const MANUFACTURED_LEN: usize = 4;

/// The serial number and manufacture date `@@Cj` reports, as the
/// engine's 10 and 4 characters (the bench engines': `SSG0163878`,
/// `6G09`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardIdentity {
    pub(crate) serial: [u8; SERIAL_LEN],
    pub(crate) manufactured: [u8; MANUFACTURED_LEN],
    /// The GPS week of the build.
    pub built_week: GpsWeek,
}

/// The emulator's serial number, before its digits, and the 32-bit FNV-1a
/// hash's offset basis and prime, which make them from the board's ID.
const SERIAL_UNSET: [u8; SERIAL_LEN] = *b"EMU0000000";
const SERIAL_PREFIX: usize = 3;
const FNV_OFFSET_BASIS: u32 = 0x811c_9dc5;
const FNV_PRIME: u32 = 0x0100_0193;
const DECIMAL: u32 = 10;

/// `n`'s last decimal digit, as a character.
fn digit(n: u32) -> u8 {
    b'0' + (n % DECIMAL) as u8
}

impl BoardIdentity {
    /// Before the board's unique ID and build date are known.
    pub const EMULATOR: Self =
        Self { serial: SERIAL_UNSET, manufactured: *b"0000", built_week: DEFAULT_WEEK_ROLLOVER_BASE };

    /// `EMU` and seven digits from a unique ID; the date as the engines
    /// give it, the year's last digit, the month as A to L, the day.
    pub fn new(unique: &[u8], year: u16, month: u8, day: u8) -> Self {
        let hash = unique.iter().fold(FNV_OFFSET_BASIS, |h, &b| (h ^ u32::from(b)).wrapping_mul(FNV_PRIME));
        let mut serial = SERIAL_UNSET;
        let mut n = hash;
        for c in serial[SERIAL_PREFIX..].iter_mut().rev() {
            *c = digit(n);
            n /= DECIMAL;
        }
        let manufactured = [
            digit(year.into()),
            b'A' + month.clamp(1, MONTHS_PER_YEAR) - 1,
            digit(u32::from(day) / DECIMAL),
            digit(day.into()),
        ];
        let built =
            GpsSeconds::from_unix(UnixSeconds::from_days(days_from_civil(i64::from(year), month, day)));
        let built_week = GpsWeek::containing(built).unwrap_or(DEFAULT_WEEK_ROLLOVER_BASE);
        Self { serial, manufactured, built_week }
    }
}

/// The width of each receiver ID line after the first (VP p. 122); the
/// lines that carry the serial number and manufacture date, in every
/// smartclock-gps preset, and their labels.
const LINE_WIDTH: usize = 26;
const SERIAL_LINE: usize = 7;
const MANUFACTURED_LINE: usize = 8;
const SERIAL_LABEL: &[u8] = b"SERIAL #   ";
const MANUFACTURED_LABEL: &[u8] = b"MANUFACTUR DATE ";

/// The receiver ID's lines the board fills, which live as long as the
/// engine that answers with them.
#[derive(Debug)]
struct BoardLines {
    serial: [u8; LINE_WIDTH],
    manufactured: [u8; LINE_WIDTH],
}

impl BoardLines {
    /// Before they are filled.
    const BLANK: Self = Self { serial: [b' '; LINE_WIDTH], manufactured: [b' '; LINE_WIDTH] };

    /// `preset`'s receiver ID with these lines for its serial number and
    /// manufacture date.  `None` if they do not fit it.
    fn receiver_id(&'static self, preset: &Identity) -> Option<Identity> {
        let mut all = *preset.lines();
        all[SERIAL_LINE] = from_utf8(&self.serial).ok()?;
        all[MANUFACTURED_LINE] = from_utf8(&self.manufactured).ok()?;
        Identity::new(preset.model(), preset.leap_details(), all)
    }
}

/// `label` then `value` into `line`, padded with spaces.
fn fill(line: &mut [u8; LINE_WIDTH], label: &[u8], value: &[u8]) {
    for (d, &s) in line.iter_mut().zip(label.iter().chain(value).chain(repeat(&b' '))) {
        *d = s;
    }
}

impl BoardIdentity {
    /// The board's serial number and manufacture date as receiver ID
    /// lines, in `lines`.
    fn lines(&self, lines: &'static mut BoardLines) -> &'static BoardLines {
        fill(&mut lines.serial, SERIAL_LABEL, &self.serial);
        fill(&mut lines.manufactured, MANUFACTURED_LABEL, &self.manufactured);
        lines
    }
}

/// The identities the engine answers as: at power-up, and, for a VP,
/// from the first `@@Ea` (smartclock-gps's `Options::eight_on_ea`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identities {
    pub power_up: &'static Identity,
    pub eight_on_ea: Option<&'static Identity>,
}

impl Identities {
    /// The Oncores the emulator presents (PLAN.md): the VP, six channels
    /// and eight from the first `@@Ea`, the Z3801A's, Z3805A's and
    /// 58503A's; or the UT Plus, software 3.1, a shipped 58503B's.
    pub const VP: Self = Self { power_up: &VP_SIX, eight_on_ea: Some(&VP_EIGHT) };
    /// The six-channel VP alone, which the first `@@Ea` does not make
    /// eight (the Z3801A's and Z3805A's B1121P1114, in any instrument).
    pub const VP_SIX: Self = Self { power_up: &VP_SIX, eight_on_ea: None };
    /// The eight-channel VP from power-up, as a saved eight-channel
    /// configuration needs (the Z3817A's, `emulator::Configuration`).
    pub const Z3817A: Self = Self { power_up: &VP_EIGHT, eight_on_ea: None };
    pub const UT_PLUS: Self = Self { power_up: &UT_PLUS_3_1, eight_on_ea: None };
}

/// Where [`Identities`] live, as long as the engines that answer with
/// them: the board's lines and the receiver IDs made of them.
#[derive(Debug)]
pub struct IdentityStore {
    lines: BoardLines,
    power_up: Identity,
    eight_on_ea: Identity,
}

impl IdentityStore {
    /// Before it is filled.
    pub const EMPTY: Self = Self { lines: BoardLines::BLANK, power_up: VP_SIX, eight_on_ea: VP_SIX };

    /// `presets` with `board`'s serial number and manufacture date.  The
    /// lines fit every preset (tested); a preset they did not fit would
    /// be answered as it is.
    pub fn fill(&'static mut self, board: &BoardIdentity, presets: Identities) -> Identities {
        let Identities { power_up, eight_on_ea } = presets;
        let Self { lines, power_up: power_up_id, eight_on_ea: eight_id } = self;
        let lines = board.lines(lines);
        let id = |preset: &Identity| lines.receiver_id(preset).unwrap_or(*preset);
        *power_up_id = id(power_up);
        let eight_on_ea = match eight_on_ea {
            Some(preset) => {
                *eight_id = id(preset);
                let eight: &'static Identity = eight_id;
                Some(eight)
            }
            None => None,
        };
        Identities { power_up: power_up_id, eight_on_ea }
    }
}

impl Default for BoardIdentity {
    fn default() -> Self {
        Self::EMULATOR
    }
}

#[cfg(test)]
mod tests {
    use smartclock_gps::engine::identity::Identity;
    use smartclock_gps::engine::identity::UT_PLUS_2_2;
    use smartclock_gps::engine::identity::UT_PLUS_3_1;
    use smartclock_gps::engine::identity::VP_EIGHT;
    use smartclock_gps::engine::identity::VP_SIX;

    use super::BoardIdentity;
    use super::BoardLines;
    use super::Identities;
    use super::IdentityStore;
    use super::MANUFACTURED_LABEL;
    use super::MANUFACTURED_LINE;
    use super::SERIAL_LABEL;
    use super::SERIAL_LINE;
    use super::SERIAL_PREFIX;
    use crate::units::GpsWeek;

    /// The tests' GPS week: 2026-10-25 to 10-31.
    const TEST_WEEK: GpsWeek = GpsWeek::new(2442);

    /// A unique ID; the build date, 2026-10-27, in the tests' week, and
    /// its manufacture date as the engines write it.
    const UNIQUE: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    const BUILT: (u16, u8, u8) = (2026, 10, 27);
    const MANUFACTURED: &[u8; 4] = b"6J27";

    #[test]
    fn identity_from_a_unique_id() {
        /// A unique ID a bit from [`UNIQUE`]; the Saturday before the
        /// build date and the Sunday that starts its week.
        const OTHER: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13];
        const SATURDAY: u8 = 24;
        const SUNDAY: u8 = 25;
        let (year, month, day) = BUILT;
        let a = BoardIdentity::new(&UNIQUE, year, month, day);
        let b = BoardIdentity::new(&OTHER, year, month, day);
        assert_eq!(&a.serial[..SERIAL_PREFIX], b"EMU");
        assert!(a.serial[SERIAL_PREFIX..].iter().all(u8::is_ascii_digit));
        assert_ne!(a.serial, b.serial);
        assert_eq!(&a.manufactured, MANUFACTURED);
        assert_eq!(a.built_week, TEST_WEEK);
        assert_eq!(
            BoardIdentity::new(&[], year, month, SATURDAY).built_week,
            GpsWeek::new(TEST_WEEK.get() - 1)
        );
        assert_eq!(BoardIdentity::new(&[], year, month, SUNDAY).built_week, TEST_WEEK);
    }

    #[test]
    fn every_presets_receiver_id_from_one_set_of_board_lines() {
        let (year, month, day) = BUILT;
        let board = BoardIdentity::new(&UNIQUE, year, month, day);
        let board_lines = board.lines(Box::leak(Box::new(BoardLines::BLANK)));
        for preset in [&VP_SIX, &VP_EIGHT, &UT_PLUS_2_2, &UT_PLUS_3_1] {
            let lines = preset.lines();
            assert!(lines[SERIAL_LINE].as_bytes().starts_with(SERIAL_LABEL));
            assert!(lines[MANUFACTURED_LINE].as_bytes().starts_with(MANUFACTURED_LABEL));
            let id: Identity = board_lines.receiver_id(preset).unwrap();
            assert_eq!((id.model(), id.leap_details()), (preset.model(), preset.leap_details()));
            let value = |line: usize, label: &[u8]| {
                id.lines()[line].as_bytes().strip_prefix(label).unwrap().trim_ascii_end().to_vec()
            };
            assert_eq!(value(SERIAL_LINE, SERIAL_LABEL), board.serial);
            assert_eq!(value(MANUFACTURED_LINE, MANUFACTURED_LABEL), MANUFACTURED);
            for (i, (line, preset_line)) in id.lines().iter().zip(lines).enumerate() {
                if i != SERIAL_LINE && i != MANUFACTURED_LINE {
                    assert_eq!(line, preset_line);
                }
            }
        }
    }

    #[test]
    fn the_store_holds_the_power_up_and_eight_channel_identities() {
        let (year, month, day) = BUILT;
        let board = BoardIdentity::new(&UNIQUE, year, month, day);
        let serial = |id: &Identity| {
            id.lines()[SERIAL_LINE].as_bytes()[SERIAL_LABEL.len()..][..board.serial.len()].to_vec()
        };
        let store = || Box::leak(Box::new(IdentityStore::EMPTY));
        let vp = store().fill(&board, Identities::VP);
        assert_eq!(vp.power_up.model(), VP_SIX.model());
        assert_eq!(vp.eight_on_ea.map(Identity::model), Some(VP_EIGHT.model()));
        assert_eq!(serial(vp.power_up), board.serial);
        assert_eq!(vp.eight_on_ea.map(serial), Some(board.serial.to_vec()));
        let ut_plus = store().fill(&board, Identities::UT_PLUS);
        assert_eq!((ut_plus.power_up.model(), ut_plus.eight_on_ea), (UT_PLUS_3_1.model(), None));
        assert_eq!(serial(ut_plus.power_up), board.serial);
    }
}
