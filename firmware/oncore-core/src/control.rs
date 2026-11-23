//! What smartclock-gps's engine's settings ask of the module: the
//! settings its commands change that are a receiver's to act on, as
//! [`ModuleRequest`]s.  The engine answers the commands and holds the
//! settings; this acts on those the module has a counterpart for, as
//! [`Engine::receive`]'s events name them, and once a second on what the
//! receiver has learned since.  The settings that choose satellites the
//! receiver reads for itself.
//!
//! [`Engine::receive`]: smartclock_gps::engine::Engine::receive

use smartclock_gps::engine::event::Event;
use smartclock_gps::engine::event::Events;
use smartclock_gps::engine::settings::PulseMode;
use smartclock_gps::engine::settings::Setting;
use smartclock_gps::engine::settings::SettingSet;
use smartclock_gps::engine::settings::Settings;

use crate::module::ModuleRequest;
use crate::receiver::Receiver;

/// The most module requests [`Control::configure`] makes, for a
/// caller's buffer.
pub const CONFIGURE_REQUESTS: usize = 8;

/// What the module has been asked for that the settings alone do not
/// say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Control {
    /// An initial position in range has been given since power-up or the
    /// last reset: only then is it the module's to start from.
    initial_set: bool,
    /// The pulse is on: the engine's 1 PPS mode rule (`Engine::pulsing`).
    pulse: bool,
}

impl Default for Control {
    fn default() -> Self {
        Self::new()
    }
}

impl Control {
    /// At power-up, the pulse on as the module's set-up leaves it.
    pub const fn new() -> Self {
        Self { initial_set: false, pulse: true }
    }

    /// Asks the module for what `events` changed, `settings` as the
    /// engine has them after.  A reset, which [`Events`] gives first,
    /// asks for every setting as it then stands but the initial position,
    /// which it forgets: the default it leaves is no position to start
    /// from, so one given in the same bytes after the reset is forgotten
    /// too.  `pulsing`: whether the engine's 1 PPS mode puts the pulse
    /// out (`Engine::pulsing`).
    pub fn events(
        &mut self,
        events: Events,
        settings: &Settings,
        pulsing: bool,
        out: &mut impl FnMut(ModuleRequest),
    ) {
        let mut reset = false;
        for event in events {
            match event {
                Event::Reset => {
                    reset = true;
                    self.initial_set = false;
                    self.pulse = pulsing;
                    self.configure(settings, out);
                }
                Event::SettingsChanged(changed) if !reset => self.changed(changed, settings, pulsing, out),
                _ => {}
            }
        }
    }

    fn changed(
        &mut self,
        changed: SettingSet,
        settings: &Settings,
        pulsing: bool,
        out: &mut impl FnMut(ModuleRequest),
    ) {
        if changed.contains(Setting::MaskAngle) {
            out(ModuleRequest::MaskAngle(settings.mask_angle));
        }
        if changed.contains(Setting::Application) {
            out(ModuleRequest::Application(settings.application));
        }
        if changed.contains(Setting::TimeBase) {
            out(ModuleRequest::TimeBase(settings.time_base));
        }
        if changed.contains(Setting::CableDelay) || changed.contains(Setting::PulseOffset) {
            out(pulse_delays(settings));
        }
        if changed.contains(Setting::PulseMode) {
            self.pulse = pulsing;
            out(self.pulse_request(settings));
        }
        // A position or height to hold matters only while it is held.
        if changed.contains(Setting::PositionHold)
            || (changed.contains(Setting::HeldPosition) && settings.position_hold)
        {
            out(position_hold(settings));
        }
        if changed.contains(Setting::AltitudeHold)
            || (changed.contains(Setting::AltitudeHoldHeight) && settings.altitude_hold)
        {
            out(altitude_hold(settings));
        }
        if changed.contains(Setting::InitialPosition) {
            self.initial_set = true;
            out(ModuleRequest::InitialPosition(settings.initial_position));
        }
    }

    /// Asks the module for every setting it has a counterpart for, as
    /// after its set-up: the pulse as last left, Time RAIM's gating
    /// included, and the initial position only once one is given.
    pub fn configure(&self, settings: &Settings, out: &mut impl FnMut(ModuleRequest)) {
        out(ModuleRequest::MaskAngle(settings.mask_angle));
        out(ModuleRequest::Application(settings.application));
        out(ModuleRequest::TimeBase(settings.time_base));
        out(pulse_delays(settings));
        out(self.pulse_request(settings));
        out(altitude_hold(settings));
        out(position_hold(settings));
        if self.initial_set {
            out(ModuleRequest::InitialPosition(settings.initial_position));
        }
    }

    /// The second's work: the pulse turned on or off as `pulsing`, the
    /// engine's 1 PPS mode rule, changes, and the holds the module
    /// withheld for the separation their heights need asked for again
    /// once the first fix gives it.
    pub fn second(
        &mut self,
        settings: &Settings,
        pulsing: bool,
        receiver: &mut Receiver,
        out: &mut impl FnMut(ModuleRequest),
    ) {
        if pulsing != self.pulse {
            self.pulse = pulsing;
            out(self.pulse_request(settings));
        }
        if core::mem::take(&mut receiver.separation_learned) {
            [altitude_hold(settings), position_hold(settings)]
                .into_iter()
                .filter(ModuleRequest::needs_separation)
                .for_each(out);
        }
    }

    fn pulse_request(&self, settings: &Settings) -> ModuleRequest {
        ModuleRequest::Pulse { on: self.pulse, locked_only: settings.pulse_mode == PulseMode::Tracking }
    }
}

fn pulse_delays(settings: &Settings) -> ModuleRequest {
    ModuleRequest::PulseDelays { cable: settings.cable_delay, offset: settings.pulse_offset }
}

/// The position held, if held: the engine takes one in range only
/// (`@@As`, p. 54).
fn position_hold(settings: &Settings) -> ModuleRequest {
    let held = settings.held_position;
    ModuleRequest::PositionHold(settings.position_hold.then_some(held))
}

fn altitude_hold(settings: &Settings) -> ModuleRequest {
    ModuleRequest::AltitudeHold(settings.altitude_hold.then_some(settings.altitude_hold_height))
}

#[cfg(test)]
mod tests {
    use smartclock_gps::engine::Engine;
    use smartclock_gps::engine::Options;
    use smartclock_gps::engine::Output;
    use smartclock_gps::engine::identity::VP_SIX;
    use smartclock_gps::engine::settings::Height;
    use smartclock_gps::engine::settings::HeightKind;
    use smartclock_gps::engine::settings::Location;
    use smartclock_gps::engine::settings::TimeBase;
    use smartclock_gps::oncore;
    use smartclock_gps::oncore::AD_LATITUDE;
    use smartclock_gps::oncore::AG_MASK_ANGLE;
    use smartclock_gps::oncore::AS_HOLD_POSITION;
    use smartclock_gps::oncore::AT_POSITION_HOLD;
    use smartclock_gps::oncore::AU_ALTITUDE_HOLD_HEIGHT;
    use smartclock_gps::oncore::AV_ALTITUDE_HOLD;
    use smartclock_gps::oncore::AW_TIME_MODE;
    use smartclock_gps::oncore::AZ_CABLE_DELAY;
    use smartclock_gps::oncore::BN_TIME_RAIM;
    use smartclock_gps::oncore::CF_SET_TO_DEFAULTS;
    use smartclock_gps::oncore::MAX_COMMAND;
    use smartclock_gps::oncore::MessageId;
    use smartclock_gps::oncore::OVERHEAD;
    use smartclock_gps::record::Record;
    use smartclock_gps::units;

    use super::Control;
    use crate::fixtures::HEIGHT_GPS;
    use crate::fixtures::HEIGHT_MSL;
    use crate::fixtures::LATITUDE;
    use crate::fixtures::LONGITUDE;
    use crate::fixtures::SEPARATION;
    use crate::module::Module;
    use crate::module::ModuleRequest;
    use crate::module::REQUEST_BYTES;
    use crate::receiver::Receiver;
    use crate::receiver::ReceiverSettings;
    use crate::record;
    use crate::units::Centimeters;
    use smartclock_gps::engine::settings::CHANNELS_MOST;

    /// `@@At`, `@@Av`, `@@Aw`'s and `@@Bn`'s codes (VP pp. 56, 58, 62 and
    /// 104).
    const OFF: u8 = 0;
    const ON: u8 = 1;
    const GPS_TIME: u8 = 0;
    const PULSE_ALWAYS: u8 = 1;
    const PULSE_CONFIRMED: u8 = 3;
    /// `@@Bn`'s alarm limit, 100 ns, and the offset of its 1 PPS mode.
    const ALARM_LIMIT: u16 = 10;
    const PULSE_MODE_OFFSET: usize = 4;
    /// `@@As`'s and `@@Au`'s height types.
    const ELLIPSOID: u8 = 0;
    const MSL: u8 = 1;
    /// Another ellipsoid height to hold, 10 m above the fixtures'.
    const RAISE: Centimeters = Centimeters::new(1000);
    const HEIGHT_GPS_HIGHER: Centimeters = HEIGHT_GPS.plus(RAISE);
    /// A mask angle, degrees, and a cable delay, ns.
    const MASK_DEGREES: u8 = 15;
    const CABLE_NS: u32 = 48;
    /// A latitude out of range, past 90 degrees.
    const LATITUDE_PAST_POLE: i32 = 324_000_001;

    struct Discard;

    impl Output for Discard {
        fn send(&mut self, _: &[u8]) {}
    }

    /// An engine, the module's control, a receiver and the module's
    /// encoder, at power-up.
    struct Bench {
        engine: Box<Engine>,
        control: Control,
        receiver: Box<Receiver>,
        record: Box<Record>,
        module: Module,
    }

    impl Bench {
        fn new() -> Self {
            Self {
                engine: Box::new(Engine::new(&VP_SIX, Options::DEFAULT).expect("the factory VP")),
                control: Control::new(),
                receiver: Box::new(Receiver::new(CHANNELS_MOST)),
                record: Box::new(Record::EMPTY),
                module: Module::new(),
            }
        }

        /// Those of `requests` the module's encoder sends, not withholds.
        fn sent(&mut self, requests: Vec<ModuleRequest>) -> Vec<ModuleRequest> {
            let mut frame = [0; REQUEST_BYTES];
            requests.into_iter().filter(|&r| self.module.encode(r, &mut frame).is_some()).collect()
        }

        /// The requests command `id` with `data` makes that the module's
        /// encoder sends.
        fn command_sent(&mut self, id: MessageId, data: &[u8]) -> Vec<ModuleRequest> {
            let requests = self.command(id, data);
            self.sent(requests)
        }

        /// Those of [`Control::configure`]'s requests the module's encoder sends.
        fn configure_sent(&mut self) -> Vec<ModuleRequest> {
            let mut all = Vec::new();
            self.control.configure(&self.engine.settings(), &mut |r| all.push(r));
            self.sent(all)
        }

        /// The module's first fix: the separation known.
        fn first_fix(&mut self) {
            self.module.separation = Some(SEPARATION);
            self.receiver.separation_learned = true;
        }

        /// The requests command `id` with `data` makes.
        fn command(&mut self, id: MessageId, data: &[u8]) -> Vec<ModuleRequest> {
            let mut frame = [0; MAX_COMMAND];
            let n = oncore::encode(id, data, &mut frame).expect("room");
            let events = self.engine.receive(&frame[..n], &self.record, units::Milliseconds(0), &mut Discard);
            let pulsing = self.engine.pulsing(&self.record);
            let mut out = Vec::new();
            self.control.events(events, &self.engine.settings(), pulsing, &mut |r| out.push(r));
            out
        }

        /// The requests a second makes.
        fn second(&mut self) -> Vec<ModuleRequest> {
            let settings = self.engine.settings();
            record::fill(&self.receiver, &ReceiverSettings::of(&settings), &mut self.record);
            let pulsing = self.engine.pulsing(&self.record);
            let mut out = Vec::new();
            self.control.second(&settings, pulsing, &mut self.receiver, &mut |r| out.push(r));
            out
        }
    }

    /// `@@As`'s data for the fixtures' position, MSL.
    fn hold_position() -> Vec<u8> {
        hold_at(MSL)
    }

    /// `@@As`'s data for the fixtures' position, its height of type
    /// `kind`.
    fn hold_at(kind: u8) -> Vec<u8> {
        [LATITUDE.get(), LONGITUDE.get()]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .chain(hold_height(kind))
            .collect()
    }

    /// `@@Au`'s data for the fixtures' height of type `kind`.
    fn hold_height(kind: u8) -> Vec<u8> {
        let height = if kind == MSL { HEIGHT_MSL } else { HEIGHT_GPS };
        height_data(height.get(), kind)
    }

    /// `@@Au`'s data for `height` of type `kind`.
    fn height_data(height: i32, kind: u8) -> Vec<u8> {
        height.to_be_bytes().into_iter().chain([kind]).collect()
    }

    fn held() -> Location {
        held_at(HeightKind::MeanSeaLevel)
    }

    fn held_at(kind: HeightKind) -> Location {
        Location {
            latitude: units::MilliArcSeconds(LATITUDE.get()),
            longitude: units::MilliArcSeconds(LONGITUDE.get()),
            height: held_height(kind),
        }
    }

    fn held_height(kind: HeightKind) -> Height {
        let height = match kind {
            HeightKind::MeanSeaLevel => HEIGHT_MSL,
            HeightKind::Ellipsoid => HEIGHT_GPS,
        };
        Height { height: units::Centimeters(height.get()), kind }
    }

    #[test]
    fn a_setting_asks_for_its_request() {
        let mut b = Bench::new();
        assert_eq!(
            b.command(AG_MASK_ANGLE, &[MASK_DEGREES]),
            [ModuleRequest::MaskAngle(units::Degrees(MASK_DEGREES))]
        );
        assert_eq!(b.command(AW_TIME_MODE, &[GPS_TIME]), [ModuleRequest::TimeBase(TimeBase::Gps)]);
        assert_eq!(
            b.command(AZ_CABLE_DELAY, &CABLE_NS.to_be_bytes()),
            [ModuleRequest::PulseDelays {
                cable: units::Nanoseconds(CABLE_NS),
                offset: units::Nanoseconds(0)
            }]
        );
        assert_eq!(b.command(AG_MASK_ANGLE, &[MASK_DEGREES]), [], "unchanged");
    }

    /// A position or height to hold goes to the module only while held.
    #[test]
    fn holds_only_while_held() {
        let mut b = Bench::new();
        assert_eq!(b.command(AS_HOLD_POSITION, &hold_position()), []);
        assert_eq!(b.command(AT_POSITION_HOLD, &[ON]), [ModuleRequest::PositionHold(Some(held()))]);
        let height: Vec<u8> = HEIGHT_MSL.get().to_be_bytes().into_iter().chain([MSL]).collect();
        assert_eq!(b.command(AU_ALTITUDE_HOLD_HEIGHT, &height), []);
    }

    /// A held position out of range is refused by the engine: the last
    /// one in range is held.
    #[test]
    fn a_held_position_out_of_range_leaves_the_last() {
        let mut b = Bench::new();
        b.command(AS_HOLD_POSITION, &hold_position());
        let past_pole: Vec<u8> = [LATITUDE_PAST_POLE, LONGITUDE.get(), HEIGHT_MSL.get()]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .chain([MSL])
            .collect();
        assert_eq!(b.command(AS_HOLD_POSITION, &past_pole), []);
        assert_eq!(b.command(AT_POSITION_HOLD, &[ON]), [ModuleRequest::PositionHold(Some(held()))]);
    }

    /// The initial position goes to the module in range only, then again
    /// with every setting at a reset, which forgets it.
    #[test]
    fn the_initial_position_in_range_until_a_reset() {
        let mut b = Bench::new();
        assert_eq!(b.command(AD_LATITUDE, &LATITUDE_PAST_POLE.to_be_bytes()), []);
        let r = b.command(AD_LATITUDE, &LATITUDE.get().to_be_bytes());
        assert!(matches!(r[..], [ModuleRequest::InitialPosition(l)] if l.latitude.0 == LATITUDE.get()));
        let mut all = Vec::new();
        b.control.configure(&b.engine.settings(), &mut |r| all.push(r));
        assert!(all.iter().any(|r| matches!(r, ModuleRequest::InitialPosition(_))));

        let reset = b.command(CF_SET_TO_DEFAULTS, &[]);
        assert!(reset.contains(&ModuleRequest::MaskAngle(b.engine.settings().mask_angle)));
        assert!(!reset.iter().any(|r| matches!(r, ModuleRequest::InitialPosition(_))));
    }

    /// The pulse follows the engine's 1 PPS mode rule: off in mode 3 with
    /// nothing tracked for Time RAIM to confirm, on in mode 1.
    #[test]
    fn the_pulse_follows_the_engine_s_rule() {
        let mut b = Bench::new();
        let length = oncore::command_length(BN_TIME_RAIM).expect("a VP command");
        let mut setup = vec![0; length - OVERHEAD];
        setup[1] = ON;
        setup[2..PULSE_MODE_OFFSET].copy_from_slice(&ALARM_LIMIT.to_be_bytes());
        setup[PULSE_MODE_OFFSET] = PULSE_CONFIRMED;
        assert_eq!(b.command(BN_TIME_RAIM, &setup), [ModuleRequest::Pulse { on: false, locked_only: false }]);
        assert_eq!(b.second(), []);
        setup[PULSE_MODE_OFFSET] = PULSE_ALWAYS;
        assert_eq!(b.command(BN_TIME_RAIM, &setup), [ModuleRequest::Pulse { on: true, locked_only: false }]);
        assert_eq!(b.second(), []);
    }

    /// An MSL position hold is withheld, set-up's end too, until the
    /// first fix gives the separation, then sent once.
    #[test]
    fn an_msl_position_hold_waits_for_the_separation() {
        let mut b = Bench::new();
        b.command(AS_HOLD_POSITION, &hold_position());
        assert_eq!(b.command_sent(AT_POSITION_HOLD, &[ON]), []);
        assert!(!b.configure_sent().iter().any(|r| matches!(r, ModuleRequest::PositionHold(_))));
        assert_eq!(b.second(), []);
        b.first_fix();
        let held = [ModuleRequest::PositionHold(Some(held()))];
        let second = b.second();
        assert_eq!(b.sent(second), held);
        assert_eq!(b.second(), []);
        assert!(b.configure_sent().contains(&held[0]));
    }

    /// An altitude hold at an ellipsoid height likewise.
    #[test]
    fn an_ellipsoid_altitude_hold_waits_for_the_separation() {
        let mut b = Bench::new();
        b.command(AU_ALTITUDE_HOLD_HEIGHT, &hold_height(ELLIPSOID));
        assert_eq!(b.command_sent(AV_ALTITUDE_HOLD, &[ON]), []);
        b.first_fix();
        let second = b.second();
        assert_eq!(b.sent(second), [ModuleRequest::AltitudeHold(Some(held_height(HeightKind::Ellipsoid)))]);
    }

    /// A hold in the module's own datum, an ellipsoid position or an MSL
    /// altitude, goes at once, and not again with the separation.
    #[test]
    fn a_hold_in_the_module_s_datum_goes_at_once() {
        let mut b = Bench::new();
        b.command(AS_HOLD_POSITION, &hold_at(ELLIPSOID));
        assert_eq!(
            b.command_sent(AT_POSITION_HOLD, &[ON]),
            [ModuleRequest::PositionHold(Some(held_at(HeightKind::Ellipsoid)))]
        );
        b.command(AU_ALTITUDE_HOLD_HEIGHT, &hold_height(MSL));
        assert_eq!(
            b.command_sent(AV_ALTITUDE_HOLD, &[ON]),
            [ModuleRequest::AltitudeHold(Some(held_height(HeightKind::MeanSeaLevel)))]
        );
        b.first_fix();
        assert_eq!(b.second(), []);
    }

    /// A hold changed while withheld goes as last set, once.  (The engine
    /// takes no `@@As` while the position is held, p. 53.)
    #[test]
    fn a_hold_changed_while_withheld_goes_as_last_set() {
        let mut b = Bench::new();
        b.command(AU_ALTITUDE_HOLD_HEIGHT, &hold_height(ELLIPSOID));
        b.command(AV_ALTITUDE_HOLD, &[ON]);
        assert_eq!(
            b.command_sent(AU_ALTITUDE_HOLD_HEIGHT, &height_data(HEIGHT_GPS_HIGHER.get(), ELLIPSOID)),
            []
        );
        b.first_fix();
        let higher =
            Height { height: units::Centimeters(HEIGHT_GPS_HIGHER.get()), kind: HeightKind::Ellipsoid };
        assert_eq!(b.second(), [ModuleRequest::AltitudeHold(Some(higher))]);
    }

    /// A hold turned off while withheld: off goes at once, and nothing
    /// with the separation.
    #[test]
    fn a_hold_turned_off_while_withheld_sends_nothing_stale() {
        let mut b = Bench::new();
        b.command(AS_HOLD_POSITION, &hold_position());
        b.command(AT_POSITION_HOLD, &[ON]);
        assert_eq!(b.command_sent(AT_POSITION_HOLD, &[OFF]), [ModuleRequest::PositionHold(None)]);
        b.first_fix();
        assert_eq!(b.second(), []);
    }
}
