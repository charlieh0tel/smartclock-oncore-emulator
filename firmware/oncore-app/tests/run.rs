//! `run` between a fake module, which acknowledges every configuration
//! message and ends an epoch every 100 ms, or a silent one, and a fake
//! instrument.

use core::convert::Infallible;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use embassy_futures::block_on;
use embassy_futures::select::Either;
use embassy_futures::select::select;
use embassy_time::Duration;
use embassy_time::Timer;
use embassy_time::with_timeout;
use embedded_io_async::ErrorType;
use embedded_io_async::Read;
use embedded_io_async::ReadReady;
use embedded_io_async::Write;
use oncore_app::BOOTLOADER_ARGUMENT;
use oncore_app::Event;
use oncore_app::Exit;
use oncore_app::MODULE_DEFAULT_BAUD;
use oncore_app::MODULE_RESET_SILENT;
use oncore_app::ModulePort;
use oncore_app::ModuleSetup;
use oncore_app::ONCORE_BAUD;
use oncore_app::Options;
use oncore_app::RESET_ARGUMENT;
use oncore_app::State;
use oncore_app::ZB_BOOTLOADER;
use oncore_app::ZR_RESET;
use oncore_app::command_length;
use oncore_app::run;
use oncore_core::emulator::Configuration;
use oncore_core::identity::BoardIdentity;
use oncore_core::identity::Identities;
use oncore_core::module::MODULE_BAUD;
use oncore_core::units::Baud;
use oncore_core::wire::UbxReader;
use smartclock_gps::oncore::AG_MASK_ANGLE;
use smartclock_gps::oncore::CA_SELF_TEST;
use smartclock_gps::oncore::CG_FIX_MODE;
use smartclock_gps::oncore::CJ_RECEIVER_ID;
use smartclock_gps::oncore::Deframer;
use smartclock_gps::oncore::EA_POSITION_STATUS;
use smartclock_gps::oncore::EN_TIME_RAIM;
use smartclock_gps::oncore::FA_SELF_TEST;
use smartclock_gps::oncore::MessageId;
use smartclock_gps::oncore::OVERHEAD;
use smartclock_gps::oncore::encode;
use smartclock_gps::oncore::response_length;
use smartclock_ubx::frame;
use smartclock_ubx::message::ACK_ACK;
use smartclock_ubx::message::CFG_ANT;
use smartclock_ubx::message::CFG_CFG;
use smartclock_ubx::message::CFG_NAV5;
use smartclock_ubx::message::CFG_PRT;
use smartclock_ubx::message::CFG_TP5;
use smartclock_ubx::message::NAV_EOE;
use smartclock_ubx::message::PayloadLength;
use smartclock_ubx::message::payload_length;

/// The configuration messages the fake module has taken: class and ID,
/// payload.
type Seen = Rc<RefCell<Vec<(frame::MessageId, Vec<u8>)>>>;

/// One direction of a link: the bytes written to it and not yet read,
/// shared by its two ends.
#[derive(Clone, Default)]
struct Pipe(Rc<RefCell<VecDeque<u8>>>);

/// One end of a link: reads one pipe, writes the other.
struct End {
    rx: Pipe,
    tx: Pipe,
    baud: Rc<RefCell<Baud>>,
    /// Module resets asked for.
    resets: Rc<RefCell<u32>>,
}

impl ErrorType for End {
    type Error = Infallible;
}

impl Read for End {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        loop {
            {
                let mut q = self.rx.0.borrow_mut();
                if !q.is_empty() {
                    let n = buf.len().min(q.len());
                    for b in buf.iter_mut().take(n) {
                        *b = q.pop_front().unwrap();
                    }
                    return Ok(n);
                }
            }
            Timer::after(TICK).await;
        }
    }
}

impl ReadReady for End {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        Ok(!self.rx.0.borrow().is_empty())
    }
}

impl Write for End {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.tx.0.borrow_mut().extend(buf);
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ModulePort for End {
    async fn set_baud(&mut self, baud: Baud) {
        *self.baud.borrow_mut() = baud;
    }

    async fn reset(&mut self) {
        *self.resets.borrow_mut() += 1;
    }
}

/// The mask angle the instrument sets, degrees, and CFG-NAV5's mask
/// when it sets that alone, minElev's bit.
const MASK_ANGLE: u8 = 5;
const MIN_ELEVATION_ONLY: u16 = 1 << 1;

/// CFG-NAV5's mask and minElev, its fields read in order.
fn nav5_mask_angle(p: &[u8]) -> (u16, u8) {
    let mut r = UbxReader::new(p);
    let (mask, _dynamic_model, _fix_mode, _fixed_altitude, _variance) =
        (r.u16(), r.u8(), r.u8(), r.i32(), r.u32());
    (mask, r.u8())
}

fn framed(id: frame::MessageId, p: &[u8]) -> Vec<u8> {
    let mut out = vec![0; p.len() + frame::OVERHEAD];
    frame::encode(id, p, &mut out).unwrap();
    out
}

/// The configuration messages' class.
const CLASS_CFG: u8 = CFG_PRT.class;

/// The fake module: acknowledges each CFG message it is sent, keeps
/// them, and ends an epoch every 100 ms.
async fn module(from_emulator: Pipe, to_emulator: Pipe, seen: Seen) {
    let Some(PayloadLength::Fixed(eoe_length)) = payload_length(NAV_EOE) else { unreachable!() };
    let mut d = frame::Deframer::<MODULE_ROOM>::new();
    loop {
        let bytes: Vec<u8> = from_emulator.0.borrow_mut().drain(..).collect();
        for b in bytes {
            if let Some(Ok(f)) = d.push(b) {
                let id = f.id();
                seen.borrow_mut().push((id, f.payload().to_vec()));
                if id.class == CLASS_CFG {
                    to_emulator.0.borrow_mut().extend(framed(ACK_ACK, &[id.class, id.id]));
                }
            }
        }
        to_emulator.0.borrow_mut().extend(framed(NAV_EOE, &vec![0; eoe_length]));
        Timer::after(EPOCH).await;
    }
}

/// A run's links and what it left: the module's pipes, to it and from
/// it, and the instrument's; the module port's baud and resets; the
/// events `run` logged; and the configuration messages the fake module
/// took.
#[derive(Default)]
struct Bench {
    to_module: Pipe,
    from_module: Pipe,
    to_onc: Pipe,
    from_onc: Pipe,
    baud: Rc<RefCell<Baud>>,
    resets: Rc<RefCell<u32>>,
    events: Rc<RefCell<Vec<Event>>>,
    seen: Seen,
}

impl Bench {
    /// `run` with `options` until `test` ends or `run` returns: why it
    /// returned, if it did.
    fn run(&self, options: Options, test: impl Future<Output = ()>) -> Option<Exit> {
        let mut gnss = End {
            rx: self.from_module.clone(),
            tx: self.to_module.clone(),
            baud: self.baud.clone(),
            resets: self.resets.clone(),
        };
        let mut onc = End {
            rx: self.to_onc.clone(),
            tx: self.from_onc.clone(),
            baud: Rc::new(RefCell::new(ONCORE_BAUD)),
            resets: Rc::default(),
        };
        let events = self.events.clone();
        let log = move |e: Event| events.borrow_mut().push(e);
        block_on(async {
            let mut state = Box::new(State::new());
            match select(run(&mut state, options, &mut gnss, &mut onc, log), test).await {
                Either::First(exit) => Some(exit),
                Either::Second(()) => None,
            }
        })
    }

    /// `check` with the fake module answering, within [`TIMEOUT`].
    async fn with_module(&self, check: impl Future<Output = ()>) {
        let fake = module(self.to_module.clone(), self.from_module.clone(), self.seen.clone());
        match select(fake, with_timeout(TIMEOUT, check)).await {
            Either::First(()) => unreachable!(),
            Either::Second(r) => r.expect("timed out"),
        }
    }

    /// Sends a command and waits for one answer.
    async fn ask(&self, id: MessageId, data: &[u8]) -> (MessageId, Vec<u8>) {
        let mut out = [0u8; COMMAND_ROOM];
        let n = encode(id, data, &mut out).unwrap();
        self.to_onc.0.borrow_mut().extend(&out[..n]);
        let mut d = Deframer::<ANSWER_ROOM>::new(answer_length);
        loop {
            let bytes: Vec<u8> = self.from_onc.0.borrow_mut().drain(..).collect();
            for b in bytes {
                if let Some(Ok(frame)) = d.push(b) {
                    return (frame.id(), frame.data().to_vec());
                }
            }
            Timer::after(POLL).await;
        }
    }

    /// Whether `run` has logged `event`.
    fn logged(&self, event: Event) -> bool {
        self.events.borrow().contains(&event)
    }

    /// Until `run` logs `event`.
    async fn until(&self, event: Event) {
        while !self.logged(event) {
            Timer::after(POLL).await;
        }
    }
}

/// A module heard at 115200 at once, since the fake module talks at any
/// rate, is set up: its port, time pulse, message rates and antenna, and
/// no save, as it has one.
#[test]
fn sets_the_module_up() {
    let bench = Bench::default();
    assert_eq!(
        None,
        bench.run(
            CONFIGURE,
            bench.with_module(async {
                bench.until(Event::SetupDone).await;
                Timer::after(SETTLE).await;
            })
        )
    );
    let events = bench.events.borrow();
    assert_eq!(events[0], Event::ModuleFound(MODULE_BAUD));
    assert!(!events.iter().any(|e| matches!(e, Event::SetupNak(_) | Event::SetupTimeout(_))));
    let seen = bench.seen.borrow();
    assert!(seen.iter().any(|(id, _)| *id == CFG_ANT));
    assert!(!seen.iter().any(|(id, _)| *id == CFG_CFG));
    assert_eq!(*bench.baud.borrow(), MODULE_BAUD);
}

/// `@@Cj` is answered, and ten at once each: a pass builds one answer at
/// most, and none is dropped.
#[test]
fn answers_a_burst_of_commands() {
    let bench = Bench::default();
    assert_eq!(
        None,
        bench.run(
            CONFIGURE,
            bench.with_module(async {
                bench.until(Event::SetupDone).await;
                let (id, data) = bench.ask(CJ_RECEIVER_ID, &[]).await;
                assert_eq!(id, CJ_RECEIVER_ID);
                assert_eq!(Some(data.len() + OVERHEAD), response_length(CJ_RECEIVER_ID));
                let mut burst = [0u8; OVERHEAD];
                let n = encode(CJ_RECEIVER_ID, &[], &mut burst).unwrap();
                for _ in 0..BURST {
                    bench.to_onc.0.borrow_mut().extend(&burst[..n]);
                }
                let mut d = Deframer::<ANSWER_ROOM>::new(answer_length);
                let mut answers = 0;
                while answers < BURST {
                    let bytes: Vec<u8> = bench.from_onc.0.borrow_mut().drain(..).collect();
                    answers += bytes
                        .iter()
                        .filter(|&&b| matches!(d.push(b), Some(Ok(frame)) if frame.id() == CJ_RECEIVER_ID))
                        .count();
                    Timer::after(POLL).await;
                }
            })
        )
    );
    assert!(!bench.logged(Event::AnswerDropped));
}

/// The instrument's mask angle, set during the set-up, is answered at
/// once; its module request is held until the set-up's end, then sent:
/// CFG-NAV5 with minElev alone.
#[test]
fn the_mask_angle_passes_through() {
    let bench = Bench::default();
    assert_eq!(
        None,
        bench.run(
            CONFIGURE,
            bench.with_module(async {
                let answer = bench.ask(AG_MASK_ANGLE, &[MASK_ANGLE]).await;
                assert_eq!(answer, (AG_MASK_ANGLE, vec![MASK_ANGLE]));
                assert!(!bench.logged(Event::SetupDone));
                bench.until(Event::SetupDone).await;
                // Long enough for the fake module to read what followed.
                Timer::after(SETTLE).await;
            })
        )
    );
    assert!(bench.logged(Event::Command(AG_MASK_ANGLE)));
    assert!(
        bench
            .seen
            .borrow()
            .iter()
            .any(|(id, p)| *id == CFG_NAV5 && nav5_mask_angle(p) == (MIN_ELEVATION_ONLY, MASK_ANGLE))
    );
}

/// A module found at 9600 is moved, set up and saved, once.
#[test]
fn a_module_at_9600_is_saved() {
    let bench = Bench::default();
    assert_eq!(
        None,
        bench.run(CONFIGURE, async {
            // Silent until the move from 9600, as a module at its defaults is
            // to one listening at 115200.
            while *bench.baud.borrow() != MODULE_DEFAULT_BAUD {
                Timer::after(TICK).await;
            }
            bench.with_module(bench.until(Event::SetupDone)).await;
        })
    );
    let saves = bench.seen.borrow().iter().filter(|(id, _)| *id == CFG_CFG).count();
    assert_eq!(saves, 1);
}

/// `@@Zr` with the wrong argument changes nothing; with its own it is
/// answered, and `run` returns.
#[test]
fn the_bench_reset_ends_the_run() {
    let bench = Bench::default();
    assert_eq!(
        Some(Exit::Reset),
        bench.run(AS_IS, async {
            let mut out = [0u8; COMMAND_ROOM];
            let n = encode(ZR_RESET, &WRONG_RESET_ARGUMENT, &mut out).unwrap();
            bench.to_onc.0.borrow_mut().extend(&out[..n]);
            Timer::after(ANSWER).await;
            assert!(bench.from_onc.0.borrow().is_empty());
            let answer = with_timeout(ANSWER, bench.ask(ZR_RESET, &RESET_ARGUMENT)).await;
            assert_eq!(answer.expect("answered"), (ZR_RESET, RESET_ARGUMENT.to_vec()));
            Timer::after(RETURN).await;
            panic!("run did not return");
        })
    );
    assert!(bench.logged(Event::Command(ZR_RESET)));
    assert!(bench.logged(Event::Reset));
}

/// `@@Zb` with the wrong argument changes nothing; with its own it is
/// answered, and `run` returns for the bootloader.
#[test]
fn the_bench_knock_ends_the_run_for_the_bootloader() {
    let bench = Bench::default();
    let exit = bench.run(AS_IS, async {
        let mut out = [0u8; COMMAND_ROOM];
        let n = encode(ZB_BOOTLOADER, &WRONG_BOOTLOADER_ARGUMENT, &mut out).unwrap();
        bench.to_onc.0.borrow_mut().extend(&out[..n]);
        Timer::after(ANSWER).await;
        assert!(bench.from_onc.0.borrow().is_empty());
        let answer = with_timeout(ANSWER, bench.ask(ZB_BOOTLOADER, &BOOTLOADER_ARGUMENT)).await;
        assert_eq!(answer.expect("answered"), (ZB_BOOTLOADER, BOOTLOADER_ARGUMENT.to_vec()));
        Timer::after(RETURN).await;
        panic!("run did not return");
    });
    assert_eq!(exit, Some(Exit::Bootloader));
    assert!(bench.logged(Event::Bootloader));
    assert!(!bench.logged(Event::Reset));
}

/// The length of an answer the emulator sends: smartclock-gps's, and
/// the echo of a bench command.
fn answer_length(id: MessageId) -> Option<usize> {
    if [ZR_RESET, ZB_BOOTLOADER].contains(&id) { command_length(id) } else { response_length(id) }
}

/// `run`'s options: the module set up, or left as it is.
const CONFIGURE: Options = options(ModuleSetup::Configure);
const AS_IS: Options = options(ModuleSetup::AsIs);

/// A VP's `run` options, with the module set up as `module` says.
const fn options(module: ModuleSetup) -> Options {
    Options {
        identities: Identities::VP,
        built_week: BoardIdentity::EMULATOR.built_week,
        module,
        configuration: Configuration::FACTORY,
    }
}

/// A Z3817A's receiver, configured as a 58503A leaves its engine before
/// it was last turned off; with no module, its seconds run on the timer.
const SAVED: Options =
    Options { identities: Identities::Z3817A, configuration: Configuration::Z3817A, ..AS_IS };
/// Past the first two seconds on the timer, 1.5 s and 2.5 s in.
const PAST_TWO_SECONDS: Duration = Duration::from_millis(3200);

/// Configured before it was last turned off, the receiver sends its
/// outputs with no command, eight channels' `@@Ea` and `@@En`, and
/// nothing else: no `@@Cg`.
#[test]
fn a_saved_configuration_sends_its_outputs_unasked() {
    let bench = Bench::default();
    assert_eq!(None, bench.run(SAVED, Timer::after(PAST_TWO_SECONDS)));
    let mut d = Deframer::<ANSWER_ROOM>::new(answer_length);
    let bytes: Vec<u8> = bench.from_onc.0.borrow_mut().drain(..).collect();
    let ids: Vec<MessageId> = bytes.iter().filter_map(|&b| d.push(b)?.ok().map(|f| f.id())).collect();
    assert!(ids.contains(&EA_POSITION_STATUS) && ids.contains(&EN_TIME_RAIM), "{ids:?}");
    assert!(!ids.contains(&CG_FIX_MODE), "{ids:?}");
}
/// A reset argument not `@@Zr`'s own, [`RESET_ARGUMENT`].
const WRONG_RESET_ARGUMENT: [u8; RESET_ARGUMENT.len()] = *b"BOOX";
/// A knock argument not `@@Zb`'s own, [`BOOTLOADER_ARGUMENT`].
const WRONG_BOOTLOADER_ARGUMENT: [u8; BOOTLOADER_ARGUMENT.len()] = *b"LOAX";
/// The pipes' waits for bytes, the fake instrument's polls for answers,
/// the time the fake module is given to read what followed, and how
/// long a test may take.
const TICK: Duration = Duration::from_millis(1);
const POLL: Duration = Duration::from_millis(5);
const SETTLE: Duration = Duration::from_millis(250);
const TIMEOUT: Duration = Duration::from_secs(20);
/// How long `run` has to return after the bench's reset.
const RETURN: Duration = Duration::from_secs(1);
/// Room for a module message's payload the fake module reads, for an
/// Oncore command, and for an answer, `@@Cj`'s 294 bytes the longest.
const MODULE_ROOM: usize = 64;
const COMMAND_ROOM: usize = 64;
const ANSWER_ROOM: usize = 300;
/// Past the silent seconds, the first epoch wait's and margin, s.
const RESET_MARGIN_S: u64 = 3;
/// The set-up's first steps a module not heard is sent: CFG-PRT at
/// 9600, then CFG-TP5.
const FIRST_STEPS: [frame::MessageId; 2] = [CFG_PRT, CFG_TP5];
/// The fake module's epochs.
const EPOCH: Duration = Duration::from_millis(100);
/// `@@Cj`s sent at once.
const BURST: usize = 10;
/// How soon the instrument's command is answered, whatever the module.
const ANSWER: Duration = Duration::from_millis(100);
/// How soon the self-test is answered: smartclock-gps's engine takes
/// 6 s, as real VPs took 5 to 7 (`docs/vp-on-jig.md`), and margin.
const SELF_TEST: Duration = Duration::from_millis(6500);
/// Past `run`'s listening for the module, 1.5 s.
const PAST_LISTEN: Duration = Duration::from_millis(1700);

/// With the module silent, the instrument is answered at once, commands
/// back to back, which hold off neither the listening's end nor the
/// move from 9600; and the self-test when its time is up.  Not heard,
/// the module is moved from 9600 with step 0, then sent step 1 at
/// 115200.
#[test]
fn answers_the_instrument_while_the_module_is_silent() {
    let bench = Bench::default();
    assert_eq!(
        None,
        bench.run(CONFIGURE, async {
            while !bench.logged(Event::ModuleMoved) {
                let answer = with_timeout(ANSWER, bench.ask(CJ_RECEIVER_ID, &[])).await;
                assert_eq!(answer.expect("answered while listening").0, CJ_RECEIVER_ID);
            }
            let answer = with_timeout(SELF_TEST, bench.ask(CA_SELF_TEST, &[])).await;
            assert_eq!(answer.expect("answered while moving").0, CA_SELF_TEST);
            Timer::after(PAST_LISTEN).await;
        })
    );
    assert!(bench.logged(Event::Command(CA_SELF_TEST)));
    assert!(bench.logged(Event::ModuleMoved));
    assert!(!bench.logged(Event::SetupDone));
    assert_eq!(*bench.baud.borrow(), MODULE_BAUD);
    let mut d = frame::Deframer::<MODULE_ROOM>::new();
    let sent: Vec<frame::MessageId> = bench
        .to_module
        .0
        .borrow()
        .iter()
        .filter_map(|&b| match d.push(b) {
            Some(Ok(f)) => Some(f.id()),
            _ => None,
        })
        .collect();
    // CFG-PRT, then CFG-TP5.
    assert_eq!(sent[..FIRST_STEPS.len()], FIRST_STEPS);
}

/// With the module silent, an `@@Ea` past the listening frames and
/// answers through run's own decoder, and the engine then takes
/// eight-channel commands and reports the eight-channel model.
#[test]
fn an_eight_channel_command_switches_the_model() {
    let bench = Bench::default();
    assert_eq!(
        None,
        bench.run(CONFIGURE, async {
            bench.until(Event::ModuleMoved).await;
            Timer::after(PAST_LISTEN).await;
            let (id, data) =
                with_timeout(ANSWER, bench.ask(EA_POSITION_STATUS, &[0])).await.expect("answered");
            assert_eq!(id, EA_POSITION_STATUS);
            assert_eq!(Some(data.len() + OVERHEAD), response_length(EA_POSITION_STATUS));
            let answer = with_timeout(SELF_TEST, bench.ask(FA_SELF_TEST, &[])).await;
            assert_eq!(answer.expect("answered at eight channels").0, FA_SELF_TEST);
            let (_, cj) = with_timeout(ANSWER, bench.ask(CJ_RECEIVER_ID, &[])).await.expect("answered");
            assert!(String::from_utf8_lossy(&cj).contains("B4121P1115"));
        })
    );
}

/// A module that never speaks is reset at the start and again after
/// [`MODULE_RESET_SILENT`] of the engine's own seconds.
#[test]
fn a_silent_module_is_reset() {
    let bench = Bench::default();
    // The first epoch wait, the silent seconds, and margin.
    let wait = Timer::after(Duration::from_secs(u64::from(MODULE_RESET_SILENT.get()) + RESET_MARGIN_S));
    assert_eq!(None, bench.run(CONFIGURE, wait));
    // At the start, and once silent.
    assert_eq!(*bench.resets.borrow(), 1 + 1);
    assert_eq!(bench.events.borrow().iter().filter(|e| **e == Event::ModuleReset).count(), 1);
}
