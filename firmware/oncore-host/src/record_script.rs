//! `--record-script PATH`: every command the instrument sends, a line
//! `SECOND ID DATA`: SECOND the module's epochs heard before it (0 before
//! the first), ID the command's two letters, DATA its data in hex, none
//! for a bare query.  Fed back to an engine after that epoch's second,
//! the commands reach it in the epochs they reached it here; their order
//! within a second is approximate.

use std::fmt::Write as _;
use std::io::Write;

use oncore_app::command_length;
use smartclock_gps::oncore::Deframer;
use smartclock_gps::oncore::OVERHEAD;

use crate::EndOfEpoch;

/// The longest command's data the deframer holds.
const COMMAND_DATA: usize = 64;

/// The script being written to `out`, and what it has heard.
pub(crate) struct Recorder<W> {
    out: W,
    /// NAV-EOEs read from the module.
    epochs: usize,
    end: EndOfEpoch,
    decoder: Deframer<{ COMMAND_DATA + OVERHEAD }>,
}

impl<W: Write> Recorder<W> {
    pub(crate) fn new(out: W) -> Self {
        Self { out, epochs: 0, end: EndOfEpoch::new(), decoder: Deframer::new(command_length) }
    }

    /// Bytes read from the module: counts its epochs.
    pub(crate) fn module_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.end.push(b) {
                self.epochs += 1;
            }
        }
    }

    /// Bytes read from the instrument: writes each whole command.
    pub(crate) fn instrument_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            let Some(Ok(frame)) = self.decoder.push(b) else { continue };
            let (id, data) = (frame.id(), frame.data());
            let [a, b] = id.bytes();
            let mut line = format!("{} {}{}", self.epochs, char::from(a), char::from(b));
            if !data.is_empty() {
                line.push(' ');
                for d in data {
                    write!(line, "{d:02x}").expect("a String takes it");
                }
            }
            line.push('\n');
            if let Err(e) = self.out.write_all(line.as_bytes()).and_then(|()| self.out.flush()) {
                eprintln!("--record-script: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use smartclock_gps::oncore;
    use smartclock_gps::oncore::BB_VISIBLE_SATELLITES;
    use smartclock_gps::oncore::CJ_RECEIVER_ID;
    use smartclock_ubx::frame::encode;
    use smartclock_ubx::message::NAV_EOE;

    use super::Recorder;

    /// NAV-EOE's payload, iTOW alone; a frame's room.
    const EOE_PAYLOAD: [u8; 4] = [0; 4];
    const ROOM: usize = 32;
    /// `@@Bb`'s mode, on change.
    const ON_CHANGE: u8 = 1;

    fn oncore_frame(id: oncore::MessageId, data: &[u8]) -> Vec<u8> {
        let mut f = [0u8; ROOM];
        let n = oncore::encode(id, data, &mut f).unwrap();
        f[..n].to_vec()
    }

    #[test]
    fn each_command_with_the_epochs_before_it() {
        let mut eoe = [0u8; ROOM];
        let n = encode(NAV_EOE, &EOE_PAYLOAD, &mut eoe).unwrap();
        let mut r = Recorder::new(Vec::new());
        r.instrument_bytes(&oncore_frame(CJ_RECEIVER_ID, &[]));
        r.module_bytes(&eoe[..n]);
        r.module_bytes(&eoe[..n]);
        // split across reads
        let bb = oncore_frame(BB_VISIBLE_SATELLITES, &[ON_CHANGE]);
        let (first, rest) = bb.split_at(bb.len() / 2);
        r.instrument_bytes(first);
        r.instrument_bytes(rest);
        assert_eq!(String::from_utf8(r.out).unwrap(), "0 Cj\n2 Bb 01\n");
    }
}
