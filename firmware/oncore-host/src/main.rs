//! `oncore_app::run` on a host: the emulator between a module and an
//! instrument (or a pty standing in for one), for the bench before the
//! board, and against recorded module output.
//!
//! ```text
//! oncore-host --module serial:/dev/ttyACM0 --oncore /dev/ttyUSB0
//! oncore-host --module file:capture.ubx --oncore /dev/pts/7
//! oncore-host --module gpsd:apu-0:2947 --oncore /dev/pts/7
//! oncore-host --module file:capture.ubx --oncore tcp:127.0.0.1:5027
//! oncore-host --module gpsd:apu-0:2947 --oncore tcp:shack:2002 --initial-config z3817a
//! ```
//!
//! A serial module is set up as on the board.  A recording (`file:`,
//! raw UBX as `gpspipe -R` writes it) is played at the module's rate,
//! and a gpsd device (`gpsd:`, its raw stream) is only listened to:
//! neither is configured, so either must already carry the messages the
//! engine reads.  `--oncore tcp:HOST:PORT` connects to a server that
//! stands in for the instrument, running its image.
//! `--model ut-plus` presents the UT Plus (R5122U1152, software 3.1), as
//! the board's `ut-plus` build does, for the 58503B's and Z3816A's UT
//! Plus path; `vp-six`, the six-channel VP alone, as its `vp-six` build;
//! `vp`, the default, the VP.
//! `--initial-config 58503a|z3801a|z3805a|z3817a` powers the engine up as
//! that instrument left its receiver configured (smartclock-gps's saved
//! configuration), for one that cannot command it (a Z3817A, whose
//! `z3817a` is `58503a`'s, the owner's choice, and deaf), over `--model`:
//! `58503a` eight channels, `@@Ea` every second, `@@Bb` on each change,
//! `@@En` every second with Time RAIM on, 1000 ns, the pulse always on,
//! fixing; `z3801a` and `z3805a` the same at six, with `@@Ba` and `@@Bn`.
//! With `--module file:` it exits a second after the recording's last
//! epoch.  `--record-script PATH` writes every command the instrument sends,
//! a line, `SECOND ID DATA`, SECOND the module's epochs before it.

#![expect(clippy::large_stack_arrays, reason = "a host program, its stack megabytes, not the firmware's")]

mod initial_config;
mod record_script;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::File;
use std::io::Error;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use embedded_io_async::ErrorType;
use embedded_io_async::Read;
use embedded_io_async::ReadReady;
use embedded_io_async::Write;
use oncore_app::Event;
use oncore_app::Exit;
use oncore_app::MODULE_DEFAULT_BAUD;
use oncore_app::ModulePort;
use oncore_app::ModuleSetup;
use oncore_app::ONCORE_BAUD;
use oncore_app::Options;
use oncore_app::State;
use oncore_app::run;
use oncore_core::emulator::Configuration;
use oncore_core::gnss::ModuleStreamer;
use oncore_core::identity::BoardIdentity;
use oncore_core::identity::Identities;
use oncore_core::identity::IdentityStore;
use oncore_core::units::Baud;
use record_script::Recorder;
use smartclock_ubx::frame::Event as UbxEvent;
use smartclock_ubx::message::NAV_EOE;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_serial::SerialPortBuilderExt;

/// `--model`'s names: the VP (six channels, eight from the first `@@Ea`),
/// as the board's default build, the six-channel VP alone or the UT Plus,
/// as its `vp-six` and `ut-plus` builds.
const MODELS: [(&str, Identities); 3] =
    [("vp", Identities::VP), ("vp-six", Identities::VP_SIX), ("ut-plus", Identities::UT_PLUS)];
/// What an I/O task reads at once.
const READ_BYTES: usize = 1024;
/// A recording's epochs, one a second.
const EPOCH: Duration = Duration::from_secs(1);

/// What the I/O task is asked to do.
enum Control {
    Write(Vec<u8>),
    Baud(Baud),
}

/// A port served by an I/O task: bytes in on one channel, writes and baud
/// changes out on another.
struct Port {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    pending: VecDeque<u8>,
    control: mpsc::UnboundedSender<Control>,
    /// What reads from it are shown to, for `--record-script`.
    tap: Option<Tap>,
}

/// A port's reads shown to the script recorder, as the module's or the
/// instrument's.
enum Tap {
    Module(Rc<RefCell<Recorder<File>>>),
    Instrument(Rc<RefCell<Recorder<File>>>),
}

impl ErrorType for Port {
    type Error = Error;
}

impl Read for Port {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.pending.is_empty() {
            match self.rx.recv().await {
                Some(bytes) => self.pending.extend(bytes),
                None => return Err(Error::other("port closed")),
            }
        }
        let n = buf.len().min(self.pending.len());
        for b in buf.iter_mut().take(n) {
            *b = self.pending.pop_front().unwrap_or(0);
        }
        match &self.tap {
            Some(Tap::Module(r)) => r.borrow_mut().module_bytes(&buf[..n]),
            Some(Tap::Instrument(r)) => r.borrow_mut().instrument_bytes(&buf[..n]),
            None => {}
        }
        Ok(n)
    }
}

impl ReadReady for Port {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        if self.pending.is_empty()
            && let Ok(bytes) = self.rx.try_recv()
        {
            self.pending.extend(bytes);
        }
        Ok(!self.pending.is_empty())
    }
}

impl Write for Port {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.control.send(Control::Write(buf.to_vec())).map_err(Error::other)?;
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ModulePort for Port {
    async fn set_baud(&mut self, baud: Baud) {
        let _ = self.control.send(Control::Baud(baud));
    }

    /// A module on a serial port or a file has no reset line.
    async fn reset(&mut self) {}
}

/// `stream` served by a task; a baud change flushes it, then goes to
/// `set_baud`.
fn serve<S>(mut stream: S, set_baud: fn(&mut S, Baud)) -> Port
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (tx, rx) = mpsc::unbounded_channel();
    let (control, mut controls) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut buf = [0u8; READ_BYTES];
        loop {
            tokio::select! {
                r = stream.read(&mut buf) => match r {
                    Ok(0) | Err(_) => return,
                    Ok(n) => { let _ = tx.send(buf[..n].to_vec()); }
                },
                c = controls.recv() => match c {
                    Some(Control::Write(bytes)) => { let _ = stream.write_all(&bytes).await; }
                    Some(Control::Baud(baud)) => {
                        let _ = stream.flush().await;
                        set_baud(&mut stream, baud);
                    }
                    None => return,
                },
            }
        }
    });
    Port { rx, pending: VecDeque::new(), control, tap: None }
}

/// A serial device at `baud`.
fn serial(path: &str, baud: Baud) -> Result<Port, Error> {
    let stream = tokio_serial::new(path, baud.get()).open_native_async()?;
    Ok(serve(stream, |s, baud| {
        let _ = tokio_serial::SerialPort::set_baud_rate(s, baud.get());
    }))
}

/// A TCP connection to `address`, raw bytes both ways; a baud change is
/// ignored.
async fn tcp(address: &str) -> Result<Port, Error> {
    let stream = TcpStream::connect(address).await?;
    stream.set_nodelay(true)?;
    Ok(serve(stream, |_, _| {}))
}

/// A recording played an epoch a second: everything up to each NAV-EOE,
/// then a second's wait.  Writes are dropped.  The receiver hears when
/// the last epoch's second is over.
fn recording(path: &str) -> Result<(Port, oneshot::Receiver<()>), Error> {
    let bytes = std::fs::read(path)?;
    let (tx, rx) = mpsc::unbounded_channel();
    let (control, mut controls) = mpsc::unbounded_channel::<Control>();
    let (ended, end) = oneshot::channel();
    tokio::spawn(async move { while controls.recv().await.is_some() {} });
    tokio::spawn(async move {
        let mut epoch = EndOfEpoch::new();
        let mut start = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if epoch.push(b) {
                if tx.send(bytes[start..=i].to_vec()).is_err() {
                    return;
                }
                start = i + 1;
                tokio::time::sleep(EPOCH).await;
            }
        }
        let _ = tx.send(bytes[start..].to_vec());
        tokio::time::sleep(EPOCH).await;
        let _ = ended.send(());
    });
    Ok((Port { rx, pending: VecDeque::new(), control, tap: None }, end))
}

/// Finds whole NAV-EOEs in the module's bytes.
struct EndOfEpoch(ModuleStreamer);

impl EndOfEpoch {
    fn new() -> Self {
        Self(ModuleStreamer::new())
    }

    /// Takes a byte: whether it ends a NAV-EOE with a good checksum.
    fn push(&mut self, b: u8) -> bool {
        matches!(self.0.push(b), Some(Ok(UbxEvent::Frame(f))) if f.id() == NAV_EOE)
    }
}

/// gpsd's WATCH `raw` mode for a device's bytes as they come, binary.
const GPSD_RAW_BINARY: u8 = 2;

/// A gpsd device's raw stream, listened to only.
async fn gpsd(address: &str) -> Result<Port, Error> {
    let mut stream = TcpStream::connect(address).await?;
    let watch = format!("?WATCH={{\"enable\":true,\"raw\":{GPSD_RAW_BINARY}}};\n");
    stream.write_all(watch.as_bytes()).await?;
    let (tx, rx) = mpsc::unbounded_channel();
    let (control, mut controls) = mpsc::unbounded_channel::<Control>();
    tokio::spawn(async move { while controls.recv().await.is_some() {} });
    tokio::spawn(async move {
        let mut buf = [0u8; READ_BYTES];
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        return;
                    }
                }
            }
        }
    });
    Ok(Port { rx, pending: VecDeque::new(), control, tap: None })
}

fn log(e: Event) {
    match e {
        Event::Command(id) => eprintln!("@@{}", String::from_utf8_lossy(&id.bytes())),
        e => eprintln!("{e:?}"),
    }
}

const USAGE: &str = "usage: oncore-host \
--module serial:PATH|file:PATH|gpsd:HOST:PORT --oncore PATH|tcp:HOST:PORT [--model vp|vp-six|ut-plus] [--initial-config 58503a|z3801a|z3805a|z3817a] [--record-script PATH]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (mut module, mut oncore, mut initial_config, mut model) = (None, None, None, MODELS[0].1);
    let mut record = None;
    while let Some(a) = args.next() {
        match (a.as_str(), args.next()) {
            ("--module", Some(m)) => module = Some(m),
            ("--oncore", Some(o)) => oncore = Some(o),
            ("--model", Some(name)) => match MODELS.iter().find(|(n, _)| *n == name) {
                Some(&(_, m)) => model = m,
                None => {
                    eprintln!("{USAGE}");
                    return ExitCode::FAILURE;
                }
            },
            ("--initial-config", Some(arg)) => match initial_config::parse(&arg) {
                Ok(preset) => initial_config = Some(preset),
                Err(e) => {
                    eprintln!("--initial-config {e}");
                    return ExitCode::FAILURE;
                }
            },
            ("--record-script", Some(path)) => match File::create(&path) {
                Ok(f) => record = Some(Rc::new(RefCell::new(Recorder::new(f)))),
                Err(e) => {
                    eprintln!("--record-script {path}: {e}");
                    return ExitCode::FAILURE;
                }
            },
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    let (Some(module), Some(oncore)) = (module, oncore) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let (presets, configuration) = initial_config.unwrap_or((model, Configuration::FACTORY));
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
    runtime.block_on(async move {
        let (port, setup, end) = match module.split_once(':') {
            Some(("serial", path)) => (serial(path, MODULE_DEFAULT_BAUD), ModuleSetup::Configure, None),
            Some(("file", path)) => match recording(path) {
                Ok((port, end)) => (Ok(port), ModuleSetup::AsIs, Some(end)),
                Err(e) => (Err(e), ModuleSetup::AsIs, None),
            },
            Some(("gpsd", address)) => (gpsd(address).await, ModuleSetup::AsIs, None),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            }
        };
        let mut gnss = match port {
            Ok(p) => p,
            Err(e) => {
                eprintln!("module {module}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let instrument = match oncore.split_once(':') {
            Some(("tcp", address)) => tcp(address).await,
            _ => serial(&oncore, ONCORE_BAUD),
        };
        let mut instrument = match instrument {
            Ok(p) => p,
            Err(e) => {
                eprintln!("oncore {oncore}: {e}");
                return ExitCode::FAILURE;
            }
        };
        if let Some(r) = record {
            gnss.tap = Some(Tap::Module(Rc::clone(&r)));
            instrument.tap = Some(Tap::Instrument(r));
        }
        // Built once, to live as long as the runs.
        let board = BoardIdentity::EMULATOR;
        let identities = Box::leak(Box::new(IdentityStore::EMPTY)).fill(&board, presets);
        let runs = async {
            // on the heap: the host's stack would take it, but it is kept off
            // stacks everywhere
            loop {
                // The bench's reset ends a run: start again from power-on.
                // So does its knock, as a host has no bootloader.
                let mut state = Box::new(State::new());
                let options =
                    Options { identities, built_week: board.built_week, module: setup, configuration };
                let _: Exit = run(&mut state, options, &mut gnss, &mut instrument, log).await;
            }
        };
        // A recording ends the run a second after its last epoch.
        match end {
            Some(end) => tokio::select! {
                () = runs => {}
                _ = end => {}
            },
            None => runs.await,
        }
        ExitCode::SUCCESS
    })
}
