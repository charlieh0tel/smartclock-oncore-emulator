//! How long the emulator's work takes on the STM32C0 at 48 MHz: the first
//! two minutes of the NEO-M8T trace, played from flash through the
//! emulator as `oncore_app::run` drives it, each piece timed.  The longest of each
//! goes out over RTT once the ephemerides are in.
//!
//! ```text
//! cargo run --release --example timing
//! ```
//!
//! On a NUCLEO-C092RC, whose STM32C092 has 30 KB of SRAM to the C091's
//! 36 KB (DS14720 3.4), `nucleo` starts the stack at its end (`build.rs`):
//!
//! ```text
//! cargo build --release --example timing -F nucleo
//! probe-rs run --chip STM32C092RCTx target/thumbv6m-none-eabi/release/examples/timing
//! ```

#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::rcc::Hsi;
use embassy_stm32::rcc::HsiKerDiv;
use embassy_stm32::rcc::HsiSysDiv;
use embassy_time::Duration;
use embassy_time::Instant;
use oncore_core::emulator::Configuration;
use oncore_core::emulator::Emulator;
use oncore_core::emulator::SELECT_EPOCHS;
use oncore_core::gnss::Gnss;
use oncore_core::identity::Identities;
use oncore_core::module::Module;
use panic_probe as _;
use smartclock_gps::engine::Output;
use smartclock_gps::engine::ReceiverControl;
use smartclock_gps::oncore;
use smartclock_gps::oncore::BA_POSITION_STATUS;
use smartclock_gps::oncore::BB_VISIBLE_SATELLITES;
use smartclock_gps::oncore::BH_CORRECTION_OUTPUT;
use smartclock_gps::oncore::BN_TIME_RAIM;
use smartclock_gps::oncore::CG_FIX_MODE;
use smartclock_gps::oncore::MAX_COMMAND;
use smartclock_gps::oncore::MessageId;
use smartclock_gps::units::Milliseconds;
use smartclock_ubx::message::NAV_EOE;
use static_cell::ConstStaticCell;

/// The trace's first epochs, cut by `build.rs`.
const TRACE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/timing.ubx"));
/// Epochs before the ephemerides are in and every satellite is corrected
/// (the trace test's).
const WARM_UP: u32 = 60;

/// Commands' data, as the trace test sends them: fixing (`@@Cg` 1, VP p.
/// 118), once a second (`@@Ba`, `@@Bh`), on each change (`@@Bb`), and
/// `@@Bn` as the Z380x start-up sends it: each second, Time RAIM on, a 1
/// us alarm limit, the pulse on, one pulse a second (pp. 102-106).
const FIXING: [u8; 1] = [1];
const EACH_SECOND: [u8; 1] = [1];
const ON_CHANGE: [u8; 1] = [1];
const Z380X_TIME_RAIM: [u8; 15] = [1, 1, 0, 10, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
/// An epoch's length, as the engine's clock reads it.
const EPOCH_MS: u64 = 1000;

/// Drops what the emulator sends.
struct Discard;

impl Output for Discard {
    fn send(&mut self, _: &[u8]) {}
}

/// Sends `id` with `data` to the emulator.
fn command(emulator: &mut Emulator, id: MessageId, data: &[u8]) {
    let mut frame = [0u8; MAX_COMMAND];
    if let Ok(n) = oncore::encode(id, data, &mut frame) {
        emulator.command(&frame[..n], Milliseconds(0), &mut Discard, &mut |_| {});
    }
}

/// The longest and the total of one kind of work, from the epochs after
/// the warm-up.
#[derive(Default)]
struct Times {
    longest: Duration,
    total: Duration,
}

impl Times {
    fn add(&mut self, d: Duration, warm: bool) {
        if warm {
            self.longest = self.longest.max(d);
            self.total += d;
        }
    }

    fn time<T>(&mut self, warm: bool, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let r = f();
        self.add(start.elapsed(), warm);
        r
    }

    /// Logs `label`, then the longest and the mean of `n`, microseconds.
    fn report(&self, label: &str, n: u32) {
        info!("{=str} {} {}", label, self.longest.as_micros(), (self.total / n).as_micros());
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.hsi = Some(Hsi { sys_div: HsiSysDiv::DIV1, ker_div: HsiKerDiv::DIV3 });
    let _p = embassy_stm32::init(config);
    info!("started, {} bytes of trace", TRACE.len());

    // built at compile time: too large for the stack
    static EMULATOR: ConstStaticCell<Emulator> = ConstStaticCell::new(Emulator::new(
        Identities::VP,
        Configuration::FACTORY,
        ReceiverControl::Commanded,
    ));
    static GNSS: ConstStaticCell<Gnss> = ConstStaticCell::new(Gnss::new());
    let (emulator, gnss, mut module) = (EMULATOR.take(), GNSS.take(), Module::new());
    // The Z380x's continuous outputs, and corrections each second.
    command(emulator, CG_FIX_MODE, &FIXING);
    command(emulator, BA_POSITION_STATUS, &EACH_SECOND);
    command(emulator, BN_TIME_RAIM, &Z380X_TIME_RAIM);
    command(emulator, BH_CORRECTION_OUTPUT, &EACH_SECOND);
    command(emulator, BB_VISIBLE_SATELLITES, &ON_CHANGE);

    let (mut module_epoch, mut second, mut alert, mut select, mut correct) =
        (Times::default(), Times::default(), Times::default(), Times::default(), Times::default());
    let mut epochs = 0u32;
    let mut start = Instant::now();
    for &b in TRACE {
        if gnss.push(b, emulator, &mut module) != Some(NAV_EOE) {
            continue;
        }
        epochs += 1;
        let warm = epochs > WARM_UP;
        // The epoch's bytes, framed and applied, since the last.
        module_epoch.add(start.elapsed(), warm);
        let now = Milliseconds(u64::from(epochs) * EPOCH_MS);
        second.time(warm, || emulator.second(now, &mut Discard, &mut |_| {}));
        if epochs.is_multiple_of(SELECT_EPOCHS) {
            alert.time(warm, || emulator.alert());
        }
        while emulator.receiver().selector.busy() {
            select.time(warm, || emulator.background());
        }
        while emulator.receiver().differential.busy() {
            correct.time(warm, || emulator.background());
        }
        start = Instant::now();
    }
    let n = epochs.saturating_sub(WARM_UP).max(1);
    info!("{} epochs, {} timed; microseconds, longest and mean an epoch:", epochs, n);
    module_epoch.report("module messages", n);
    second.report("second         ", n);
    alert.report("alert          ", n);
    select.report("selection step ", n);
    correct.report("correction step", n);
    let corrected =
        emulator.receiver().differential.latest().map_or(0, |s| s.corrections.iter().flatten().count());
    info!("satellites in the last correction set: {}", corrected);
    loop {
        cortex_m::asm::wfi();
    }
}
