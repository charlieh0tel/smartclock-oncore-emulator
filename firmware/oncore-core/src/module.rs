//! The module's start-up configuration, and the engine's requests, as
//! UBX configuration messages for the LEA-M8T (u-blox 8 receiver
//! description UBX-13003221 R28: CFG-PRT p. 243, CFG-MSG p. 215, CFG-ANT
//! p. 191, CFG-CFG p. 193, CFG-NAV5 p. 216, CFG-TP5 p. 270, CFG-TMODE2
//! p. 266, MGA-INI-POS_LLH p. 329), each encoded by smartclock-ubx.
//!
//! Heights come from the Oncore as ellipsoid or MSL; the module takes
//! ellipsoid heights for its position and MSL for its fixed altitude, so
//! each is converted with the separation of the module's last fix.  A
//! hold whose height needs the separation is withheld until the
//! module's first fix gives it.

use crate::receiver::DEFAULT_MASK_ANGLE;
use crate::units::Baud;
use crate::units::Centimeters;
use crate::units::GpsWeek;
use crate::units::Microseconds;
use crate::units::MilliArcSeconds;
use crate::units::Nanoseconds;
use smartclock_gps::engine::settings::Application;
use smartclock_gps::engine::settings::Height;
use smartclock_gps::engine::settings::HeightKind;
use smartclock_gps::engine::settings::Location;
use smartclock_gps::engine::settings::TimeBase;
use smartclock_gps::record::ReceiverMode;
use smartclock_gps::units;
use smartclock_ubx::cfg::AntennaFlags;
use smartclock_ubx::cfg::AntennaPins;
use smartclock_ubx::cfg::AopConfig;
use smartclock_ubx::cfg::CfgAnt;
use smartclock_ubx::cfg::CfgCfg;
use smartclock_ubx::cfg::CfgDat;
use smartclock_ubx::cfg::CfgGnss;
use smartclock_ubx::cfg::CfgMsg;
use smartclock_ubx::cfg::CfgNav5;
use smartclock_ubx::cfg::CfgNavx5;
use smartclock_ubx::cfg::CfgPrt;
use smartclock_ubx::cfg::CfgRate;
use smartclock_ubx::cfg::CfgRxm;
use smartclock_ubx::cfg::CfgTmode2;
use smartclock_ubx::cfg::CfgTp5;
use smartclock_ubx::cfg::ConfigSections;
use smartclock_ubx::cfg::DeviceMask;
use smartclock_ubx::cfg::DynamicModel;
use smartclock_ubx::cfg::FixMode;
use smartclock_ubx::cfg::GnssBlock;
use smartclock_ubx::cfg::GnssFlags;
use smartclock_ubx::cfg::LowPowerMode;
use smartclock_ubx::cfg::Nav5Mask;
use smartclock_ubx::cfg::Navx5Mask1;
use smartclock_ubx::cfg::Navx5Mask2;
use smartclock_ubx::cfg::PortFlags;
use smartclock_ubx::cfg::Protocols;
use smartclock_ubx::cfg::PulseFlags;
use smartclock_ubx::cfg::TimeMode;
use smartclock_ubx::cfg::TimeModeFlags;
use smartclock_ubx::cfg::TimeReference;
use smartclock_ubx::cfg::TxReady;
use smartclock_ubx::cfg::UartMode;
use smartclock_ubx::cfg::UtcStandard;
use smartclock_ubx::cfg::cfg_gnss_length;
use smartclock_ubx::frame::Encode;
use smartclock_ubx::frame::MessageId;
use smartclock_ubx::frame::OVERHEAD;
use smartclock_ubx::message::MON_HW;
use smartclock_ubx::message::NAV_CLOCK;
use smartclock_ubx::message::NAV_DOP;
use smartclock_ubx::message::NAV_EOE;
use smartclock_ubx::message::NAV_POSECEF;
use smartclock_ubx::message::NAV_PVT;
use smartclock_ubx::message::NAV_SAT;
use smartclock_ubx::message::NAV_TIMEGPS;
use smartclock_ubx::message::NAV_TIMELS;
use smartclock_ubx::message::RXM_RAWX;
use smartclock_ubx::message::RXM_SFRBX;
use smartclock_ubx::message::SYSTEM_BEIDOU;
use smartclock_ubx::message::SYSTEM_GALILEO;
use smartclock_ubx::message::SYSTEM_GLONASS;
use smartclock_ubx::message::SYSTEM_GPS;
use smartclock_ubx::message::SYSTEM_IMES;
use smartclock_ubx::message::SYSTEM_QZSS;
use smartclock_ubx::message::SYSTEM_SBAS;
use smartclock_ubx::message::TIM_TP;
use smartclock_ubx::mga::MgaIniPosLlh;
use smartclock_ubx::units::CentimetersPerSecond;
use smartclock_ubx::units::DecibelHertz;
use smartclock_ubx::units::F32Bits;
use smartclock_ubx::units::F64Bits;
use smartclock_ubx::units::HundredNanodegrees;
use smartclock_ubx::units::Meters;
use smartclock_ubx::units::Millimeters;
use smartclock_ubx::units::Milliseconds;
use smartclock_ubx::units::Seconds;
use smartclock_ubx::units::SquareCentimeters;
use smartclock_ubx::units::Tenths;

/// CFG-PRT: UART1 at [`MODULE_BAUD`], 8N1, UBX in and out.
const PORT_UART1: u8 = 1;
const PORT: CfgPrt = CfgPrt {
    port_id: PORT_UART1,
    tx_ready: TxReady::NONE,
    mode: UartMode::EIGHT_BITS.union(UartMode::NO_PARITY).union(UartMode::ONE_STOP_BIT),
    baud_rate: MODULE_BAUD.get(),
    input_protocols: Protocols::UBX,
    output_protocols: Protocols::UBX,
    flags: PortFlags::NONE,
};

/// CFG-MSG's rate: each message once a navigation epoch.
const EVERY_EPOCH: u8 = 1;

/// The Oncore's 1 PPS: rising edge on the second, 200 ms long, from power
/// up (PLAN.md, "What the translator does").
const PULSE_PERIOD: Microseconds = Microseconds::new(1_000_000);
const PULSE_LENGTH: Microseconds = Microseconds::new(200_000);

/// CFG-TP5's antenna cable and RF group delays: none, both of the
/// Oncore's delays going in the user delay.
const NO_TP5_DELAY: smartclock_ubx::units::Nanoseconds<i16> = smartclock_ubx::units::Nanoseconds(0);

/// CFG-TP5 flags but `active` and the time grid.
const TP5_FLAGS: PulseFlags = PulseFlags::LOCK_GNSS_FREQUENCY
    .union(PulseFlags::LOCKED_OTHER_SET)
    .union(PulseFlags::IS_LENGTH)
    .union(PulseFlags::ALIGN_TO_TOW)
    .union(PulseFlags::RISING);

/// CFG-NAV5's mask in the set-up: every field but drLim's reserved one.
const NAV5_SETUP_MASK: Nav5Mask = Nav5Mask::DYNAMIC_MODEL
    .union(Nav5Mask::MINIMUM_ELEVATION)
    .union(Nav5Mask::POSITION_FIX_MODE)
    .union(Nav5Mask::POSITION_MASK)
    .union(Nav5Mask::TIME_MASK)
    .union(Nav5Mask::STATIC_HOLD_MASK)
    .union(Nav5Mask::DGPS_MASK)
    .union(Nav5Mask::CNO_THRESHOLD)
    .union(Nav5Mask::UTC);
const NAV5_DOP: Tenths = Tenths(250);
const NAV5_POSITION_ACCURACY: Meters<u16> = Meters(100);
const NAV5_TIME_ACCURACY: Meters<u16> = Meters(350);
const NAV5_DGNSS_TIMEOUT: Seconds<u8> = Seconds(60);
/// CFG-NAV5's off values: no static hold, no C/N0 threshold.
const NO_STATIC_HOLD_THRESHOLD: CentimetersPerSecond<u8> = CentimetersPerSecond(0);
const NO_STATIC_HOLD_DISTANCE: Meters<u16> = Meters(0);
const NO_CNO_THRESHOLD_SATELLITES: u8 = 0;
const NO_CNO_THRESHOLD: DecibelHertz = DecibelHertz(0);

/// The set-up's CFG-NAV5, every field: the engine's power-on mask angle
/// and dynamic model, auto 2-D/3-D, and TIM 1.1x's defaults for the rest
/// (UBX-13003221 R28, C.10): PDOP and TDOP masks 25.0, position and
/// time accuracy masks 100 m and 350 m, no static hold, DGNSS timeout
/// 60 s, no C/N0 threshold, UTC as USNO's.  A request's CFG-NAV5 masks
/// its own fields and gives the rest these values.
const NAV5_SETUP: CfgNav5 = CfgNav5 {
    mask: NAV5_SETUP_MASK,
    dynamic_model: dynamic_model(POWER_ON_APPLICATION),
    fix_mode: FixMode::Auto,
    fixed_altitude: NO_FIXED_ALTITUDE,
    fixed_altitude_variance: ALTITUDE_VARIANCE,
    minimum_elevation: elevation(DEFAULT_MASK_ANGLE.get()),
    position_dop: NAV5_DOP,
    time_dop: NAV5_DOP,
    position_accuracy: NAV5_POSITION_ACCURACY,
    time_accuracy: NAV5_TIME_ACCURACY,
    static_hold_threshold: NO_STATIC_HOLD_THRESHOLD,
    dgnss_timeout: NAV5_DGNSS_TIMEOUT,
    cno_threshold_satellites: NO_CNO_THRESHOLD_SATELLITES,
    cno_threshold: NO_CNO_THRESHOLD,
    static_hold_maximum_distance: NO_STATIC_HOLD_DISTANCE,
    utc_standard: UtcStandard::Usno,
};

/// CFG-NAVX5 version 2, every field the mask covers at TIM 1.1x's defaults
/// (UBX-13003221 R28, C.11): 1 to 32 satellites, minimum C/N0 9 dB-Hz,
/// no forced 3-D initial fix, no aiding ACKs, the week rollover base
/// ([`Module::week_rollover_base`]), no PPP, no `AssistNow` Autonomous,
/// its orbit error limit 100 m.
const NAVX5_MASK: Navx5Mask1 = Navx5Mask1::MINIMUM_MAXIMUM
    .union(Navx5Mask1::MINIMUM_CNO)
    .union(Navx5Mask1::INITIAL_3D_FIX)
    .union(Navx5Mask1::WEEK_ROLLOVER)
    .union(Navx5Mask1::ACK_AIDING)
    .union(Navx5Mask1::PPP)
    .union(Navx5Mask1::AOP);
const NAVX5_MIN_SVS: u8 = 1;
const NAVX5_MAX_SVS: u8 = 32;
const NAVX5_MIN_CNO: DecibelHertz = DecibelHertz(9);
const NAVX5_AOP_ORBIT_ERROR: Meters<u16> = Meters(100);
/// CFG-NAVX5's sigAttenCompMode: disabled.
const SIGNAL_ATTENUATION_COMPENSATION_OFF: u8 = 0;
/// TIM 1.12's week rollover base, 2023-01-01; TIM 1.10's and 1.11's is
/// 1867, which resolves no date past 2035-06-02 (UBX-13003221 R28,
/// C.11, wknRollover).
pub(crate) const DEFAULT_WEEK_ROLLOVER_BASE: GpsWeek = GpsWeek::new(2243);

/// CFG-RATE: one measurement and one solution a second, on GPS time (TIM
/// 1.xx's defaults, UBX-13003221 R28, C.16): the engine reads an epoch a
/// second.
const RATE: CfgRate =
    CfgRate { measurement_rate: Milliseconds(1000), navigation_rate: 1, time_reference: TimeReference::Gps };

/// WGS-84's ellipsoid: semi-major axis, inverse flattening, flattening
/// and first eccentricity squared, f(2 - f), 2 the factor of 2f - f^2.
pub(crate) const WGS84_SEMI_MAJOR_M: f64 = 6_378_137.0;
pub(crate) const WGS84_INVERSE_FLATTENING: f64 = 298.257_223_563;
const WGS84_FLATTENING: f64 = 1.0 / WGS84_INVERSE_FLATTENING;
const E2_FLATTENING_FACTOR: f64 = 2.0;
pub(crate) const WGS84_E2: f64 = WGS84_FLATTENING * (E2_FLATTENING_FACTOR - WGS84_FLATTENING);

/// CFG-DAT: a datum saved by someone else would move every position
/// the engine reports as WGS-84 (UBX-13003221 R28, 8.7, "Geodetic
/// Coordinate Systems and Ellipsoids"); CFG-DAT sets one only as
/// user-defined, so WGS-84's own figures, with no shift, rotation or
/// scale.
const NO_SHIFT: F32Bits = F32Bits(0);
const DATUM: CfgDat = CfgDat {
    semi_major_axis: F64Bits(WGS84_SEMI_MAJOR_M.to_bits()),
    inverse_flattening: F64Bits(WGS84_INVERSE_FLATTENING.to_bits()),
    shift_x: NO_SHIFT,
    shift_y: NO_SHIFT,
    shift_z: NO_SHIFT,
    rotation_x: NO_SHIFT,
    rotation_y: NO_SHIFT,
    rotation_z: NO_SHIFT,
    scale: NO_SHIFT,
};

/// CFG-CFG: every section saved, to battery-backed RAM and flash.
const SAVE: CfgCfg = CfgCfg {
    clear_mask: ConfigSections::NONE,
    save_mask: ConfigSections::ALL,
    load_mask: ConfigSections::NONE,
    device_mask: DeviceMask::BBR.union(DeviceMask::FLASH),
};

/// CFG-RXM: continuous power, as power save would space the epochs out.
const POWER: CfgRxm = CfgRxm { low_power_mode: LowPowerMode::Continuous };

/// A system's CFG-GNSS block: its reserved and maximum tracking
/// channels, and its L1 signal, enabled or not.
const fn gnss_block(system: u8, reserved: u8, maximum: u8, flags: GnssFlags) -> GnssBlock {
    GnssBlock { system, reserved_tracking_channels: reserved, maximum_tracking_channels: maximum, flags }
}

/// IMES's L1, `sigCfgMask` 0x01, which smartclock-ubx does not name.
const IMES_L1: GnssFlags = GnssFlags::signals(0x01);

/// CFG-GNSS's blocks: GPS only (PLAN.md, "Constellations"), with TIM
/// 1.1x's channel counts (UBX-13003221 R28, C.6); L1 signals.  u-blox
/// advises QZSS on whenever GPS is, against cross-correlation
/// (UBX-13003221 R28, p. 208).
const GNSS_BLOCKS: [GnssBlock; 7] = [
    gnss_block(SYSTEM_GPS, 8, 16, GnssFlags::GPS_L1CA.union(GnssFlags::ENABLE)),
    gnss_block(SYSTEM_SBAS, 0, 3, GnssFlags::SBAS_L1CA),
    gnss_block(SYSTEM_GALILEO, 0, 8, GnssFlags::GALILEO_E1),
    gnss_block(SYSTEM_BEIDOU, 0, 16, GnssFlags::BEIDOU_B1I),
    gnss_block(SYSTEM_IMES, 0, 8, IMES_L1),
    gnss_block(SYSTEM_QZSS, 0, 3, GnssFlags::QZSS_L1CA),
    gnss_block(SYSTEM_GLONASS, 0, 14, GnssFlags::GLONASS_L1),
];
/// numTrkChHw, which the module does not take.
const HARDWARE_CHANNELS_READ_ONLY: u8 = 0;
/// All the hardware's tracking channels.
const GNSS_ALL_CHANNELS: u8 = 0xff;
const SYSTEMS: CfgGnss<'static> = CfgGnss {
    hardware_tracking_channels: HARDWARE_CHANNELS_READ_ONLY,
    tracking_channels: GNSS_ALL_CHANNELS,
    blocks: &GNSS_BLOCKS,
};

/// The accuracy given with a held position.  The Oncore takes the
/// position as exact; the module needs a figure.
const HOLD_ACCURACY: Millimeters<u32> = Millimeters(100);
/// The accuracy given with an initial position: the reference asks for
/// one within a degree (p. 17).
const INITIAL_ACCURACY: smartclock_ubx::units::Centimeters<u32> =
    smartclock_ubx::units::Centimeters(10_000_000);
/// The variance given with a held altitude: 1 m^2.
const ALTITUDE_VARIANCE: SquareCentimeters = SquareCentimeters(10_000);
/// The fixed altitude with none held.
const NO_FIXED_ALTITUDE: smartclock_ubx::units::Centimeters<i32> = smartclock_ubx::units::Centimeters(0);

/// The heights CFG-TMODE2's position, CFG-NAV5's fixed altitude and
/// MGA-INI-POS_LLH's position take.
const TMODE2_HEIGHT: HeightKind = HeightKind::Ellipsoid;
const NAV5_HEIGHT: HeightKind = HeightKind::MeanSeaLevel;
const MGA_INI_HEIGHT: HeightKind = HeightKind::Ellipsoid;

/// CFG-TMODE2 off, no position and no survey-in.
const TMODE2_OFF: CfgTmode2 = CfgTmode2 {
    time_mode: TimeMode::Disabled,
    flags: TimeModeFlags::NONE,
    ecef_x_or_latitude: 0,
    ecef_y_or_longitude: 0,
    ecef_z_or_altitude: smartclock_ubx::units::Centimeters(0),
    fixed_position_accuracy: Millimeters(0),
    survey_in_minimum_duration: Seconds(0),
    survey_in_accuracy_limit: Millimeters(0),
};

/// The longest message to the module, a set-up step's or a request's,
/// UBX framing included.
pub const REQUEST_BYTES: usize = OVERHEAD + cfg_gnss_length(GNSS_BLOCKS.len());

/// The baud rate the module is moved to: NAV-SAT and RXM-RAWX each
/// second need more than the 9600 it starts at.
pub const MODULE_BAUD: Baud = Baud::new(115_200);

/// The messages the engine reads, each once a navigation epoch.
const MESSAGES: [MessageId; 12] = [
    NAV_TIMEGPS,
    NAV_PVT,
    NAV_DOP,
    NAV_SAT,
    NAV_TIMELS,
    NAV_POSECEF,
    NAV_CLOCK,
    RXM_RAWX,
    MON_HW,
    TIM_TP,
    // sent only when enabled (UBX-13003221 R28, section 10)
    RXM_SFRBX,
    // last: the epoch is complete
    NAV_EOE,
];

/// The set-up's steps, each after the one before: the port's, the
/// first; the time pulse's; the messages'; then the rest.
pub const PORT_STEP: usize = 0;
const TIME_PULSE_STEP: usize = PORT_STEP + 1;
const MESSAGES_STEP: usize = TIME_PULSE_STEP + 1;
const ANTENNA_STEP: usize = MESSAGES_STEP + MESSAGES.len();
const RATE_STEP: usize = ANTENNA_STEP + 1;
const RXM_STEP: usize = RATE_STEP + 1;
const GNSS_STEP: usize = RXM_STEP + 1;
const NAV5_STEP: usize = GNSS_STEP + 1;
const NAVX5_STEP: usize = NAV5_STEP + 1;
const TMODE2_STEP: usize = NAVX5_STEP + 1;
const DATUM_STEP: usize = TMODE2_STEP + 1;
/// The step that saves the set-up to battery-backed RAM and flash, the
/// last.
pub const SAVE_STEP: usize = DATUM_STEP + 1;

/// The steps of [`Module::setup`].
pub const SETUP_STEPS: usize = SAVE_STEP + 1;

/// CFG-ANT: supply control, short detection, power down on a short, and
/// recovery.  Open detection needs a circuit on `ANT_DET_N` (EXTINT1),
/// which the board does not have (UBX-15030060 R07, 2.4.4).  The pins
/// are TIM 1.1x's defaults (UBX-13003221 R28, C.1), the supply switch on
/// PIO16, short detection on PIO15, open detection on PIO14, with
/// `reconfig` set so the module takes them.  Sent every boot: a module
/// whose saved pins were lost refuses supply control on PIO0.
const ANT_PIN_SWITCH: u8 = 16;
const ANT_PIN_SHORT: u8 = 15;
const ANT_PIN_OPEN: u8 = 14;
const ANTENNA: CfgAnt = CfgAnt {
    flags: AntennaFlags::SUPPLY_VOLTAGE_CONTROL
        .union(AntennaFlags::SHORT_CIRCUIT_DETECTION)
        .union(AntennaFlags::POWER_DOWN_ON_SHORT_CIRCUIT)
        .union(AntennaFlags::RECOVERY),
    pins: AntennaPins::new(ANT_PIN_SWITCH, ANT_PIN_SHORT, ANT_PIN_OPEN).union(AntennaPins::RECONFIGURE),
};

/// What the engine's settings ask of the module, in smartclock-gps's
/// terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleRequest {
    /// Satellites below this elevation are not used.
    MaskAngle(units::Degrees<u8>),
    /// The dynamic model for the application.
    Application(Application),
    /// A position to start from, before the first fix.
    InitialPosition(Location),
    /// Hold this position (timing mode), or `None` to navigate.
    PositionHold(Option<Location>),
    /// Hold this height, or `None` to fix in 3-D.
    AltitudeHold(Option<Height>),
    /// What 1 PPS is aligned to.
    TimeBase(TimeBase),
    /// 1 PPS delays: the cable delay is taken off, the offset added.
    PulseDelays { cable: units::Nanoseconds<u32>, offset: units::Nanoseconds<u32> },
    /// The 1 PPS on or off, and on only once the module has GNSS time.
    Pulse { on: bool, locked_only: bool },
}

impl ModuleRequest {
    /// Whether the module acknowledges it: each CFG message does;
    /// MGA-INI does not, the set-up turning its ACK off (CFG-NAVX5
    /// ackAiding).
    pub const fn acknowledged(&self) -> bool {
        !matches!(self, Self::InitialPosition(_))
    }

    /// The module's mode once it acknowledges this request: holding
    /// for a position held, navigating for none; `None` for the rest.
    pub const fn mode_once_acknowledged(&self) -> Option<ReceiverMode> {
        match self {
            Self::PositionHold(Some(_)) => Some(ReceiverMode::Holding),
            Self::PositionHold(None) => Some(ReceiverMode::Navigating),
            _ => None,
        }
    }

    /// Whether it is a hold whose height the module takes in the other
    /// datum, which only the separation converts.
    pub(crate) fn needs_separation(&self) -> bool {
        match self {
            Self::PositionHold(Some(p)) => p.height.kind != TMODE2_HEIGHT,
            Self::AltitudeHold(Some(h)) => h.kind != NAV5_HEIGHT,
            _ => false,
        }
    }
}

/// Turns the engine's requests into UBX for the module.
#[derive(Debug)]
pub struct Module {
    /// Ellipsoid height less MSL height, from the module's last fix,
    /// once there has been one.
    pub(crate) separation: Option<Centimeters>,
    /// The first of the 1024 GPS weeks the module resolves a broadcast
    /// week into: the firmware's build week, as no date is earlier.
    pub week_rollover_base: GpsWeek,
    /// The time pulse as the instrument has asked for it.
    pulse: TimePulse,
}

/// What CFG-TP5 is built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimePulse {
    time_base: TimeBase,
    cable_delay: Nanoseconds,
    offset: Nanoseconds,
    /// The pulse is on at all, and only once the module has GNSS time.
    on: bool,
    locked_only: bool,
}

impl TimePulse {
    /// At power-up, and in the set-up the module saves: on from the
    /// start, on UTC, no delays.  The instrument's own go to the module's
    /// RAM at the set-up's end.
    const POWER_UP: Self = Self {
        time_base: TimeBase::Utc,
        cable_delay: Nanoseconds::new(0),
        offset: Nanoseconds::new(0),
        on: true,
        locked_only: false,
    };

    /// CFG-TP5 for TIMEPULSE.  Both of the module's delays move the
    /// pulse earlier; the Oncore's cable delay does too, and its 1 PPS
    /// offset is taken to move it later.  Both go in the user delay,
    /// which holds the whole range.
    fn message(&self) -> CfgTp5 {
        // Before GNSS time, a pulse of no length when it is to wait for
        // the module's lock.
        let unlocked = if self.locked_only { Microseconds::new(0) } else { PULSE_LENGTH };
        // The cable delay is any i32 the instrument sends: past the user
        // delay's range the difference saturates, rather than wrap to the
        // other sign.
        let delay = self.cable_delay.saturating_sub(self.offset);
        let grid = match self.time_base {
            TimeBase::Utc => PulseFlags::GRID_UTC,
            TimeBase::Gps => PulseFlags::GRID_GPS,
        };
        let active = if self.on { PulseFlags::ACTIVE } else { PulseFlags::NONE };
        CfgTp5 {
            time_pulse: smartclock_ubx::cfg::TimePulse::First,
            antenna_cable_delay: NO_TP5_DELAY,
            rf_group_delay: NO_TP5_DELAY,
            frequency_period: PULSE_PERIOD.get(),
            frequency_period_locked: PULSE_PERIOD.get(),
            pulse_length_ratio: unlocked.get(),
            pulse_length_ratio_locked: PULSE_LENGTH.get(),
            user_config_delay: smartclock_ubx::units::Nanoseconds(delay.get()),
            flags: TP5_FLAGS.union(grid).union(active),
        }
    }
}

impl Default for Module {
    fn default() -> Self {
        Self::new()
    }
}

impl Module {
    pub const fn new() -> Self {
        Self { separation: None, week_rollover_base: DEFAULT_WEEK_ROLLOVER_BASE, pulse: TimePulse::POWER_UP }
    }

    /// `h` as a height of `kind`, if it is one or the separation is
    /// known.
    fn height(&self, h: Height, kind: HeightKind) -> Option<Centimeters> {
        let value = Centimeters::new(h.height.0);
        match (h.kind, kind) {
            (HeightKind::Ellipsoid, HeightKind::Ellipsoid)
            | (HeightKind::MeanSeaLevel, HeightKind::MeanSeaLevel) => Some(value),
            (HeightKind::MeanSeaLevel, HeightKind::Ellipsoid) => self.separation.map(|s| value + s),
            (HeightKind::Ellipsoid, HeightKind::MeanSeaLevel) => self.separation.map(|s| value - s),
        }
    }

    /// The start-up configuration, a message a step from 0 to
    /// [`SETUP_STEPS`] - 1, written into `out`; returns its length, or 0
    /// past the last.  Step 0 moves UART1 to [`MODULE_BAUD`], UBX only;
    /// the sender switches its own baud rate after it.  Then the time
    /// pulse, the messages, the antenna supervisor, the measurement rate,
    /// continuous power, the systems, every navigation setting, position
    /// hold off, and a save to battery-backed RAM and flash.  Each sets
    /// every field the engine depends on, so nothing rests on what the
    /// module has saved.
    pub fn setup(&self, step: usize, out: &mut [u8; REQUEST_BYTES]) -> usize {
        let n = match step {
            PORT_STEP => PORT.encode(out),
            TIME_PULSE_STEP => TimePulse::POWER_UP.message().encode(out),
            s if s < ANTENNA_STEP => {
                CfgMsg { message: MESSAGES[s - MESSAGES_STEP], rate: EVERY_EPOCH }.encode(out)
            }
            ANTENNA_STEP => ANTENNA.encode(out),
            RATE_STEP => RATE.encode(out),
            RXM_STEP => POWER.encode(out),
            GNSS_STEP => SYSTEMS.encode(out),
            NAV5_STEP => NAV5_SETUP.encode(out),
            NAVX5_STEP => self.navigation_expert().encode(out),
            TMODE2_STEP => TMODE2_OFF.encode(out),
            DATUM_STEP => DATUM.encode(out),
            SAVE_STEP => SAVE.encode(out),
            _ => return 0,
        };
        n.unwrap_or(0)
    }

    /// CFG-NAVX5: the satellite and signal limits, the week rollover
    /// base, and no aiding, PPP or autonomous orbits.
    const fn navigation_expert(&self) -> CfgNavx5 {
        CfgNavx5 {
            mask1: NAVX5_MASK,
            mask2: Navx5Mask2::NONE,
            minimum_satellites: NAVX5_MIN_SVS,
            maximum_satellites: NAVX5_MAX_SVS,
            minimum_cno: NAVX5_MIN_CNO,
            initial_fix_3d: false,
            ack_aiding: false,
            week_rollover: self.week_rollover_base.get(),
            signal_attenuation_compensation: SIGNAL_ATTENUATION_COMPENSATION_OFF,
            use_ppp: false,
            aop_config: AopConfig::NONE,
            aop_orbit_maximum_error: NAVX5_AOP_ORBIT_ERROR,
            use_adr: false,
        }
    }

    /// Writes the UBX message for a request into `out`; returns its
    /// length, or `None` for nothing to send: a hold that
    /// [needs the separation](ModuleRequest::needs_separation) before the
    /// module's first fix gives it, withheld.
    pub fn encode(&mut self, request: ModuleRequest, out: &mut [u8; REQUEST_BYTES]) -> Option<usize> {
        let n = match request {
            ModuleRequest::MaskAngle(degrees) => CfgNav5 {
                mask: Nav5Mask::MINIMUM_ELEVATION,
                minimum_elevation: elevation(degrees.0),
                ..NAV5_SETUP
            }
            .encode(out),
            ModuleRequest::Application(a) => {
                CfgNav5 { mask: Nav5Mask::DYNAMIC_MODEL, dynamic_model: dynamic_model(a), ..NAV5_SETUP }
                    .encode(out)
            }
            ModuleRequest::AltitudeHold(height) => self.altitude_hold(height)?.encode(out),
            ModuleRequest::PositionHold(Some(p)) => {
                position_hold(Some((p, self.height(p.height, TMODE2_HEIGHT)?))).encode(out)
            }
            ModuleRequest::PositionHold(None) => position_hold(None).encode(out),
            ModuleRequest::InitialPosition(Location { latitude, longitude, height }) => MgaIniPosLlh {
                latitude: HundredNanodegrees(MilliArcSeconds::new(latitude.0).degrees_1e7().get()),
                longitude: HundredNanodegrees(MilliArcSeconds::new(longitude.0).degrees_1e7().get()),
                // Before the first fix, the height as given: the separation
                // is well within the position's accuracy.
                altitude: smartclock_ubx::units::Centimeters(
                    self.height(height, MGA_INI_HEIGHT).unwrap_or(Centimeters::new(height.height.0)).get(),
                ),
                position_accuracy: INITIAL_ACCURACY,
            }
            .encode(out),
            ModuleRequest::TimeBase(time_base) => {
                self.pulse.time_base = time_base;
                self.pulse.message().encode(out)
            }
            ModuleRequest::PulseDelays { cable, offset } => {
                // The cable delay's bits as the instrument sent them: the
                // 58503A adds its reference bias to it unclamped, so a
                // negative sum comes as its two's complement.
                (self.pulse.cable_delay, self.pulse.offset) =
                    (Nanoseconds::new(cable.0 as i32), Nanoseconds::new(offset.0 as i32));
                self.pulse.message().encode(out)
            }
            ModuleRequest::Pulse { on, locked_only } => {
                (self.pulse.on, self.pulse.locked_only) = (on, locked_only);
                self.pulse.message().encode(out)
            }
        };
        n.ok()
    }

    /// CFG-NAV5's fix mode, fixed altitude and its variance: 2-D at a
    /// height held, its MSL height, or auto 2-D/3-D at the set-up's;
    /// `None` for an ellipsoid height before the separation is known.
    fn altitude_hold(&self, height: Option<Height>) -> Option<CfgNav5> {
        let mask = Nav5Mask::POSITION_FIX_MODE;
        Some(match height {
            Some(h) => CfgNav5 {
                mask,
                fix_mode: FixMode::TwoD,
                fixed_altitude: smartclock_ubx::units::Centimeters(self.height(h, NAV5_HEIGHT)?.get()),
                fixed_altitude_variance: ALTITUDE_VARIANCE,
                ..NAV5_SETUP
            },
            None => CfgNav5 { mask, fix_mode: FixMode::Auto, ..NAV5_SETUP },
        })
    }
}

/// The engine's application at power-up (VP p. 12), which the set-up
/// gives the module with the power-up mask angle.
const POWER_ON_APPLICATION: Application = Application::Land;

/// CFG-NAV5's dynamic model for an Oncore application type.
const fn dynamic_model(a: Application) -> DynamicModel {
    match a {
        Application::Air => DynamicModel::Airborne1G,
        Application::Handheld => DynamicModel::Pedestrian,
        Application::Land => DynamicModel::Automotive,
        Application::Marine => DynamicModel::Sea,
        Application::Static => DynamicModel::Stationary,
    }
}

/// CFG-NAV5's minElev, an I1, for a mask angle, its bits as given: the
/// Oncore's mask angle is 0 to 89 degrees.
const fn elevation(degrees: u8) -> smartclock_ubx::units::Degrees<i8> {
    smartclock_ubx::units::Degrees(degrees.cast_signed())
}

/// CFG-TMODE2: fixed mode at a position and its ellipsoid height, or
/// off.
fn position_hold(hold: Option<(Location, Centimeters)>) -> CfgTmode2 {
    match hold {
        Some((p, ellipsoid)) => CfgTmode2 {
            time_mode: TimeMode::Fixed,
            flags: TimeModeFlags::LLA,
            ecef_x_or_latitude: MilliArcSeconds::new(p.latitude.0).degrees_1e7().get(),
            ecef_y_or_longitude: MilliArcSeconds::new(p.longitude.0).degrees_1e7().get(),
            ecef_z_or_altitude: smartclock_ubx::units::Centimeters(ellipsoid.get()),
            fixed_position_accuracy: HOLD_ACCURACY,
            ..TMODE2_OFF
        },
        None => TMODE2_OFF,
    }
}

#[cfg(test)]
mod tests {
    use smartclock_gps::engine::Engine;
    use smartclock_gps::engine::Options;
    use smartclock_gps::engine::identity::VP_SIX;
    use smartclock_gps::record::ReceiverMode;
    use smartclock_ubx::cfg::CfgNav5;
    use smartclock_ubx::cfg::CfgTmode2;
    use smartclock_ubx::cfg::CfgTp5;
    use smartclock_ubx::cfg::FixMode;
    use smartclock_ubx::cfg::Nav5Mask;
    use smartclock_ubx::cfg::PulseFlags;
    use smartclock_ubx::cfg::TMODE2_LENGTH;
    use smartclock_ubx::cfg::TimeMode;
    use smartclock_ubx::cfg::TimeModeFlags;
    use smartclock_ubx::frame::Deframer;
    use smartclock_ubx::frame::Encode;
    use smartclock_ubx::frame::MessageId;
    use smartclock_ubx::message::CFG_MSG;
    use smartclock_ubx::message::CFG_TMODE2;
    use smartclock_ubx::message::CFG_TP5;
    use smartclock_ubx::message::Message;
    use smartclock_ubx::units::Millimeters;
    use smartclock_ubx::units::Seconds;

    use super::ANTENNA_STEP;
    use super::Application;
    use super::Centimeters;
    use super::DEFAULT_MASK_ANGLE;
    use super::EVERY_EPOCH;
    use super::HOLD_ACCURACY;
    use super::Height;
    use super::HeightKind;
    use super::Location;
    use super::MESSAGES;
    use super::MESSAGES_STEP;
    use super::Module;
    use super::ModuleRequest;
    use super::NAV5_SETUP;
    use super::Nanoseconds;
    use super::POWER_ON_APPLICATION;
    use super::PULSE_LENGTH;
    use super::PULSE_PERIOD;
    use super::REQUEST_BYTES;
    use super::SETUP_STEPS;
    use super::TIME_PULSE_STEP;
    use super::TimeBase;
    use super::elevation;
    use super::units;
    use crate::fixtures::HEIGHT_GPS;
    use crate::fixtures::HEIGHT_MSL;
    use crate::fixtures::LATITUDE;
    use crate::fixtures::LATITUDE_1E7;
    use crate::fixtures::LONGITUDE;
    use crate::fixtures::LONGITUDE_1E7;
    use crate::fixtures::SEPARATION;
    use crate::units::Microseconds;
    use crate::wire::UbxReader;

    /// Hand-worked from UBX-13003221 R28, not from the encoder's
    /// constants: the set-up's step count (CFG-PRT, CFG-TP5, twelve
    /// CFG-MSG, CFG-ANT, CFG-RATE, CFG-RXM, CFG-GNSS, CFG-NAV5, CFG-NAVX5,
    /// CFG-TMODE2, CFG-DAT, CFG-CFG); CFG-TP5's flags, active, locked to
    /// GNSS, locked other set, isLength, alignToTow and rising (bits 0 to
    /// 2 and 4 to 6), and those bits; CFG-TMODE2's payload, 28 bytes.
    const STEPS: usize = 23;
    const SETUP_TP5_FLAGS: u32 = 0x77;
    const TP5_FLAG_BITS: u32 = 0x7f;
    const TMODE2_PAYLOAD_BYTES: usize = TMODE2_LENGTH;
    /// A cable delay and 1 PPS offset, ns, and the user delay they make.
    const CABLE: Nanoseconds = Nanoseconds::new(150);
    const OFFSET: Nanoseconds = Nanoseconds::new(1000);
    const USER_DELAY: Nanoseconds = CABLE.saturating_sub(OFFSET);
    /// The mask angle asked for, degrees.
    const MASK_DEGREES: u8 = 10;

    /// The golden frames' inputs, synthetic: 10.5 N, 20.5 W, 100 m, a
    /// separation of -30.04 m, a mask angle of 5 degrees, and 1 PPS
    /// delays of 500 and 300 ns.
    const GOLDEN_LATITUDE: units::MilliArcSeconds = units::MilliArcSeconds(37_800_000);
    const GOLDEN_LONGITUDE: units::MilliArcSeconds = units::MilliArcSeconds(-73_800_000);
    const GOLDEN_HEIGHT: units::Centimeters = units::Centimeters(10_000);
    const GOLDEN_SEPARATION: Centimeters = Centimeters::new(-3004);
    const GOLDEN_MASK_ANGLE: units::Degrees<u8> = units::Degrees(5);
    const GOLDEN_CABLE: units::Nanoseconds<u32> = units::Nanoseconds(500);
    const GOLDEN_OFFSET: units::Nanoseconds<u32> = units::Nanoseconds(300);
    const NO_DELAY: units::Nanoseconds<u32> = units::Nanoseconds(0);

    const fn golden_height(kind: HeightKind) -> Height {
        Height { height: GOLDEN_HEIGHT, kind }
    }

    const fn golden_location(kind: HeightKind) -> Location {
        Location { latitude: GOLDEN_LATITUDE, longitude: GOLDEN_LONGITUDE, height: golden_height(kind) }
    }

    /// The frames of the board's own encoder before smartclock-ubx's,
    /// but for three changes: CFG-CFG saves every section the document
    /// names, the set-up's CFG-NAV5 leaves drLim out of its mask, and a
    /// request's CFG-NAV5 gives the fields its mask leaves out the
    /// set-up's values, not zeros.
    /// The set-up's CFG-NAV5, its mask without drLim (bit 3).
    const NAV5_SETUP_FRAME: [u8; 44] = [
        0xb5, 0x62, 0x06, 0x24, 0x24, 0x00, 0xf7, 0x05, 0x04, 0x03, 0x00, 0x00, 0x00, 0x00, 0x10, 0x27, 0x00,
        0x00, 0x0a, 0x00, 0xfa, 0x00, 0xfa, 0x00, 0x64, 0x00, 0x5e, 0x01, 0x00, 0x3c, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x00,
    ];
    /// The set-up's CFG-CFG, saving every section the document names.
    const SAVE_FRAME: [u8; 21] = [
        0xb5, 0x62, 0x06, 0x09, 0x0d, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1f, 0x1f, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x03, 0x5d, 0xcb,
    ];
    /// `MaskAngle` 5 degrees: minElev alone in the mask, the rest at the set-up's values.
    const MASK_ANGLE_FRAME: [u8; 44] = [
        0xb5, 0x62, 0x06, 0x24, 0x24, 0x00, 0x02, 0x00, 0x04, 0x03, 0x00, 0x00, 0x00, 0x00, 0x10, 0x27, 0x00,
        0x00, 0x05, 0x00, 0xfa, 0x00, 0xfa, 0x00, 0x64, 0x00, 0x5e, 0x01, 0x00, 0x3c, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x89, 0x65,
    ];
    /// `Application` air: dynModel alone in the mask, the rest at the set-up's values.
    const AIR_FRAME: [u8; 44] = [
        0xb5, 0x62, 0x06, 0x24, 0x24, 0x00, 0x01, 0x00, 0x06, 0x03, 0x00, 0x00, 0x00, 0x00, 0x10, 0x27, 0x00,
        0x00, 0x0a, 0x00, 0xfa, 0x00, 0xfa, 0x00, 0x64, 0x00, 0x5e, 0x01, 0x00, 0x3c, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x8f, 0xfd,
    ];
    /// `Application` handheld, likewise.
    const HANDHELD_FRAME: [u8; 44] = [
        0xb5, 0x62, 0x06, 0x24, 0x24, 0x00, 0x01, 0x00, 0x03, 0x03, 0x00, 0x00, 0x00, 0x00, 0x10, 0x27, 0x00,
        0x00, 0x0a, 0x00, 0xfa, 0x00, 0xfa, 0x00, 0x64, 0x00, 0x5e, 0x01, 0x00, 0x3c, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x8c, 0x97,
    ];
    /// `AltitudeHold` at 100 m MSL: posFixMode alone in the mask, 2-D at 100 m, the rest at the set-up's values.
    const ALTITUDE_MSL_FRAME: [u8; 44] = [
        0xb5, 0x62, 0x06, 0x24, 0x24, 0x00, 0x04, 0x00, 0x04, 0x01, 0x10, 0x27, 0x00, 0x00, 0x10, 0x27, 0x00,
        0x00, 0x0a, 0x00, 0xfa, 0x00, 0xfa, 0x00, 0x64, 0x00, 0x5e, 0x01, 0x00, 0x3c, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc5, 0x9c,
    ];
    /// `AltitudeHold` at 100 m above the ellipsoid: 130.04 m MSL.
    const ALTITUDE_ELLIPSOID_FRAME: [u8; 44] = [
        0xb5, 0x62, 0x06, 0x24, 0x24, 0x00, 0x04, 0x00, 0x04, 0x01, 0xcc, 0x32, 0x00, 0x00, 0x10, 0x27, 0x00,
        0x00, 0x0a, 0x00, 0xfa, 0x00, 0xfa, 0x00, 0x64, 0x00, 0x5e, 0x01, 0x00, 0x3c, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x8c, 0x71,
    ];
    /// `AltitudeHold` off: posFixMode alone in the mask, every field at
    /// the set-up's values, auto 2-D/3-D with its altitude variance.
    const ALTITUDE_OFF_FRAME: [u8; 44] = [
        0xb5, 0x62, 0x06, 0x24, 0x24, 0x00, 0x04, 0x00, 0x04, 0x03, 0x00, 0x00, 0x00, 0x00, 0x10, 0x27, 0x00,
        0x00, 0x0a, 0x00, 0xfa, 0x00, 0xfa, 0x00, 0x64, 0x00, 0x5e, 0x01, 0x00, 0x3c, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x90, 0x25,
    ];

    /// Every set-up step's frame, in order.
    const SETUP_FRAMES: [&[u8]; STEPS] = [
        &[
            0xb5, 0x62, 0x06, 0x00, 0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0xc0, 0x08, 0x00, 0x00, 0x00, 0xc2,
            0x01, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa8, 0x42,
        ],
        &[
            0xb5, 0x62, 0x06, 0x31, 0x20, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x42,
            0x0f, 0x00, 0x40, 0x42, 0x0f, 0x00, 0x40, 0x0d, 0x03, 0x00, 0x40, 0x0d, 0x03, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x77, 0x00, 0x00, 0x00, 0x91, 0x8c,
        ],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x01, 0x20, 0x01, 0x2c, 0x83],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x01, 0x07, 0x01, 0x13, 0x51],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x01, 0x04, 0x01, 0x10, 0x4b],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x01, 0x35, 0x01, 0x41, 0xad],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x01, 0x26, 0x01, 0x32, 0x8f],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x01, 0x01, 0x01, 0x0d, 0x45],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x01, 0x22, 0x01, 0x2e, 0x87],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x02, 0x15, 0x01, 0x22, 0x70],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x0a, 0x09, 0x01, 0x1e, 0x70],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x0d, 0x01, 0x01, 0x19, 0x69],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x02, 0x13, 0x01, 0x20, 0x6c],
        &[0xb5, 0x62, 0x06, 0x01, 0x03, 0x00, 0x01, 0x61, 0x01, 0x6d, 0x05],
        &[0xb5, 0x62, 0x06, 0x13, 0x04, 0x00, 0x1b, 0x00, 0xf0, 0xb9, 0xe1, 0xd2],
        &[0xb5, 0x62, 0x06, 0x08, 0x06, 0x00, 0xe8, 0x03, 0x01, 0x00, 0x01, 0x00, 0x01, 0x39],
        &[0xb5, 0x62, 0x06, 0x11, 0x02, 0x00, 0x00, 0x00, 0x19, 0x81],
        &[
            0xb5, 0x62, 0x06, 0x3e, 0x3c, 0x00, 0x00, 0x00, 0xff, 0x07, 0x00, 0x08, 0x10, 0x00, 0x01, 0x00,
            0x01, 0x00, 0x01, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00, 0x08, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x03, 0x00, 0x10, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04, 0x00, 0x08, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x05, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x06, 0x00, 0x0e, 0x00, 0x00, 0x00,
            0x01, 0x00, 0xef, 0x29,
        ],
        &NAV5_SETUP_FRAME,
        &[
            0xb5, 0x62, 0x06, 0x23, 0x28, 0x00, 0x02, 0x00, 0x4c, 0x66, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x01, 0x20, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc3, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x5e, 0xdb,
        ],
        &[
            0xb5, 0x62, 0x06, 0x3d, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x5f, 0x6b,
        ],
        &[
            0xb5, 0x62, 0x06, 0x06, 0x2c, 0x00, 0x00, 0x00, 0x00, 0x40, 0xa6, 0x54, 0x58, 0x41, 0x88, 0x6d,
            0x74, 0x96, 0x1d, 0xa4, 0x72, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x7d, 0x70,
        ],
        &SAVE_FRAME,
    ];

    /// The requests, in order on one module, and their frames: CFG-TP5
    /// carries the pulse settings of the requests before it.
    static REQUEST_FRAMES: [(ModuleRequest, &[u8]); 15] = [
        (ModuleRequest::MaskAngle(GOLDEN_MASK_ANGLE), &MASK_ANGLE_FRAME),
        (ModuleRequest::Application(Application::Air), &AIR_FRAME),
        (ModuleRequest::Application(Application::Handheld), &HANDHELD_FRAME),
        (
            ModuleRequest::InitialPosition(golden_location(HeightKind::MeanSeaLevel)),
            &[
                0xb5, 0x62, 0x13, 0x40, 0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0x40, 0x2c, 0x42, 0x06, 0xc0,
                0xf2, 0xc7, 0xf3, 0x54, 0x1b, 0x00, 0x00, 0x80, 0x96, 0x98, 0x00, 0xa5, 0xd8,
            ],
        ),
        (
            ModuleRequest::PositionHold(Some(golden_location(HeightKind::MeanSeaLevel))),
            &[
                0xb5, 0x62, 0x06, 0x3d, 0x1c, 0x00, 0x02, 0x00, 0x01, 0x00, 0x40, 0x2c, 0x42, 0x06, 0xc0,
                0xf2, 0xc7, 0xf3, 0x54, 0x1b, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x55, 0x77,
            ],
        ),
        (
            ModuleRequest::PositionHold(Some(golden_location(HeightKind::Ellipsoid))),
            &[
                0xb5, 0x62, 0x06, 0x3d, 0x1c, 0x00, 0x02, 0x00, 0x01, 0x00, 0x40, 0x2c, 0x42, 0x06, 0xc0,
                0xf2, 0xc7, 0xf3, 0x10, 0x27, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x1d, 0xeb,
            ],
        ),
        (
            ModuleRequest::PositionHold(None),
            &[
                0xb5, 0x62, 0x06, 0x3d, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x5f, 0x6b,
            ],
        ),
        (ModuleRequest::AltitudeHold(Some(golden_height(HeightKind::MeanSeaLevel))), &ALTITUDE_MSL_FRAME),
        (ModuleRequest::AltitudeHold(Some(golden_height(HeightKind::Ellipsoid))), &ALTITUDE_ELLIPSOID_FRAME),
        (ModuleRequest::AltitudeHold(None), &ALTITUDE_OFF_FRAME),
        (
            ModuleRequest::TimeBase(TimeBase::Gps),
            &[
                0xb5, 0x62, 0x06, 0x31, 0x20, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40,
                0x42, 0x0f, 0x00, 0x40, 0x42, 0x0f, 0x00, 0x40, 0x0d, 0x03, 0x00, 0x40, 0x0d, 0x03, 0x00,
                0x00, 0x00, 0x00, 0x00, 0xf7, 0x00, 0x00, 0x00, 0x11, 0x8c,
            ],
        ),
        (
            ModuleRequest::PulseDelays { cable: GOLDEN_CABLE, offset: NO_DELAY },
            &[
                0xb5, 0x62, 0x06, 0x31, 0x20, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40,
                0x42, 0x0f, 0x00, 0x40, 0x42, 0x0f, 0x00, 0x40, 0x0d, 0x03, 0x00, 0x40, 0x0d, 0x03, 0x00,
                0xf4, 0x01, 0x00, 0x00, 0xf7, 0x00, 0x00, 0x00, 0x06, 0x33,
            ],
        ),
        (
            ModuleRequest::PulseDelays { cable: NO_DELAY, offset: GOLDEN_OFFSET },
            &[
                0xb5, 0x62, 0x06, 0x31, 0x20, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40,
                0x42, 0x0f, 0x00, 0x40, 0x42, 0x0f, 0x00, 0x40, 0x0d, 0x03, 0x00, 0x40, 0x0d, 0x03, 0x00,
                0xd4, 0xfe, 0xff, 0xff, 0xf7, 0x00, 0x00, 0x00, 0xe1, 0x13,
            ],
        ),
        (
            ModuleRequest::Pulse { on: true, locked_only: true },
            &[
                0xb5, 0x62, 0x06, 0x31, 0x20, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40,
                0x42, 0x0f, 0x00, 0x40, 0x42, 0x0f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x0d, 0x03, 0x00,
                0xd4, 0xfe, 0xff, 0xff, 0xf7, 0x00, 0x00, 0x00, 0x91, 0x26,
            ],
        ),
        (
            ModuleRequest::Pulse { on: false, locked_only: false },
            &[
                0xb5, 0x62, 0x06, 0x31, 0x20, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40,
                0x42, 0x0f, 0x00, 0x40, 0x42, 0x0f, 0x00, 0x40, 0x0d, 0x03, 0x00, 0x40, 0x0d, 0x03, 0x00,
                0xd4, 0xfe, 0xff, 0xff, 0xf6, 0x00, 0x00, 0x00, 0xe0, 0x0f,
            ],
        ),
    ];

    /// The 1 PPS delays.
    fn delays(cable: Nanoseconds, offset: Nanoseconds) -> ModuleRequest {
        ModuleRequest::PulseDelays {
            cable: units::Nanoseconds(cable.get() as u32),
            offset: units::Nanoseconds(offset.get() as u32),
        }
    }

    /// A whole UBX message's ID and payload, framed by smartclock-ubx:
    /// one frame, its checksum good, ending at `m`'s last byte.
    fn unframe(m: &[u8]) -> (MessageId, Vec<u8>) {
        let mut deframer = Deframer::<REQUEST_BYTES>::new();
        let (taken, ended) = deframer.feed(m);
        assert_eq!(taken, m.len());
        let frame = ended.expect("a whole frame").expect("its checksum good");
        (frame.id(), frame.payload().to_vec())
    }

    /// The frame for a request.
    fn frame(m: &mut Module, r: ModuleRequest) -> Vec<u8> {
        let mut out = [0; REQUEST_BYTES];
        let n = m.encode(r, &mut out).expect("a message");
        out[..n].to_vec()
    }

    fn message(m: &mut Module, r: ModuleRequest) -> (MessageId, Vec<u8>) {
        unframe(&frame(m, r))
    }

    fn nav5_frame(nav5: &CfgNav5) -> Vec<u8> {
        let mut out = [0; REQUEST_BYTES];
        let n = nav5.encode(&mut out).expect("room");
        out[..n].to_vec()
    }

    /// CFG-TP5, decoded by smartclock-ubx.
    fn tp5(p: &[u8]) -> CfgTp5 {
        let Some(Message::CfgTp5(tp5)) = Message::parse_payload(CFG_TP5, p) else { panic!("a CFG-TP5") };
        tp5
    }

    /// CFG-TMODE2, decoded by smartclock-ubx.
    fn tmode2(p: &[u8]) -> CfgTmode2 {
        let Some(Message::CfgTmode2(tmode2)) = Message::parse_payload(CFG_TMODE2, p) else {
            panic!("a CFG-TMODE2")
        };
        tmode2
    }

    /// The set-up's messages, step by step, each unframed.
    fn setup_steps() -> Vec<(MessageId, Vec<u8>)> {
        let module = Module::new();
        (0..SETUP_STEPS)
            .map(|step| {
                let mut out = [0; REQUEST_BYTES];
                let n = module.setup(step, &mut out);
                assert!(n > 0, "step {step}");
                unframe(&out[..n])
            })
            .collect()
    }

    #[test]
    fn every_frame_is_the_golden_one() {
        let mut module = Module::new();
        for (step, golden) in SETUP_FRAMES.iter().enumerate() {
            let mut out = [0; REQUEST_BYTES];
            let n = module.setup(step, &mut out);
            assert_eq!(&out[..n], *golden, "step {step}");
        }
        module.separation = Some(GOLDEN_SEPARATION);
        for &(request, golden) in &REQUEST_FRAMES {
            assert_eq!(frame(&mut module, request), golden, "{request:?}");
        }
    }

    /// An MSL position hold and an ellipsoid altitude hold need the
    /// separation, and are withheld until it is known; every other
    /// request goes without it.
    #[test]
    fn a_hold_needing_the_separation_waits_for_it() {
        let needing: Vec<_> =
            REQUEST_FRAMES.iter().map(|&(r, _)| r).filter(ModuleRequest::needs_separation).collect();
        assert_eq!(
            needing,
            [
                ModuleRequest::PositionHold(Some(golden_location(HeightKind::MeanSeaLevel))),
                ModuleRequest::AltitudeHold(Some(golden_height(HeightKind::Ellipsoid))),
            ]
        );
        let mut module = Module::new();
        let mut out = [0; REQUEST_BYTES];
        for &(request, _) in &REQUEST_FRAMES {
            assert_eq!(module.encode(request, &mut out).is_none(), request.needs_separation(), "{request:?}");
        }
        module.separation = Some(GOLDEN_SEPARATION);
        for &(request, _) in &REQUEST_FRAMES {
            assert!(module.encode(request, &mut out).is_some(), "{request:?}");
        }
    }

    #[test]
    fn setup_has_every_step_and_no_more() {
        assert_eq!(setup_steps().len(), STEPS);
        assert_eq!(Module::new().setup(SETUP_STEPS, &mut [0; REQUEST_BYTES]), 0);
    }

    /// The time pulse: 1 Hz, 200 ms, rising edge on the second.
    #[test]
    fn setup_cfg_tp5() {
        let (id, p) = setup_steps().swap_remove(TIME_PULSE_STEP);
        assert_eq!(id, CFG_TP5);
        let tp5 = tp5(&p);
        assert_eq!(tp5.time_pulse, smartclock_ubx::cfg::TimePulse::First);
        assert_eq!(
            (tp5.frequency_period, tp5.frequency_period_locked),
            (PULSE_PERIOD.get(), PULSE_PERIOD.get())
        );
        assert_eq!(
            (tp5.pulse_length_ratio, tp5.pulse_length_ratio_locked),
            (PULSE_LENGTH.get(), PULSE_LENGTH.get())
        );
        assert_eq!(tp5.flags.0 & TP5_FLAG_BITS, SETUP_TP5_FLAGS);
    }

    /// The set-up, which the module may save, carries the power-up pulse
    /// whatever the instrument has asked for since: its own go to RAM.
    #[test]
    fn the_setup_pulse_is_the_power_up_one() {
        let mut module = Module::new();
        message(&mut module, delays(CABLE, OFFSET));
        message(&mut module, ModuleRequest::Pulse { on: false, locked_only: true });
        let mut out = [0; REQUEST_BYTES];
        let n = module.setup(TIME_PULSE_STEP, &mut out);
        let tp5 = tp5(&unframe(&out[..n]).1);
        assert_eq!(
            (tp5.user_config_delay.0, tp5.pulse_length_ratio, tp5.flags.0 & TP5_FLAG_BITS),
            (0, PULSE_LENGTH.get(), SETUP_TP5_FLAGS)
        );
    }

    /// The messages, each once an epoch.
    #[test]
    fn setup_cfg_msg() {
        let steps = setup_steps();
        for ((id, p), message) in steps[MESSAGES_STEP..ANTENNA_STEP].iter().zip(MESSAGES) {
            let mut r = UbxReader::new(p);
            assert_eq!((*id, MessageId::new(r.u8(), r.u8()), r.u8()), (CFG_MSG, message, EVERY_EPOCH));
        }
    }

    #[test]
    fn a_position_hold_sets_the_mode_once_acknowledged() {
        let held = Location {
            latitude: units::MilliArcSeconds(LATITUDE.get()),
            longitude: units::MilliArcSeconds(LONGITUDE.get()),
            height: Height { height: units::Centimeters(HEIGHT_MSL.get()), kind: HeightKind::MeanSeaLevel },
        };
        assert_eq!(
            ModuleRequest::PositionHold(Some(held)).mode_once_acknowledged(),
            Some(ReceiverMode::Holding)
        );
        assert_eq!(
            ModuleRequest::PositionHold(None).mode_once_acknowledged(),
            Some(ReceiverMode::Navigating)
        );
        assert_eq!(ModuleRequest::TimeBase(TimeBase::Gps).mode_once_acknowledged(), None);
    }

    #[test]
    fn pulse_delay_saturates() {
        const LEAST: Nanoseconds = Nanoseconds::new(i32::MIN);
        const PAST_LEAST: Nanoseconds = Nanoseconds::new(1);
        const OFFSET: Nanoseconds = Nanoseconds::new(10);
        const CABLE: Nanoseconds = Nanoseconds::new(30);
        let mut module = Module::new();
        let (_, p) = message(&mut module, delays(LEAST, PAST_LEAST));
        assert_eq!(tp5(&p).user_config_delay.0, LEAST.get());
        let (_, p) = message(&mut module, delays(CABLE, OFFSET));
        assert_eq!(tp5(&p).user_config_delay.0, CABLE.saturating_sub(OFFSET).get());
    }

    #[test]
    fn pulse_off_and_locked_only() {
        let mut module = Module::new();
        let (_, p) = message(&mut module, ModuleRequest::Pulse { on: false, locked_only: false });
        assert!(!tp5(&p).flags.contains(PulseFlags::ACTIVE));
        let (_, p) = message(&mut module, ModuleRequest::Pulse { on: true, locked_only: true });
        let tp5 = tp5(&p);
        assert!(tp5.flags.contains(PulseFlags::ACTIVE));
        assert_eq!(
            (tp5.pulse_length_ratio, tp5.pulse_length_ratio_locked),
            (Microseconds::new(0).get(), PULSE_LENGTH.get())
        );
    }

    #[test]
    fn the_set_up_has_smartclock_gps_s_power_up_settings() {
        let settings = Engine::new(&VP_SIX, Options::DEFAULT).expect("the factory VP").settings();
        assert_eq!(
            (settings.application, settings.mask_angle.0),
            (POWER_ON_APPLICATION, DEFAULT_MASK_ANGLE.get())
        );
    }

    #[test]
    fn mask_angle_is_nav5_min_elevation() {
        let got = frame(&mut Module::new(), ModuleRequest::MaskAngle(units::Degrees(MASK_DEGREES)));
        let expected = CfgNav5 {
            mask: Nav5Mask::MINIMUM_ELEVATION,
            minimum_elevation: elevation(MASK_DEGREES),
            ..NAV5_SETUP
        };
        assert_eq!(got, nav5_frame(&expected));
    }

    #[test]
    fn hold_position_in_ellipsoid_height() {
        let mut module = Module::new();
        module.separation = Some(SEPARATION);
        let held = Location {
            latitude: units::MilliArcSeconds(LATITUDE.get()),
            longitude: units::MilliArcSeconds(LONGITUDE.get()),
            height: Height { height: units::Centimeters(HEIGHT_MSL.get()), kind: HeightKind::MeanSeaLevel },
        };
        let (id, p) = message(&mut module, ModuleRequest::PositionHold(Some(held)));
        assert_eq!(id, CFG_TMODE2);
        // 37.32558333 N, 121.99783333 W.
        assert_eq!(
            tmode2(&p),
            CfgTmode2 {
                time_mode: TimeMode::Fixed,
                flags: TimeModeFlags::LLA,
                ecef_x_or_latitude: LATITUDE_1E7.get(),
                ecef_y_or_longitude: LONGITUDE_1E7.get(),
                ecef_z_or_altitude: smartclock_ubx::units::Centimeters((HEIGHT_MSL + SEPARATION).get()),
                fixed_position_accuracy: HOLD_ACCURACY,
                survey_in_minimum_duration: Seconds(0),
                survey_in_accuracy_limit: Millimeters(0),
            }
        );
        let (_, off) = message(&mut module, ModuleRequest::PositionHold(None));
        assert_eq!(off, [0; TMODE2_PAYLOAD_BYTES]);
    }

    #[test]
    fn altitude_hold_in_msl_height() {
        let mut module = Module::new();
        module.separation = Some(SEPARATION);
        let height = Height { height: units::Centimeters(HEIGHT_GPS.get()), kind: HeightKind::Ellipsoid };
        let held = CfgNav5 {
            mask: Nav5Mask::POSITION_FIX_MODE,
            fix_mode: FixMode::TwoD,
            fixed_altitude: smartclock_ubx::units::Centimeters(HEIGHT_MSL.get()),
            ..NAV5_SETUP
        };
        assert_eq!(frame(&mut module, ModuleRequest::AltitudeHold(Some(height))), nav5_frame(&held));
        let free = CfgNav5 { mask: Nav5Mask::POSITION_FIX_MODE, fix_mode: FixMode::Auto, ..NAV5_SETUP };
        assert_eq!(frame(&mut module, ModuleRequest::AltitudeHold(None)), nav5_frame(&free));
    }

    #[test]
    fn time_pulse_follows_delays_and_time_base() {
        let mut module = Module::new();
        let (id, p) = message(&mut module, delays(CABLE, OFFSET));
        assert_eq!(id, CFG_TP5);
        let tp5_delays = tp5(&p);
        assert_eq!(
            (tp5_delays.pulse_length_ratio, tp5_delays.user_config_delay.0, tp5_delays.flags.grid()),
            (PULSE_LENGTH.get(), USER_DELAY.get(), PulseFlags::GRID_UTC)
        );
        let (_, p) = message(&mut module, ModuleRequest::TimeBase(TimeBase::Gps));
        let tp5_gps = tp5(&p);
        assert_eq!(
            (tp5_gps.user_config_delay.0, tp5_gps.flags.grid()),
            (USER_DELAY.get(), PulseFlags::GRID_GPS)
        );
    }
}
