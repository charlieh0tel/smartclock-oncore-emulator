//! `--initial-config`: the receiver as an instrument left it configured
//! before it was last turned off (smartclock-gps's saved configuration),
//! for an instrument that cannot command it (a Z3817A).  A VP keeps that
//! configuration over a power cycle (`docs/vp-on-jig.md`, "The mask
//! angle").

use oncore_core::emulator::Configuration;
use oncore_core::identity::Identities;
use smartclock_gps::engine::identity::Model;
use smartclock_gps::engine::saved::CONFIGURATION_58503A;
use smartclock_gps::engine::saved::ChannelCount;
use smartclock_gps::engine::saved::SavedConfiguration;

/// The Z3801A's and Z3805A's (3543, 3543B, the same scripts): the
/// 58503A's at six channels, `@@Ba` and `@@Bn` in place of `@@Ea` and
/// `@@En` (smartclock `docs/firmware/oncore/`, the init scripts; `gps.md`,
/// "Time RAIM", the hold scripts).
const CONFIGURATION_Z3801A: SavedConfiguration =
    SavedConfiguration { channels: ChannelCount::Six, ..CONFIGURATION_58503A };

const _: () = assert!(CONFIGURATION_Z3801A.check(Model::VpSix).is_ok());

/// The presets: each instrument's own configuration, with identities
/// whose channels match it; the Z3817A's is the 58503A's (the owner's
/// choice) and deaf, as it cannot command its receiver.
const PRESETS: [(&str, Identities, Configuration); 4] = [
    ("58503a", Identities::Z3817A, Configuration { saved: Some(CONFIGURATION_58503A), deaf: false }),
    ("z3801a", Identities::VP, Configuration { saved: Some(CONFIGURATION_Z3801A), deaf: false }),
    ("z3805a", Identities::VP, Configuration { saved: Some(CONFIGURATION_Z3801A), deaf: false }),
    ("z3817a", Identities::Z3817A, Configuration::Z3817A),
];

/// A preset's identities and configuration, by name.
pub(crate) fn parse(arg: &str) -> Result<(Identities, Configuration), String> {
    PRESETS
        .iter()
        .find(|(name, ..)| *name == arg)
        .map(|&(_, identities, configuration)| (identities, configuration))
        .ok_or_else(|| format!("{arg:?}: expected 58503a, z3801a, z3805a or z3817a"))
}

#[cfg(test)]
mod tests {
    use oncore_core::emulator::Emulator;
    use smartclock_gps::engine::Output;
    use smartclock_gps::engine::ReceiverControl;
    use smartclock_gps::engine::identity::VP_EIGHT;
    use smartclock_gps::engine::identity::VP_SIX;
    use smartclock_gps::oncore::BA_POSITION_STATUS;
    use smartclock_gps::oncore::BN_TIME_RAIM;
    use smartclock_gps::oncore::EA_POSITION_STATUS;
    use smartclock_gps::oncore::EN_TIME_RAIM;
    use smartclock_gps::oncore::MessageId;
    use smartclock_gps::units::Milliseconds;

    use super::PRESETS;
    use super::parse;

    /// The emulator's frames, by ID.
    #[derive(Default)]
    struct Sent(Vec<MessageId>);

    impl Output for Sent {
        fn send(&mut self, frame: &[u8]) {
            if let [_, _, a, b, ..] = *frame {
                self.0.push(MessageId::new([a, b]));
            }
        }
    }

    const NOW: Milliseconds<u64> = Milliseconds(0);

    /// `name`'s preset at power-up, a second on: its channels and what it
    /// sent unasked.
    fn powered_up(name: &str) -> (usize, Vec<MessageId>) {
        let (identities, configuration) = parse(name).unwrap();
        let mut emulator = Box::new(Emulator::new(identities, configuration, ReceiverControl::Uncommanded));
        let mut sent = Sent::default();
        emulator.silent_second(NOW, &mut sent, &mut |_| {});
        (emulator.receiver().channels(), sent.0)
    }

    #[test]
    fn every_preset_powers_up_with_its_outputs() {
        for (name, ..) in PRESETS {
            let (channels, sent) = powered_up(name);
            let (status, traim, model) = if channels == VP_EIGHT.model().channels() {
                (EA_POSITION_STATUS, EN_TIME_RAIM, &VP_EIGHT)
            } else {
                (BA_POSITION_STATUS, BN_TIME_RAIM, &VP_SIX)
            };
            assert_eq!(channels, model.model().channels(), "{name}");
            assert!(sent.contains(&status) && sent.contains(&traim), "{name}: {sent:?}");
        }
    }

    #[test]
    fn the_z3817a_as_the_58503a_and_the_z3805a_as_the_z3801a() {
        assert_eq!(powered_up("z3817a"), powered_up("58503a"));
        assert_eq!(parse("z3805a"), parse("z3801a"));
        assert!(parse("ea:1").is_err());
    }
}
