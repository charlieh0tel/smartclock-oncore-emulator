//! The emulated Oncore: smartclock-gps's engine answering the instrument
//! from the record the receiver fills each second, and the module asked
//! for what the engine's settings need of it ([`control`](crate::control)).

use smartclock_gps::engine::Engine;
use smartclock_gps::engine::Options;
use smartclock_gps::engine::Output;
use smartclock_gps::engine::ReceiverControl;
use smartclock_gps::engine::saved::CONFIGURATION_58503A;
use smartclock_gps::engine::saved::SavedConfiguration;
use smartclock_gps::engine::settings::Settings;
use smartclock_gps::record::ReceiverMode;
use smartclock_gps::record::Record;
use smartclock_gps::units::Milliseconds;

use crate::control::Control;
use crate::gnss::Epochs;
use crate::identity::Identities;
use crate::module::ModuleRequest;
use crate::receiver::Receiver;
use crate::receiver::ReceiverSettings;
use crate::record;
use crate::units::GpsSeconds;
use crate::units::Seconds;

/// How the engine powers up besides its identities: as a receiver left
/// it configured, if one did, and whether it takes commands at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Configuration {
    pub saved: Option<SavedConfiguration>,
    pub deaf: bool,
}

impl Configuration {
    /// As it left the factory, waiting to be told.
    pub const FACTORY: Self = Self { saved: None, deaf: false };
    /// The Z3817A's: as a 58503A leaves its engine, which a VP keeps
    /// over a power cycle (`docs/vp-on-jig.md`, "The mask angle"), and
    /// deaf, as the Z3817A cannot command its receiver; with an
    /// eight-channel identity ([`Identities::Z3817A`]).
    pub const Z3817A: Self = Self { saved: Some(CONFIGURATION_58503A), deaf: true };
}

/// An engine at power-up as `identities` and `configuration` say, its
/// status holding the position as `receiver` says.
///
/// # Panics
///
/// If they disagree (smartclock-gps's `ConfigurationError`): both are
/// constants, held to each other by the tests, and in a static the
/// build fails.
const fn engine(identities: Identities, configuration: Configuration, receiver: ReceiverControl) -> Engine {
    let mut options = Options::DEFAULT;
    options.receiver = receiver;
    options.eight_on_ea = identities.eight_on_ea;
    options.saved = configuration.saved;
    options.deaf = configuration.deaf;
    match Engine::new(identities.power_up, options) {
        Ok(engine) => engine,
        Err(_) => panic!("the identities and the saved configuration disagree"),
    }
}

/// The satellite alert computation, and channel selection after it,
/// every this many epochs; combinations a step of the selection.
pub const SELECT_EPOCHS: u32 = 5;
const SELECT_BUDGET: usize = 8;
/// Satellites a step of the pseudorange corrections.
const CORRECTION_BUDGET: usize = 1;

/// The engine, the receiver it reports and what the module is asked for.
#[derive(Debug)]
pub struct Emulator {
    /// Answers the instrument and holds its settings.
    engine: Engine,
    /// What the module has said, and what is worked out from it.
    receiver: Receiver,
    /// What the module has been asked for.
    control: Control,
    /// The current second's, as the engine reads it.
    record: Record,
}

impl Emulator {
    /// At power-up as `identities` and `configuration` say, before the
    /// module is heard: `receiver`, whether the module is commanded.
    pub const fn new(
        identities: Identities,
        configuration: Configuration,
        receiver: ReceiverControl,
    ) -> Self {
        Self {
            engine: engine(identities, configuration, receiver),
            receiver: Receiver::new(identities.power_up.model().channels()),
            control: Control::new(),
            record: Record::EMPTY,
        }
    }

    /// The engine at power-up as `identities` and `configuration` say;
    /// what the module has said, and been asked for, kept.  Not inlined:
    /// the new engine is built in this frame, not in the caller's task.
    #[inline(never)]
    pub fn start(&mut self, identities: Identities, configuration: Configuration, receiver: ReceiverControl) {
        self.engine = engine(identities, configuration, receiver);
        self.receiver.channels = identities.power_up.model().channels();
    }

    /// What the instrument's commands have set the engine to.
    pub fn settings(&self) -> Settings {
        self.engine.settings()
    }

    /// The settings the receiver acts on, from the engine's.
    pub fn receiver_settings(&self) -> ReceiverSettings {
        ReceiverSettings::of(&self.engine.settings())
    }

    /// What the module has said, and what is worked out from it.
    pub fn receiver(&self) -> &Receiver {
        &self.receiver
    }

    /// The instrument's bytes, at `now`: each whole command answered to
    /// `out`, and the module asked for what they changed.  Not inlined:
    /// the engine's command handling would join its caller's poll frame.
    #[inline(never)]
    pub fn command(
        &mut self,
        bytes: &[u8],
        now: Milliseconds<u64>,
        out: &mut impl Output,
        requests: &mut impl FnMut(ModuleRequest),
    ) {
        let events = self.engine.receive(bytes, &self.record, now, out);
        // The first `@@Ea` may have made it eight channels.
        self.receiver.channels = self.engine.identity().model().channels();
        let pulsing = self.engine.pulsing(&self.record);
        self.control.events(events, &self.engine.settings(), pulsing, requests);
    }

    /// A second's end, after its epoch or on the timer: Time RAIM
    /// evaluated, the record filled, the engine's continuous messages to
    /// `out`, the module asked for what the second changed, and the
    /// corrections started on its measurements.
    pub fn second(
        &mut self,
        now: Milliseconds<u64>,
        out: &mut impl Output,
        requests: &mut impl FnMut(ModuleRequest),
    ) {
        let settings = self.engine.settings();
        let receiver_settings = ReceiverSettings::of(&settings);
        self.receiver.evaluate_traim(&receiver_settings);
        record::fill(&self.receiver, &receiver_settings, &mut self.record);
        self.engine.second(&self.record, now, out);
        let pulsing = self.engine.pulsing(&self.record);
        self.control.second(&settings, pulsing, &mut self.receiver, requests);
        self.receiver.start_corrections();
    }

    /// No epoch in time: the receiver's missed second, then
    /// [`Self::second`].
    pub fn silent_second(
        &mut self,
        now: Milliseconds<u64>,
        out: &mut impl Output,
        requests: &mut impl FnMut(ModuleRequest),
    ) {
        self.receiver.missed();
        self.second(now, out, requests);
    }

    /// Starts choosing the channels' satellites afresh, unless a choice
    /// is under way.
    pub fn alert(&mut self) {
        if !self.receiver.selector.busy() {
            let settings = self.receiver_settings();
            self.receiver.selector.start(&settings, &self.receiver.sky, self.receiver.channels);
        }
    }

    /// A step of channel selection, else of the corrections, if either is
    /// under way: whether one ran.
    pub fn background(&mut self) -> bool {
        if self.receiver.selector.busy() {
            let settings = self.receiver_settings();
            let r = &mut self.receiver;
            r.selector.step(&settings, &r.sky, r.channels, SELECT_BUDGET);
            true
        } else if self.receiver.differential.busy() {
            self.receiver.differential.step(&self.receiver.nav, CORRECTION_BUDGET);
            true
        } else {
            false
        }
    }

    /// Asks the module for every setting it has a counterpart for, as
    /// after its set-up.
    pub fn configure(&self, requests: &mut impl FnMut(ModuleRequest)) {
        self.control.configure(&self.engine.settings(), requests);
    }

    /// When the next answer that waits is due, by `now`'s clock.
    pub fn deadline(&self) -> Option<Milliseconds<u64>> {
        self.engine.deadline()
    }

    /// Sends the answers due at `now`.
    pub fn poll(&mut self, now: Milliseconds<u64>, out: &mut impl Output) {
        self.engine.poll(now, out);
    }

    /// The module lost.
    pub fn lost(&mut self) {
        self.receiver.lost();
    }

    /// The module's mode, as an acknowledged CFG-TMODE2 shows it, before
    /// its NAV-PVT does: it holds or navigates with or without
    /// satellites, as an Oncore's position hold does.
    pub fn module_mode(&mut self, mode: ReceiverMode) {
        self.receiver.mode = mode;
    }
}

impl Epochs for Emulator {
    fn receiver(&mut self) -> &mut Receiver {
        &mut self.receiver
    }

    fn second(&mut self, gps_time: Option<GpsSeconds>) {
        self.receiver.second(gps_time);
    }

    fn leap_seconds(&mut self, leap: Seconds<i8>) {
        self.receiver.leap_seconds(leap);
    }
}

#[cfg(test)]
mod tests {
    use smartclock_gps::engine::Output;
    use smartclock_gps::engine::ReceiverControl;
    use smartclock_gps::engine::identity::VP_EIGHT;
    use smartclock_gps::engine::identity::VP_SIX;
    use smartclock_gps::oncore;
    use smartclock_gps::oncore::AG_MASK_ANGLE;
    use smartclock_gps::oncore::BA_POSITION_STATUS;
    use smartclock_gps::oncore::CG_FIX_MODE;
    use smartclock_gps::oncore::EA_POSITION_STATUS;
    use smartclock_gps::oncore::EN_TIME_RAIM;
    use smartclock_gps::oncore::MAX_COMMAND;
    use smartclock_gps::oncore::MessageId;
    use smartclock_gps::units;
    use smartclock_gps::units::Milliseconds;

    use super::Configuration;
    use super::Emulator;
    use crate::identity::Identities;
    use crate::module::ModuleRequest;

    /// The frames' IDs sent.
    #[derive(Default)]
    struct Frames(Vec<[u8; 2]>);

    impl Output for Frames {
        fn send(&mut self, frame: &[u8]) {
            self.0.push([frame[2], frame[3]]);
        }
    }

    /// `@@Ba`'s rate, once a second, `@@Cg`'s fix mode (VP p. 118), and a
    /// mask angle, degrees.
    const EVERY_SECOND: u8 = 1;
    const FIXING: u8 = 1;
    const MASK_DEGREES: u8 = 15;
    const NOW: Milliseconds<u64> = Milliseconds(0);

    fn frame(id: MessageId, data: &[u8]) -> Vec<u8> {
        let mut b = [0; MAX_COMMAND];
        let n = oncore::encode(id, data, &mut b).unwrap();
        b[..n].to_vec()
    }

    fn emulator() -> Box<Emulator> {
        Box::new(Emulator::new(Identities::VP, Configuration::FACTORY, ReceiverControl::Commanded))
    }

    #[test]
    fn a_command_answered_and_the_module_asked() {
        let mut e = emulator();
        let (mut out, mut requests) = (Frames::default(), Vec::new());
        e.command(&frame(AG_MASK_ANGLE, &[MASK_DEGREES]), NOW, &mut out, &mut |r| requests.push(r));
        assert_eq!(out.0, [AG_MASK_ANGLE.bytes()]);
        assert_eq!(requests, [ModuleRequest::MaskAngle(units::Degrees(MASK_DEGREES))]);
    }

    #[test]
    fn a_silent_second_while_fixing_sends_the_continuous_messages() {
        let mut e = emulator();
        let (mut out, mut requests) = (Frames::default(), Vec::new());
        let commands = [frame(CG_FIX_MODE, &[FIXING]), frame(BA_POSITION_STATUS, &[EVERY_SECOND])].concat();
        e.command(&commands, NOW, &mut out, &mut |r| requests.push(r));
        out.0.clear();
        e.silent_second(NOW, &mut out, &mut |r| requests.push(r));
        assert_eq!(out.0, [BA_POSITION_STATUS.bytes()]);
        assert!(requests.is_empty());
        assert!(!e.background());
    }

    #[test]
    fn every_build_s_identities_and_configuration_agree() {
        for (identities, configuration) in [
            (Identities::VP, Configuration::FACTORY),
            (Identities::VP_SIX, Configuration::FACTORY),
            (Identities::UT_PLUS, Configuration::FACTORY),
            (Identities::Z3817A, Configuration::Z3817A),
        ] {
            drop(Box::new(Emulator::new(identities, configuration, ReceiverControl::Commanded)));
        }
    }

    /// The Z3817A's receiver, configured before it was last turned off:
    /// eight channels, `@@Ea` and `@@En` each second with nothing asked,
    /// and no answer to a command.
    #[test]
    fn the_z3817a_s_receiver_sends_unasked_and_hears_nothing() {
        let mut e =
            Box::new(Emulator::new(Identities::Z3817A, Configuration::Z3817A, ReceiverControl::Commanded));
        let mut out = Frames::default();
        e.command(&frame(AG_MASK_ANGLE, &[MASK_DEGREES]), NOW, &mut out, &mut |_| {});
        assert!(out.0.is_empty());
        e.silent_second(NOW, &mut out, &mut |_| {});
        assert!(
            out.0.contains(&EA_POSITION_STATUS.bytes()) && out.0.contains(&EN_TIME_RAIM.bytes()),
            "{:?}",
            out.0
        );
        assert_eq!(e.receiver.channels, VP_EIGHT.model().channels());
    }

    #[test]
    fn the_six_channel_vp_alone_stays_six() {
        let mut e =
            Box::new(Emulator::new(Identities::VP_SIX, Configuration::FACTORY, ReceiverControl::Commanded));
        e.command(&frame(EA_POSITION_STATUS, &[EVERY_SECOND]), NOW, &mut Frames::default(), &mut |_| {});
        assert_eq!(e.receiver.channels, VP_SIX.model().channels());
    }

    #[test]
    fn the_first_ea_makes_the_receiver_eight_channels() {
        let mut e = emulator();
        let six = e.receiver.channels;
        e.command(&frame(EA_POSITION_STATUS, &[EVERY_SECOND]), NOW, &mut Frames::default(), &mut |_| {});
        assert_eq!((six, e.receiver.channels), (VP_SIX.model().channels(), VP_EIGHT.model().channels()));
        e.start(Identities::VP, Configuration::FACTORY, ReceiverControl::Commanded);
        assert_eq!(e.receiver.channels, six);
    }
}
