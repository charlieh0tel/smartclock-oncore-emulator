//! The emulator running: the engine between the module's UART and the
//! instrument's, the same on the STM32 and on a host.  `no_std`, on
//! `embedded_io_async` ports and `embassy_time`.
//!
//! [`run`] serves both ports from one task, so nothing is shared, and
//! sets the module up between the instrument's commands, a message at a
//! time as its ACKs come: the module's messages feed the receiver, NAV-EOE
//! ends each epoch and runs the emulator's second, smartclock-gps's engine
//! answers the instrument, and channel selection runs a few combinations
//! at a time whenever no byte is waiting.  If the module falls silent the
//! second runs on a timer, so the Oncore's clock keeps going.

#![no_std]

use embassy_futures::select::Either3;
use embassy_futures::select::select3;
use embassy_time::Duration;
use embassy_time::Instant;
use embassy_time::Timer;
use embedded_io_async::Read;
use embedded_io_async::ReadReady;
use embedded_io_async::Write;
use oncore_core::control::CONFIGURE_REQUESTS;
use oncore_core::emulator::Configuration;
use oncore_core::emulator::Emulator;
use oncore_core::emulator::SELECT_EPOCHS;
use oncore_core::gnss::Ack;
use oncore_core::gnss::Gnss;
use oncore_core::identity::Identities;
use oncore_core::module::MODULE_BAUD;
use oncore_core::module::Module;
use oncore_core::module::ModuleRequest;
use oncore_core::module::PORT_STEP;
use oncore_core::module::REQUEST_BYTES;
use oncore_core::module::SAVE_STEP;
use oncore_core::module::SETUP_STEPS;
use oncore_core::units::Baud;
use oncore_core::units::GpsWeek;
use oncore_core::units::Seconds;
use smartclock_gps::engine::Output;
use smartclock_gps::engine::ReceiverControl;
use smartclock_gps::oncore;
use smartclock_gps::oncore::Deframer;
use smartclock_gps::oncore::FrameError;
use smartclock_gps::oncore::MAX_COMMAND;
use smartclock_gps::oncore::MessageId;
use smartclock_gps::record::ReceiverMode;
use smartclock_gps::units::Milliseconds;
use smartclock_ubx::frame::message_id;
use smartclock_ubx::message::NAV_EOE;

/// The module's UART, whose baud rate the set-up changes, and its reset.
/// Used from one task, so its futures need not be `Send`.
#[expect(async_fn_in_trait, reason = "used from one task, so its futures need not be Send")]
pub trait ModulePort: Read + ReadReady + Write {
    async fn set_baud(&mut self, baud: Baud);
    /// Resets the module, which clears its RTC time, where the port can.
    async fn reset(&mut self);
}

/// What the board running [`run`] gives it besides its ports: a log of
/// what happens, and a watchdog to feed.
pub trait Host {
    fn log(&mut self, event: Event);
    /// The loop is alive: once each pass, which waits for a second at
    /// most.
    fn alive(&mut self) {}
}

/// A log alone, with no watchdog: a host's or a test's.
impl<F: FnMut(Event)> Host for F {
    fn log(&mut self, event: Event) {
        self(event);
    }
}

/// What happens, for a log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The module answered at this baud rate before the set-up.
    ModuleFound(Baud),
    /// The module was not heard at [`MODULE_BAUD`]; set up from 9600.
    ModuleMoved,
    /// A set-up step the module refused, or did not acknowledge.
    SetupNak(usize),
    SetupTimeout(usize),
    /// The set-up is done.
    SetupDone,
    /// The set-up ended with a step refused or unanswered: not saved,
    /// and the module reset and set up again after a wait.
    SetupFailed,
    /// A command from the instrument, by ID.
    Command(MessageId),
    /// A frame from the instrument with a bad checksum or unknown ID.
    BadFrame(MessageId),
    /// A byte after the instrument's `@@` that cannot start an ID.
    BadId(u8),
    UnknownCommand(MessageId),
    /// The second ran on the timer: no epoch from the module.
    Silent,
    /// A module request did not fit in the queue and was dropped.
    ModuleRequestDropped,
    /// A module request refused, or not acknowledged, [`ACK_TRIES`]
    /// times: the emulator's settings go to the module again.
    ModuleRequestFailed,
    /// An answer or output to the instrument did not fit in a pass's
    /// buffer and was dropped.
    AnswerDropped,
    /// The bench's reset, `@@Zr`, answered: `run` returns.
    Reset,
    /// The bench's knock, `@@Zb`, answered: `run` returns for the ROM
    /// bootloader.
    Bootloader,
    /// The module was silent [`MODULE_RESET_SILENT`] seconds: reset and
    /// set up again.
    ModuleReset,
}

/// Why [`run`] returned, for its caller to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// The bench's reset, `@@Zr`: reset.
    Reset,
    /// The bench's knock, `@@Zb`: start the ROM serial bootloader, which
    /// listens on the instrument's port, for `stm32flash`.
    Bootloader,
}

/// The Oncore binary's baud rate, 8N1 (VP reference, "Binary Format"):
/// the instrument's port.
pub const ONCORE_BAUD: Baud = Baud::new(9600);

/// `@@Zr`, the bench's reset, not an Oncore message: in neither the
/// reference nor the images' tables; and its argument, which keeps a
/// stray frame from taking it.
pub const ZR_RESET: MessageId = MessageId::new(*b"Zr");
pub const RESET_ARGUMENT: [u8; 4] = *b"BOOT";

/// `@@Zb`, the bench's knock for the bootloader, not an Oncore message,
/// and its argument, which keeps a stray frame from taking it.
pub const ZB_BOOTLOADER: MessageId = MessageId::new(*b"Zb");
pub const BOOTLOADER_ARGUMENT: [u8; 4] = *b"LOAD";

/// The bench's commands, their arguments, and why each ends a run.
const BENCH_COMMANDS: [(MessageId, [u8; 4], Exit); 2] =
    [(ZR_RESET, RESET_ARGUMENT, Exit::Reset), (ZB_BOOTLOADER, BOOTLOADER_ARGUMENT, Exit::Bootloader)];

/// The module's baud rate out of reset (UBX-15025193 R10, 8, "Default
/// messages").
pub const MODULE_DEFAULT_BAUD: Baud = Baud::new(9600);
/// How long without a message before the module is taken as not there.
const LISTEN: Duration = Duration::from_millis(1500);
/// The baud rates a module not heard at [`MODULE_BAUD`] is looked for
/// at, one each set-up, from its default out of reset; a module saved
/// at another rate keeps it through a reset.
const PROBE_BAUDS: [Baud; 4] = [MODULE_DEFAULT_BAUD, BAUD_38400, BAUD_19200, BAUD_57600];
/// The rates the module may have been saved at, besides its own.
const BAUD_19200: Baud = Baud::new(19_200);
const BAUD_38400: Baud = Baud::new(38_400);
const BAUD_57600: Baud = Baud::new(57_600);
/// How long after a failed set-up the module is reset and set up again:
/// at first, doubled after each failure in a row, up to the most.
pub const SETUP_RETRY: Duration = Duration::from_secs(30);
const SETUP_RETRY_MOST: Duration = Duration::from_secs(480);
/// How long the module is given to take step 0 at 9600 and change its
/// baud rate.
const MOVE_SETTLE: Duration = Duration::from_millis(100);
/// How long to wait for a set-up message's ACK, and how often to send it.
const ACK_WAIT: Duration = Duration::from_millis(1000);
const ACK_TRIES: usize = 3;
/// Seconds run on the timer, with no message from the module,
/// before the module is reset and set up again.
pub const MODULE_RESET_SILENT: Seconds<u8> = Seconds::new(10);
/// The longest a second may wait for the module's epoch; then the
/// second runs each second on its own until an epoch comes.
const EPOCH_WAIT: Duration = Duration::from_millis(1500);
const SECOND: Duration = Duration::from_secs(1);

/// A second's answers and continuous outputs to the instrument, and its
/// requests to the module, as gathered before they are written.
const ONCORE_OUT: usize = 1024;
/// The module requests waiting at most: `configure`'s and a pass's
/// others, a command's and a second's (pulse, initial position and
/// hold).
const REQUESTS_QUEUED: usize = CONFIGURE_REQUESTS + PASS_REQUESTS;
const PASS_REQUESTS: usize = 4;
/// What one read takes from the module and from the instrument.  The
/// shortest Oncore command is `@@`, its ID, a checksum and CR LF, so a
/// read of that much completes one command at most, and a pass builds
/// one answer at most: `@@Cj`'s 294 bytes and a second's continuous
/// outputs fit `ONCORE_OUT`, where four `@@Cj` did not.
const MODULE_READ: usize = 128;
const ONCORE_READ: usize = SHORTEST_COMMAND;
const SHORTEST_COMMAND: usize = 7;

/// Bytes gathered for a port while the emulator runs, and written out
/// after.
struct Gathered<'a> {
    buf: &'a mut [u8],
    len: usize,
    /// Whether something did not fit, and was dropped.
    dropped: bool,
}

impl<'a> Gathered<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0, dropped: false }
    }

    /// `bytes` after what is gathered, or dropped if they do not fit.
    fn append(&mut self, bytes: &[u8]) {
        match self.buf.get_mut(self.len..self.len + bytes.len()) {
            Some(out) => {
                out.copy_from_slice(bytes);
                self.len += bytes.len();
            }
            None => self.dropped = true,
        }
    }
}

impl Output for Gathered<'_> {
    fn send(&mut self, frame: &[u8]) {
        self.append(frame);
    }
}

/// Answers to the instrument, gathered, and requests to the module,
/// queued.
struct Outbox<'a> {
    oncore: Gathered<'a>,
    requests: &'a mut Requests,
    encoder: &'a mut Module,
}

impl<'a> Outbox<'a> {
    /// Answers into `oncore`, requests into `requests`, encoded by
    /// `encoder`.
    fn new(oncore: &'a mut [u8], requests: &'a mut Requests, encoder: &'a mut Module) -> Self {
        Self { oncore: Gathered::new(oncore), requests, encoder }
    }

    /// The instrument's output, and where module requests go.
    fn split(&mut self) -> (&mut Gathered<'a>, impl FnMut(ModuleRequest)) {
        let Self { oncore, requests, encoder } = self;
        // A request the encoder withholds is not queued, so its ACK sets
        // no mode.
        let request = |request: ModuleRequest| {
            let mut b = [0u8; REQUEST_BYTES];
            if let Some(n) = encoder.encode(request, &mut b) {
                requests.queue(&b[..n], request.acknowledged(), request.mode_once_acknowledged());
            }
        };
        (oncore, request)
    }

    /// Logs an answer that did not fit.
    fn log_drops(&self, log: &mut impl FnMut(Event)) {
        if self.oncore.dropped {
            log(Event::AnswerDropped);
        }
    }
}

/// Requests to the module once it is set up, sent one at a time, each
/// sent again until the module acknowledges it, [`ACK_TRIES`] times at
/// most.  Requests made while it is not open, during a set-up, are
/// dropped: its end sends every setting.
#[derive(Debug)]
struct Requests {
    /// The requests waiting, oldest first, the first being sent: each
    /// frame, its length, whether the module acknowledges it, and the
    /// module's mode once it does.
    frames: [[u8; REQUEST_BYTES]; REQUESTS_QUEUED],
    lengths: [usize; REQUESTS_QUEUED],
    acknowledged: [bool; REQUESTS_QUEUED],
    modes: [Option<ReceiverMode>; REQUESTS_QUEUED],
    waiting: usize,
    /// The first request's tries so far and when its ACK is due, once
    /// sent.
    sent: Option<(usize, Instant)>,
    /// Requests are taken: the module is set up.
    open: bool,
    /// A request was refused or not acknowledged after its tries.
    failed: bool,
    /// A request did not fit, and was dropped.
    dropped: bool,
}

impl Requests {
    #[expect(
        clippy::large_stack_arrays,
        reason = "a const fn: the firmware builds it at compile time, in a static"
    )]
    const fn new() -> Self {
        Self {
            frames: [[0; REQUEST_BYTES]; REQUESTS_QUEUED],
            lengths: [0; REQUESTS_QUEUED],
            acknowledged: [false; REQUESTS_QUEUED],
            modes: [None; REQUESTS_QUEUED],
            waiting: 0,
            sent: None,
            open: false,
            failed: false,
            dropped: false,
        }
    }

    /// `frame` queued, if the queue is open and has room.
    fn queue(&mut self, frame: &[u8], acknowledged: bool, mode: Option<ReceiverMode>) {
        if !self.open {
            return;
        }
        let i = self.waiting;
        match self.frames.get_mut(i) {
            Some(slot) => {
                slot[..frame.len()].copy_from_slice(frame);
                self.lengths[i] = frame.len();
                self.acknowledged[i] = acknowledged;
                self.modes[i] = mode;
                self.waiting += 1;
            }
            None => self.dropped = true,
        }
    }

    /// Nothing waiting, and nothing taken until opened.
    fn clear(&mut self) {
        (self.waiting, self.sent, self.open) = (0, None, false);
    }

    /// The first request done with.
    fn pop(&mut self) {
        if self.waiting > 0 {
            self.frames.copy_within(1..self.waiting, 0);
            self.lengths.copy_within(1..self.waiting, 0);
            self.acknowledged.copy_within(1..self.waiting, 0);
            self.modes.copy_within(1..self.waiting, 0);
            self.waiting -= 1;
        }
        self.sent = None;
    }

    /// The module's ACK or NAK: the first request done, or sent again
    /// at once.  The module's mode, if an ACK sets it.
    fn answered(&mut self, ack: Ack, now: Instant) -> Option<ReceiverMode> {
        let (tries, _) = self.sent?;
        if message_id(&self.frames[0][..self.lengths[0]]) != Some(ack.message) {
            return None;
        }
        if ack.accepted {
            let mode = self.modes[0];
            self.pop();
            mode
        } else {
            self.sent = Some((tries, now));
            None
        }
    }

    /// The first request's frame, if it goes now: not sent yet, or its
    /// ACK overdue with tries left.  One with no tries left is given up.
    fn due(&mut self, now: Instant) -> Option<&[u8]> {
        if self.waiting == 0 {
            return None;
        }
        match self.sent {
            None => self.sent = Some((1, now + ACK_WAIT)),
            Some((tries, until)) if until <= now && tries < ACK_TRIES => {
                self.sent = Some((tries + 1, now + ACK_WAIT));
            }
            Some((_, until)) if until <= now => {
                self.pop();
                self.failed = true;
                return None;
            }
            Some(_) => return None,
        }
        Some(&self.frames[0][..self.lengths[0]])
    }

    /// The first request sent: done with if the module does not
    /// acknowledge it.
    fn sent(&mut self) {
        if self.waiting > 0 && !self.acknowledged[0] {
            self.pop();
        }
    }

    /// When the first request's ACK is due, once sent.
    fn deadline(&self) -> Option<Instant> {
        self.sent.map(|(_, until)| until)
    }

    /// Logs a request dropped or given up, once each.
    fn log(&mut self, log: &mut impl FnMut(Event)) {
        if core::mem::take(&mut self.dropped) {
            log(Event::ModuleRequestDropped);
        }
        if self.failed {
            log(Event::ModuleRequestFailed);
        }
    }
}

/// The module's set-up, stepped between the loop's events, so the
/// instrument is answered from the start however long the module takes.
/// Nothing else goes to the module meanwhile: at the end
/// `Emulator::configure` gives it every setting as the instrument left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Setup {
    run: Run,
    phase: Phase,
}

/// What a set-up keeps from phase to phase.  The probe and the wait
/// carry over a module reset, so a search goes on where it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    /// The rate in [`PROBE_BAUDS`] to try if the module is not heard.
    probe: usize,
    /// How long after this set-up, if it fails, the module is reset and
    /// set up again.
    retry: Duration,
    /// The module was found at 9600, so the set-up is saved at its end; a
    /// module at [`MODULE_BAUD`] has saved it already, and its flash is
    /// spared.
    save: bool,
    /// A step was refused or not answered.
    failed: bool,
}

impl Run {
    /// The first set-up's.
    const FIRST: Self = Self { probe: 0, retry: SETUP_RETRY, save: false, failed: false };
}

/// Where a set-up is, and until when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Listening at [`MODULE_BAUD`] for a message.
    Listen { until: Instant },
    /// Not heard: step 0, `len` bytes in [`State`]'s `setup_out`, sent at
    /// the probed rate, the module given until `until` to change its rate.
    Settle { len: usize, until: Instant },
    /// Step `step`, its message's `len` bytes in `setup_out`, sent `tries`
    /// times, its ACK awaited until `until`.
    Step { step: usize, len: usize, tries: usize, until: Instant },
    /// Failed: serving, and the module reset and set up again at `until`.
    Retry { until: Instant },
    /// Done, or not wanted.
    Done,
}

/// What a set-up step is written with: the module's encoder, and the
/// buffer the message being sent is kept in, [`State`]'s `setup_out`.
struct Outgoing<'a> {
    module: &'a Module,
    message: &'a mut [u8; REQUEST_BYTES],
}

/// News from the module for the set-up: whether a whole message was
/// read, and the ACK or NAK it sent, if one, which counts for the
/// message being sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Heard {
    message: bool,
    ack: Option<Ack>,
}

/// What the set-up asks of the loop next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupAction {
    Nothing,
    /// Send the message in `setup_out`.
    Send,
    /// Go to this baud rate and send step 0, in `setup_out`.
    Move(Baud),
    /// Reset the module and set it up again.
    Reset,
    /// Back to [`MODULE_BAUD`], then send the message in `setup_out`.
    Settled,
    /// Done: send the emulator's settings.
    Finish,
}

impl Setup {
    /// Listening from `now`, the search going on from `after`'s probe and
    /// retry wait.
    fn listen(now: Instant, after: Run) -> Self {
        Self {
            run: Run { save: false, failed: false, ..after },
            phase: Phase::Listen { until: now + LISTEN },
        }
    }

    /// Not wanted: the module is left as it is.
    const fn done() -> Self {
        Self { run: Run::FIRST, phase: Phase::Done }
    }

    /// `action`, else, when the wake brought none and the deadline is
    /// past, the time-out's.
    fn due(
        &mut self,
        action: SetupAction,
        out: &mut Outgoing<'_>,
        now: Instant,
        log: &mut impl FnMut(Event),
    ) -> SetupAction {
        match self.deadline() {
            Some(due) if action == SetupAction::Nothing && due <= now => self.expired(out, now, log),
            _ => action,
        }
    }

    /// When it next times out, if it is under way.
    fn deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Listen { until }
            | Phase::Settle { until, .. }
            | Phase::Step { until, .. }
            | Phase::Retry { until } => Some(until),
            Phase::Done => None,
        }
    }

    /// The length of the message in `setup_out`, if one is being sent.
    fn sending(&self) -> Option<usize> {
        match self.phase {
            Phase::Settle { len, .. } | Phase::Step { len, .. } => Some(len),
            _ => None,
        }
    }

    /// Whether the emulator's own requests may go to the module: the
    /// set-up is over, for now or for good.
    fn serving(&self) -> bool {
        matches!(self.phase, Phase::Done | Phase::Retry { .. })
    }

    /// Set up, every step acknowledged, CFG-TMODE2's off among them.
    fn set_up(&self) -> bool {
        matches!(self.phase, Phase::Done)
    }

    /// Step `step`, its message written into `out`, or the end: a failed
    /// set-up is not saved, and the module is reset and set up again
    /// after a wait that doubles with each failure in a row.
    fn begin(
        &mut self,
        step: usize,
        out: &mut Outgoing<'_>,
        now: Instant,
        log: &mut impl FnMut(Event),
    ) -> SetupAction {
        let Run { save, failed, .. } = self.run;
        if step >= SETUP_STEPS || (step == SAVE_STEP && (!save || failed)) {
            if failed {
                self.phase = Phase::Retry { until: now + self.run.retry };
                self.run.retry = (self.run.retry * 2).min(SETUP_RETRY_MOST);
                log(Event::SetupFailed);
            } else {
                self.phase = Phase::Done;
                self.run.retry = SETUP_RETRY;
                log(Event::SetupDone);
            }
            return SetupAction::Finish;
        }
        let len = out.module.setup(step, out.message);
        self.phase = Phase::Step { step, len, tries: 1, until: now + ACK_WAIT };
        SetupAction::Send
    }

    /// News from the module.
    fn heard(
        &mut self,
        news: Heard,
        out: &mut Outgoing<'_>,
        now: Instant,
        log: &mut impl FnMut(Event),
    ) -> SetupAction {
        match self.phase {
            // Step 0 too: the port's other settings may not be ours.
            Phase::Listen { .. } if news.message => {
                log(Event::ModuleFound(MODULE_BAUD));
                (self.run.save, self.run.failed) = (false, false);
                self.begin(0, out, now, log)
            }
            Phase::Step { step, .. } => match news.ack {
                Some(ack) if Some(ack.message) == message_id(out.message) => {
                    if !ack.accepted {
                        log(Event::SetupNak(step));
                        self.run.failed = true;
                    }
                    self.begin(step + 1, out, now, log)
                }
                _ => SetupAction::Nothing,
            },
            _ => SetupAction::Nothing,
        }
    }

    /// Its deadline passed: listening ends in a move from a probed rate
    /// with step 0, and settling in step 1; a step is sent again, up to
    /// [`ACK_TRIES`] times, then given up; a failed set-up's wait ends in
    /// listening again.  A module moved from a probed rate that then
    /// fails is looked for at the next rate.
    fn expired(&mut self, out: &mut Outgoing<'_>, now: Instant, log: &mut impl FnMut(Event)) -> SetupAction {
        match self.phase {
            Phase::Listen { .. } => {
                log(Event::ModuleMoved);
                let len = out.module.setup(PORT_STEP, out.message);
                self.phase = Phase::Settle { len, until: now + MOVE_SETTLE };
                SetupAction::Move(PROBE_BAUDS[self.run.probe])
            }
            Phase::Settle { .. } => {
                let probe = (self.run.probe + 1) % PROBE_BAUDS.len();
                self.run = Run { probe, save: true, failed: false, ..self.run };
                match self.begin(1, out, now, log) {
                    SetupAction::Send => SetupAction::Settled,
                    action => action,
                }
            }
            Phase::Step { step, len, tries, .. } if tries < ACK_TRIES => {
                self.phase = Phase::Step { step, len, tries: tries + 1, until: now + ACK_WAIT };
                SetupAction::Send
            }
            Phase::Step { step, .. } => {
                log(Event::SetupTimeout(step));
                self.run.failed = true;
                self.begin(step + 1, out, now, log)
            }
            Phase::Retry { .. } => SetupAction::Reset,
            Phase::Done => SetupAction::Nothing,
        }
    }
}

/// What the emulator keeps: several kilobytes, so built in place, as a
/// static on the STM32, never on the stack (AGENTS.md, Stack and RAM).
#[derive(Debug)]
pub struct State {
    /// The Oncore.
    emulator: Emulator,
    /// The module's messages, read.
    gnss: Gnss,
    /// The module's settings, and its messages encoded.
    module: Module,
    /// What goes out to the instrument, gathered, and to the module,
    /// queued.
    oncore_out: [u8; ONCORE_OUT],
    requests: Requests,
    /// What came in from the module and from the instrument, read.
    module_in: [u8; MODULE_READ],
    oncore_in: [u8; ONCORE_READ],
    /// The set-up message being sent.
    setup_out: [u8; REQUEST_BYTES],
}

impl State {
    /// Before anything is heard; [`run`] gives it its identities.
    #[expect(
        clippy::large_stack_arrays,
        reason = "a const fn: the firmware builds it at compile time, in a static"
    )]
    pub const fn new() -> Self {
        Self {
            emulator: Emulator::new(Identities::VP, Configuration::FACTORY, ReceiverControl::Commanded),
            gnss: Gnss::new(),
            module: Module::new(),
            oncore_out: [0; ONCORE_OUT],
            requests: Requests::new(),
            module_in: [0; MODULE_READ],
            oncore_in: [0; ONCORE_READ],
            setup_out: [0; REQUEST_BYTES],
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether [`run`] sets the module up, or leaves it as it is: for a
/// recording or a module another program owns, which must already send
/// the messages the engine reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleSetup {
    Configure,
    AsIs,
}

/// What [`run`] starts with besides its state and ports: the identities
/// the engine answers as and how it powers up (as a receiver left it
/// configured, for one that cannot command it), the GPS week of the
/// board's build, and whether the module is set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub identities: Identities,
    pub built_week: GpsWeek,
    pub module: ModuleSetup,
    pub configuration: Configuration,
}

/// Runs the emulator in `state`: the module on `gnss`, the instrument on
/// `oncore`, what happens to `host`'s log, `host`'s watchdog fed each
/// pass.  The engine starts at power-up; the rest of `state` is not
/// reset: a second call goes on with navigation and the module's parsers
/// as the first left them, and starts the instrument's command framer
/// afresh, losing a command half read.  Returns on the bench's reset,
/// `@@Zr`, or knock, `@@Zb`, its answer sent, saying which.
pub async fn run(
    state: &mut State,
    options: Options,
    gnss_port: &mut impl ModulePort,
    oncore_port: &mut (impl Read + ReadReady + Write),
    mut host: impl Host,
) -> Exit {
    let State { emulator, gnss, module, oncore_out, requests, module_in, oncore_in, setup_out } = state;
    let Options { identities, built_week, module: setup_wanted, configuration } = options;
    let configure = setup_wanted == ModuleSetup::Configure;
    let receiver = if configure { ReceiverControl::Commanded } else { ReceiverControl::Uncommanded };
    emulator.start(identities, configuration, receiver);
    module.week_rollover_base = built_week;
    let mut serve = Serve {
        decoder: Deframer::new(command_length),
        epochs: 0,
        next_second: Instant::now() + EPOCH_WAIT,
        quiet: Seconds::new(0),
        exit: None,
    };
    let mut setup = if configure { restart_module(gnss_port, Run::FIRST).await } else { Setup::done() };
    loop {
        host.alive();
        let mut log = |event| host.log(event);
        let idle = !gnss_port.read_ready().unwrap_or(true) && !oncore_port.read_ready().unwrap_or(true);
        if idle && emulator.background() {
            embassy_futures::yield_now().await;
            continue;
        }
        requests.open = configure && setup.serving();
        let answer = emulator.deadline().map(|due| Instant::from_millis(due.0));
        let wake = Timer::at(
            [setup.deadline(), answer, requests.deadline()]
                .into_iter()
                .flatten()
                .fold(serve.next_second, Instant::min),
        );
        let mut out = Outbox::new(oncore_out, requests, module);
        let action = match select3(gnss_port.read(module_in), oncore_port.read(oncore_in), wake).await {
            Either3::First(Ok(n)) => {
                let message = serve.module_bytes(&module_in[..n], gnss, emulator, &mut out);
                let ack = gnss.take_ack();
                if let Some(mode) = ack.and_then(|ack| out.requests.answered(ack, Instant::now())) {
                    emulator.module_mode(mode);
                }
                let news = Heard { message, ack };
                let mut outgoing = Outgoing { module: out.encoder, message: setup_out };
                setup.heard(news, &mut outgoing, Instant::now(), &mut log)
            }
            Either3::Second(Ok(n)) => {
                serve.instrument_bytes(&oncore_in[..n], emulator, &mut out, &mut log);
                SetupAction::Nothing
            }
            _ => SetupAction::Nothing,
        };
        // Whichever woke the loop: a busy port must not hold off a
        // deadline.
        let now = Instant::now();
        let action =
            setup.due(action, &mut Outgoing { module: out.encoder, message: setup_out }, now, &mut log);
        if serve.next_second <= now {
            serve.silent_second(emulator, &mut out, &mut log);
        }
        let reset_module = configure && serve.quiet >= MODULE_RESET_SILENT;
        // The set-up may have ended this pass.
        out.requests.open = configure && setup.serving();
        {
            let failed = core::mem::take(&mut out.requests.failed);
            let (answers, mut request) = out.split();
            emulator.poll(milliseconds(now), answers);
            if action == SetupAction::Finish && setup.set_up() {
                emulator.module_mode(ReceiverMode::Navigating);
            }
            if action == SetupAction::Finish || failed {
                emulator.configure(&mut request);
            }
        }
        out.log_drops(&mut log);
        let oncore_len = out.oncore.len;
        if oncore_len > 0 {
            let _ = oncore_port.write_all(&oncore_out[..oncore_len]).await;
        }
        if let Some(exit) = serve.exit {
            let _ = oncore_port.flush().await;
            log(match exit {
                Exit::Reset => Event::Reset,
                Exit::Bootloader => Event::Bootloader,
            });
            return exit;
        }
        if reset_module || action == SetupAction::Reset {
            serve.quiet = Seconds::new(0);
            // A message cut short by the reset is dropped.
            emulator.lost();
            gnss.reset();
            requests.clear();
            if reset_module {
                log(Event::ModuleReset);
            }
            setup = restart_module(gnss_port, setup.run).await;
            continue;
        }
        carry_out(action, &setup, gnss_port, setup_out).await;
        if let Some(frame) = requests.due(Instant::now()) {
            let _ = gnss_port.write_all(frame).await;
            requests.sent();
        }
        requests.log(&mut log);
    }
}

/// The module reset and listened for at [`MODULE_BAUD`], for a set-up
/// going on from `after`'s search.
async fn restart_module(gnss_port: &mut impl ModulePort, after: Run) -> Setup {
    gnss_port.reset().await;
    gnss_port.set_baud(MODULE_BAUD).await;
    Setup::listen(Instant::now(), after)
}

/// The set-up's `action` on the module's port: its baud rate moved, and
/// the message in `setup_out` sent.
async fn carry_out(
    action: SetupAction,
    setup: &Setup,
    gnss_port: &mut impl ModulePort,
    setup_out: &[u8; REQUEST_BYTES],
) {
    match action {
        SetupAction::Move(baud) => gnss_port.set_baud(baud).await,
        SetupAction::Settled => gnss_port.set_baud(MODULE_BAUD).await,
        SetupAction::Nothing | SetupAction::Send | SetupAction::Finish | SetupAction::Reset => {}
    }
    if matches!(action, SetupAction::Send | SetupAction::Move(_) | SetupAction::Settled)
        && let Some(len) = setup.sending()
    {
        let _ = gnss_port.write_all(&setup_out[..len]).await;
    }
}

/// `now` as the engine's clock reads it.
fn milliseconds(now: Instant) -> Milliseconds<u64> {
    Milliseconds(now.as_millis())
}

/// The length of a command the instrument may send: smartclock-gps's
/// commands, and the bench's.
pub fn command_length(id: MessageId) -> Option<usize> {
    match BENCH_COMMANDS.iter().find(|(bench, ..)| *bench == id) {
        Some((_, argument, _)) => Some(oncore::OVERHEAD + argument.len()),
        None => oncore::command_length(id),
    }
}

/// The main loop's own state, beside [`State`]'s.
struct Serve {
    /// The instrument's commands, framed for the log and the bench's
    /// commands; the engine frames them itself.
    decoder: Deframer<MAX_COMMAND>,
    /// Epochs from the module so far.
    epochs: u32,
    /// When the second runs on its own, if no epoch comes first.
    next_second: Instant,
    /// Seconds run on the timer since the module's last message or reset.
    quiet: Seconds<u8>,
    /// A bench command that ends the run has been answered.
    exit: Option<Exit>,
}

impl Serve {
    /// Bytes from the module: each epoch's end, its NAV-EOE, runs the
    /// emulator's second, and every [`SELECT_EPOCHS`] the alert.  Returns
    /// whether a whole message was read.
    fn module_bytes(
        &mut self,
        bytes: &[u8],
        gnss: &mut Gnss,
        emulator: &mut Emulator,
        out: &mut Outbox<'_>,
    ) -> bool {
        let mut heard = false;
        for &b in bytes {
            let message = gnss.push(b, emulator, out.encoder);
            heard |= message.is_some();
            if heard {
                self.quiet = Seconds::new(0);
            }
            if message == Some(NAV_EOE) {
                let (answers, mut request) = out.split();
                emulator.second(milliseconds(Instant::now()), answers, &mut request);
                self.next_second = Instant::now() + EPOCH_WAIT;
                self.epochs += 1;
                if self.epochs.is_multiple_of(SELECT_EPOCHS) {
                    emulator.alert();
                }
            }
        }
        heard
    }

    /// Bytes from the instrument: the engine's answers, each command
    /// logged, and the bench's commands answered.
    fn instrument_bytes(
        &mut self,
        bytes: &[u8],
        emulator: &mut Emulator,
        out: &mut Outbox<'_>,
        log: &mut impl FnMut(Event),
    ) {
        {
            let (answers, mut request) = out.split();
            emulator.command(bytes, milliseconds(Instant::now()), answers, &mut request);
        }
        for &b in bytes {
            match self.decoder.push(b) {
                Some(Ok(frame)) => {
                    let id = frame.id();
                    if let Some(&(_, argument, exit)) = BENCH_COMMANDS
                        .iter()
                        .find(|(bench, argument, _)| *bench == id && frame.data() == argument)
                    {
                        let mut b = [0u8; MAX_COMMAND];
                        if let Ok(n) = oncore::encode(id, &argument, &mut b) {
                            out.oncore.append(&b[..n]);
                        }
                        self.exit = Some(exit);
                    }
                    log(Event::Command(id));
                }
                Some(Err(FrameError::BadChecksum { id, .. } | FrameError::BadTerminator { id, .. })) => {
                    log(Event::BadFrame(id));
                }
                Some(Err(FrameError::UnknownId(id) | FrameError::Overflow { id, .. })) => {
                    log(Event::UnknownCommand(id));
                }
                Some(Err(FrameError::BadId(b))) => log(Event::BadId(b)),
                None => {}
            }
        }
    }

    /// No epoch in time: the second on the timer.
    fn silent_second(&mut self, emulator: &mut Emulator, out: &mut Outbox<'_>, log: &mut impl FnMut(Event)) {
        let (answers, mut request) = out.split();
        emulator.silent_second(milliseconds(Instant::now()), answers, &mut request);
        self.next_second += SECOND;
        self.quiet = self.quiet.saturating_increment();
        log(Event::Silent);
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec::Vec;

    use embassy_time::Instant;
    use oncore_core::gnss::Ack;
    use oncore_core::module::Module;
    use oncore_core::module::ModuleRequest;
    use oncore_core::module::REQUEST_BYTES;
    use oncore_core::module::SAVE_STEP;
    use oncore_core::module::SETUP_STEPS;
    use smartclock_gps::engine::settings::Height;
    use smartclock_gps::engine::settings::HeightKind;
    use smartclock_gps::engine::settings::Location;
    use smartclock_gps::engine::settings::TimeBase;
    use smartclock_gps::record::ReceiverMode;
    use smartclock_gps::units::Centimeters;
    use smartclock_gps::units::Degrees;
    use smartclock_gps::units::MilliArcSeconds;
    use smartclock_ubx::frame::message_id;

    use super::ACK_WAIT;
    use super::Event;
    use super::Heard;
    use super::MODULE_BAUD;
    use super::MODULE_DEFAULT_BAUD;
    use super::Outbox;
    use super::Outgoing;
    use super::PROBE_BAUDS;
    use super::Phase;
    use super::REQUESTS_QUEUED;
    use super::Requests;
    use super::Run;
    use super::SETUP_RETRY;
    use super::SETUP_RETRY_MOST;
    use super::Setup;
    use super::SetupAction;

    /// When the tests' set-ups start.
    const START_S: u64 = 1;

    /// Runs the set-up from a module heard at its rate, answering step
    /// `nak` with a NAK and every other with an ACK; returns the steps
    /// sent and the events.
    fn answered(nak: usize) -> (Setup, Vec<usize>, Vec<Event>) {
        let module = Module::new();
        let mut message = [0; REQUEST_BYTES];
        let mut events = Vec::new();
        let mut log = |e| events.push(e);
        let now = Instant::from_secs(START_S);
        let mut setup = Setup::listen(now, Run::FIRST);
        let mut sent = Vec::new();
        let mut out = Outgoing { module: &module, message: &mut message };
        let mut action = setup.heard(Heard { message: true, ack: None }, &mut out, now, &mut log);
        while action == SetupAction::Send {
            let Phase::Step { step, .. } = setup.phase else { unreachable!() };
            sent.push(step);
            let ack = message_id(out.message).map(|message| Ack { accepted: step != nak, message });
            action = setup.heard(Heard { message: true, ack }, &mut out, now, &mut log);
        }
        assert_eq!(action, SetupAction::Finish);
        (setup, sent, events)
    }

    #[test]
    fn a_module_heard_is_set_up_from_its_port_and_not_saved() {
        let (setup, sent, events) = answered(usize::MAX);
        assert_eq!(sent, (0..SAVE_STEP).collect::<Vec<_>>());
        assert_eq!(setup.phase, Phase::Done);
        assert_eq!(events, [Event::ModuleFound(MODULE_BAUD), Event::SetupDone]);
    }

    /// A step the module refuses, any but the save: the second CFG-MSG.
    const REFUSED: usize = 3;

    #[test]
    fn a_refused_step_fails_the_set_up_which_resets_the_module() {
        let (mut setup, sent, events) = answered(REFUSED);
        assert_eq!(sent, (0..SAVE_STEP).collect::<Vec<_>>());
        assert_eq!(events[1..], [Event::SetupNak(REFUSED), Event::SetupFailed]);
        assert!(setup.serving());
        let (Phase::Retry { until }, 0) = (setup.phase, setup.run.probe) else { panic!("{setup:?}") };
        assert_eq!(until, Instant::from_secs(START_S) + SETUP_RETRY);
        let (module, mut message) = (Module::new(), [0; REQUEST_BYTES]);
        let action =
            setup.expired(&mut Outgoing { module: &module, message: &mut message }, until, &mut |_| {});
        assert_eq!(action, SetupAction::Reset);
    }

    /// Each failure in a row doubles the wait before the next set-up, up
    /// to the most; one that succeeds starts it over.
    #[test]
    fn failed_set_ups_back_off() {
        let (module, mut message) = (Module::new(), [0; REQUEST_BYTES]);
        let mut run = Run::FIRST;
        let mut waits = Vec::new();
        while waits.last() != Some(&SETUP_RETRY_MOST) {
            let mut setup = Setup::listen(Instant::from_secs(START_S), run);
            setup.run.failed = true;
            let now = Instant::from_secs(START_S);
            let mut out = Outgoing { module: &module, message: &mut message };
            assert_eq!(setup.begin(SETUP_STEPS, &mut out, now, &mut |_| {}), SetupAction::Finish);
            let Phase::Retry { until } = setup.phase else { panic!("{setup:?}") };
            waits.push(until - now);
            run = setup.run;
        }
        assert_eq!(waits[..2], [SETUP_RETRY, SETUP_RETRY * 2]);
        let mut setup = Setup::listen(Instant::from_secs(START_S), run);
        let mut out = Outgoing { module: &module, message: &mut message };
        setup.begin(SETUP_STEPS, &mut out, Instant::from_secs(START_S), &mut |_| {});
        assert_eq!((setup.phase, setup.run.retry), (Phase::Done, SETUP_RETRY));
    }

    /// A module never heard, reset for its silence after each probe: the
    /// search goes on from the next rate, not the first.
    #[test]
    fn the_search_goes_on_across_a_module_reset() {
        let (module, mut message) = (Module::new(), [0; REQUEST_BYTES]);
        let mut run = Run::FIRST;
        let mut moves = Vec::new();
        while moves.len() < PROBE_BAUDS.len() {
            let mut setup = Setup::listen(Instant::from_secs(START_S), run);
            // Listening ends in a move, and settling in the next probe.
            for _ in 0..2 {
                let now = setup.deadline().unwrap();
                let mut out = Outgoing { module: &module, message: &mut message };
                if let SetupAction::Move(baud) = setup.expired(&mut out, now, &mut |_| {}) {
                    moves.push(baud);
                }
            }
            run = setup.run;
        }
        assert_eq!(moves, PROBE_BAUDS);
    }

    /// Two CFG messages and their ACKs, as the queue takes them.
    fn two_requests() -> (Requests, [Vec<u8>; 2]) {
        let mut module = Module::new();
        let frames =
            [ModuleRequest::MaskAngle(Degrees(MASK)), ModuleRequest::TimeBase(TimeBase::Gps)].map(|r| {
                let mut b = [0; REQUEST_BYTES];
                let n = module.encode(r, &mut b).expect("a message");
                b[..n].to_vec()
            });
        let mut requests = Requests::new();
        requests.open = true;
        for f in &frames {
            requests.queue(f, true, None);
        }
        (requests, frames)
    }

    fn ack(frame: &[u8], accepted: bool) -> Ack {
        Ack { accepted, message: message_id(frame).expect("a frame") }
    }

    /// A mask angle, degrees.
    const MASK: u8 = 10;

    /// One at a time: the second only once the first is acknowledged.
    #[test]
    fn requests_go_one_at_a_time_on_their_acks() {
        let (mut requests, frames) = two_requests();
        let now = Instant::from_secs(START_S);
        assert_eq!(requests.due(now), Some(&frames[0][..]));
        requests.sent();
        assert_eq!(requests.due(now), None);
        requests.answered(ack(&frames[0], true), now);
        assert_eq!(requests.due(now), Some(&frames[1][..]));
    }

    /// A NAK sends it again at once, a missing ACK after the wait; after
    /// its tries it is given up and the failure marked.
    #[test]
    fn a_refused_request_is_sent_again_then_given_up() {
        let (mut requests, frames) = two_requests();
        let mut now = Instant::from_secs(START_S);
        assert!(requests.due(now).is_some());
        requests.answered(ack(&frames[0], false), now);
        assert_eq!(requests.due(now), Some(&frames[0][..]));
        now += ACK_WAIT;
        assert_eq!(requests.due(now), Some(&frames[0][..]));
        now += ACK_WAIT;
        assert_eq!(requests.due(now), None);
        assert!(requests.failed);
        assert_eq!(requests.due(now), Some(&frames[1][..]));
    }

    /// A position hold's ACK gives the module's mode; its NAK, and an ACK
    /// of a request that sets no mode, give none.
    #[test]
    fn a_hold_s_ack_gives_the_mode() {
        let mut module = Module::new();
        let mut b = [0; REQUEST_BYTES];
        let hold = ModuleRequest::PositionHold(None);
        let n = module.encode(hold, &mut b).expect("a message");
        let mut requests = Requests::new();
        requests.open = true;
        requests.queue(&b[..n], true, hold.mode_once_acknowledged());
        let now = Instant::from_secs(START_S);
        assert!(requests.due(now).is_some());
        assert_eq!(requests.answered(ack(&b[..n], false), now), None);
        assert_eq!(requests.answered(ack(&b[..n], true), now), Some(ReceiverMode::Navigating));
        let (mut plain, frames) = two_requests();
        assert!(plain.due(now).is_some());
        assert_eq!(plain.answered(ack(&frames[0], true), now), None);
    }

    /// A held position, 37.8 N, 122 W, 100 m, of `kind`.
    fn held(kind: HeightKind) -> ModuleRequest {
        const LATITUDE: MilliArcSeconds = MilliArcSeconds(136_080_000);
        const LONGITUDE: MilliArcSeconds = MilliArcSeconds(-439_200_000);
        const HEIGHT: Centimeters = Centimeters(10_000);
        let height = Height { height: HEIGHT, kind };
        ModuleRequest::PositionHold(Some(Location { latitude: LATITUDE, longitude: LONGITUDE, height }))
    }

    /// Before the module's first fix, an MSL position hold is withheld:
    /// not queued, so no ACK of it sets the mode.  An ellipsoid one, the
    /// module's own datum, is queued with its mode.
    #[test]
    fn a_withheld_hold_is_not_queued() {
        let (mut module, mut requests) = (Module::new(), Requests::new());
        requests.open = true;
        let mut queue = |request| {
            let mut out = Outbox::new(&mut [], &mut requests, &mut module);
            out.split().1(request);
        };
        queue(held(HeightKind::MeanSeaLevel));
        queue(held(HeightKind::Ellipsoid));
        assert_eq!(requests.modes[..requests.waiting], [Some(ReceiverMode::Holding)]);
    }

    /// MGA-INI goes without waiting; a closed queue takes nothing, a full
    /// one marks the drop.
    #[test]
    fn unacknowledged_closed_and_full() {
        let (mut requests, frames) = two_requests();
        let mut fresh = Requests::new();
        fresh.queue(&frames[0], true, None);
        assert_eq!(fresh.waiting, 0);
        fresh.open = true;
        fresh.queue(&frames[0], false, None);
        assert!(fresh.due(Instant::from_secs(START_S)).is_some());
        fresh.sent();
        assert_eq!(fresh.waiting, 0);
        while !requests.dropped {
            requests.queue(&frames[0], true, None);
        }
        assert_eq!(requests.waiting, REQUESTS_QUEUED);
    }

    /// A module never heard: each set-up moves it from the next rate.
    #[test]
    fn a_module_not_heard_is_looked_for_at_each_rate() {
        let module = Module::new();
        let mut message = [0; REQUEST_BYTES];
        let mut setup = Setup::listen(Instant::from_secs(START_S), Run::FIRST);
        let mut moves = Vec::new();
        let mut saved = false;
        while moves.len() < PROBE_BAUDS.len() + 1 {
            let now = setup.deadline().unwrap();
            match setup.expired(&mut Outgoing { module: &module, message: &mut message }, now, &mut |_| {}) {
                SetupAction::Move(baud) => moves.push(baud),
                SetupAction::Settled | SetupAction::Send => {
                    if let Phase::Step { step, tries: 1, .. } = setup.phase {
                        saved |= step == SAVE_STEP;
                    }
                }
                SetupAction::Reset => setup = Setup::listen(now, setup.run),
                _ => {}
            }
        }
        assert_eq!(moves, [&PROBE_BAUDS[..], &[MODULE_DEFAULT_BAUD][..]].concat());
        assert!(!saved, "a set-up with no answers is never saved");
    }
}
