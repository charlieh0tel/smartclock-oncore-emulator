//! The STM32C091 side: the UARTs and pins for `oncore_app::run`, which
//! sets the module up and serves the instrument.  Events go out over RTT
//! with the `rtt` feature.
//! STATUS flashes on each of the module's 1 PPS pulses.

#![no_std]
#![no_main]

mod bootloader;

use core::hint::black_box;
use core::ops::Range;

#[cfg(feature = "rtt")]
use defmt::info;
#[cfg(feature = "rtt")]
use defmt::warn;
#[cfg(feature = "rtt")]
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_futures::select::Either;
use embassy_futures::select::select;
use embassy_stm32::bind_interrupts;
use embassy_stm32::exti;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::Level;
use embassy_stm32::gpio::Output;
use embassy_stm32::gpio::OutputOpenDrain;
use embassy_stm32::gpio::Pull;
use embassy_stm32::gpio::Speed;
use embassy_stm32::interrupt;
use embassy_stm32::mode::Async;
use embassy_stm32::pac;
use embassy_stm32::peripherals;
use embassy_stm32::rcc::Hsi;
use embassy_stm32::rcc::HsiKerDiv;
use embassy_stm32::rcc::HsiSysDiv;
use embassy_stm32::usart;
use embassy_stm32::usart::BufferedUart;
use embassy_stm32::usart::Config;
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_time::Duration;
use embassy_time::Timer;
use embedded_io_async::ErrorType;
use embedded_io_async::Read;
use embedded_io_async::ReadReady;
use embedded_io_async::Write;
use oncore_app::Event;
use oncore_app::Exit;
use oncore_app::Host;
use oncore_app::MODULE_DEFAULT_BAUD;
use oncore_app::ModulePort;
use oncore_app::ModuleSetup;
use oncore_app::ONCORE_BAUD;
use oncore_app::Options;
use oncore_app::State;
use oncore_core::emulator::Configuration;
use oncore_core::identity::BoardIdentity;
use oncore_core::identity::Identities;
use oncore_core::identity::IdentityStore;
use oncore_core::units::Baud;
#[cfg(not(feature = "rtt"))]
use panic_halt as _;
#[cfg(feature = "rtt")]
use panic_probe as _;
use static_cell::ConstStaticCell;

/// A `'static` buffer of `$size` bytes, zeroed at compile time: never on
/// the stack, as `StaticCell::init`'s argument would be.
macro_rules! buffer {
    ($size:expr) => {{
        static BUFFER: ConstStaticCell<[u8; $size]> = ConstStaticCell::new([0; $size]);
        BUFFER.take()
    }};
}

bind_interrupts!(
    /// The interrupts the firmware takes: the two UARTs' buffered
    /// handlers, and the 1 PPS input's EXTI line.
    struct Irqs {
        USART1 => usart::BufferedInterruptHandler<peripherals::USART1>;
        USART2 => usart::BufferedInterruptHandler<peripherals::USART2>;
        EXTI0_1 => exti::InterruptHandler<interrupt::typelevel::EXTI0_1>;
    }
);

/// How long `RESET_N` is held low at start: u-blox gives no minimum; the
/// pin's 11 kOhm pull-up is the module's own (UBX-15025193 R10, 4.2).
const GNSS_RESET_PULSE: Duration = Duration::from_millis(10);

/// The module's link: 115200 baud carries about 11.5 KB/s, and each second
/// brings a burst of 1 to 2 KB, so its receive buffer holds a second's
/// worth while a long answer goes out to the instrument at 9600.
const GNSS_RX: usize = 2048;
const GNSS_TX: usize = 256;
/// The instrument's link: an answer as long as `@@Cj`, 294 bytes, and
/// the continuous outputs of a second, written without waiting.
const ONC_RX: usize = 256;
const ONC_TX: usize = 1024;

/// How often a write that found too little room in the transmit buffer
/// is tried again.  embassy-stm32 0.6.0's `BufferedUartTx::write` then
/// returns `Pending` without registering a waker (`usart/buffered.rs`),
/// so the write would wait for some other wake of the task, and with
/// both ports quiet for none.  2 ms is two characters at 9600.
const TX_RETRY: Duration = Duration::from_millis(2);

/// The watchdog's time-out: well over the longest pass of the loop, a
/// second's wait and a full transmit buffer's drain at 9600 (about 1.1 s).
const WATCHDOG_TIMEOUT_US: u32 = 8_000_000;

/// A buffered UART with a `TX`-byte transmit buffer.  Embassy refuses a
/// write longer than that whole (`BufferTooLong`), so each write is cut
/// to it and `write_all` sends the rest.
struct Port<const TX: usize>(BufferedUart<'static>);

/// The module's UART and its `RESET_N`, open drain only: u-blox says never
/// to drive `RESET_N` high.
struct Gnss {
    port: Port<GNSS_TX>,
    reset: OutputOpenDrain<'static>,
}
/// The instrument's UART.
type Instrument = Port<ONC_TX>;

impl ErrorType for Gnss {
    type Error = usart::Error;
}

impl Read for Gnss {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.port.read(buf).await
    }
}

impl ReadReady for Gnss {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        self.port.read_ready()
    }
}

impl Write for Gnss {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.port.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.port.flush().await
    }
}

impl<const TX: usize> ErrorType for Port<TX> {
    type Error = usart::Error;
}

impl<const TX: usize> Read for Port<TX> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await
    }
}

impl<const TX: usize> ReadReady for Port<TX> {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        self.0.read_ready()
    }
}

impl<const TX: usize> Write for Port<TX> {
    /// A write that does not fit yet is dropped and made again each
    /// [`TX_RETRY`]: until it fits it changes nothing.
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let chunk = &buf[..buf.len().min(TX)];
        loop {
            if let Either::First(written) = select(self.0.write(chunk), Timer::after(TX_RETRY)).await {
                return written;
            }
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().await
    }
}

impl ModulePort for Gnss {
    async fn set_baud(&mut self, baud: Baud) {
        let _ = self.port.flush().await;
        if self.port.0.set_baudrate(baud.get()).is_err() {
            #[cfg(feature = "rtt")]
            warn!("module baud {} refused", baud.get());
        }
    }

    /// `RESET_N` low a moment: the reset clears the module's RTC time but
    /// not BBR (UBX-15030060 R07, 1.5.1).
    async fn reset(&mut self) {
        self.reset.set_low();
        Timer::after(GNSS_RESET_PULSE).await;
        self.reset.set_high();
    }
}

/// The board for `oncore_app::run`: the log, and the watchdog fed each
/// pass of its loop.
struct Board {
    watchdog: IndependentWatchdog<'static, peripherals::IWDG>,
}

impl Host for Board {
    fn log(&mut self, event: Event) {
        log(event);
    }

    fn alive(&mut self) {
        self.watchdog.pet();
    }
}

#[cfg(not(feature = "rtt"))]
fn log(_: Event) {}

#[cfg(feature = "rtt")]
fn log(e: Event) {
    match e {
        Event::ModuleFound(baud) => info!("module at {} baud", baud.get()),
        Event::ModuleMoved => info!("module moved from {} baud", MODULE_DEFAULT_BAUD.get()),
        Event::SetupNak(step) => warn!("module refused set-up step {}", step),
        Event::SetupTimeout(step) => warn!("no answer to set-up step {}", step),
        Event::SetupDone => info!("module set up"),
        Event::SetupFailed => warn!("module set-up failed: not saved, to run again"),
        Event::Command(id) => info!("@@{=[u8]:a}", id.bytes()),
        Event::BadFrame(id) => warn!("@@{=[u8]:a}: checksum", id.bytes()),
        Event::BadId(b) => warn!("@@ then {=u8:#04x}: not an ID", b),
        Event::UnknownCommand(id) => warn!("@@{=[u8]:a}: not taken", id.bytes()),
        Event::Silent => warn!("no epoch from the module"),
        Event::ModuleRequestDropped => warn!("module request dropped: no room"),
        Event::ModuleRequestFailed => warn!("module request refused or unanswered: settings sent again"),
        Event::AnswerDropped => warn!("answer to the instrument dropped: no room"),
        Event::Reset => info!("reset asked for"),
        Event::Bootloader => info!("bootloader asked for"),
        Event::ModuleReset => warn!("module silent: reset"),
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    bootloader::at_start();
    // Kept in the image for a flash dump to name; nothing reads it here.
    black_box(&BUILD_ID);
    #[cfg(feature = "rtt")]
    info!("{=str}", BUILD_IDENTITY);
    // SYSCLK 48 MHz, the HSI undivided: embassy's default divides it by 4.
    let mut rcc = embassy_stm32::Config::default();
    rcc.rcc.hsi = Some(Hsi { sys_div: HsiSysDiv::DIV1, ker_div: HsiKerDiv::DIV3 });
    let p = embassy_stm32::init(rcc);

    usart1_on_instrument_pins();
    watchdog_stops_for_the_debugger();
    // After the bootloader's check: the ROM bootloader is never fed.
    let mut watchdog = IndependentWatchdog::new(p.IWDG, WATCHDOG_TIMEOUT_US);
    watchdog.unleash();

    let gnss_reset = OutputOpenDrain::new(p.PA4, Level::High, Speed::Low);

    // Each UART at its peer's rate out of reset, 8N1.
    let (mut gnss_config, mut onc_config) = (Config::default(), Config::default());
    gnss_config.baudrate = MODULE_DEFAULT_BAUD.get();
    onc_config.baudrate = ONCORE_BAUD.get();

    let gnss =
        BufferedUart::new(p.USART2, p.PA3, p.PA2, buffer!(GNSS_TX), buffer!(GNSS_RX), Irqs, gnss_config)
            .unwrap();
    let onc = BufferedUart::new(p.USART1, p.PA10, p.PA9, buffer!(ONC_TX), buffer!(ONC_RX), Irqs, onc_config)
        .unwrap();

    let pps = ExtiInput::new(p.PA0, p.EXTI0, Pull::None, Irqs);
    let led = Output::new(p.PA5, Level::Low, Speed::Low);

    spawner.spawn(pps_led(pps, led).unwrap());
    spawner.spawn(
        emulator(Gnss { port: Port(gnss), reset: gnss_reset }, Port(onc), Board { watchdog }).unwrap(),
    );
}

/// The watchdog stopped while a debugger halts the core, so flashing and
/// stepping do not reset the board (RM0490, `DBG_APB_FZ1`).
fn watchdog_stops_for_the_debugger() {
    pac::RCC.apbenr1().modify(|w| w.set_dbgen(true));
    pac::DBGMCU.apb1fzr().modify(|w| w.set_iwdg(true));
}

/// USART1's PA9 and PA10 on TSSOP20 pins 16 and 17, the instrument's,
/// which are PA11 and PA12 unless remapped (RM0490, `SYSCFG_CFGR1`); with
/// SYSCFG's clock on.  Not on the NUCLEO-C092RC, whose LQFP64 has PA9
/// and PA10 pins of their own, which the remap would turn analog, and
/// whose PA11 and PA12 go to its USB-C connector.
fn usart1_on_instrument_pins() {
    pac::RCC.apbenr2().modify(|w| w.set_syscfgen(true));
    #[cfg(not(feature = "nucleo"))]
    pac::SYSCFG.cfgr1().modify(|w| {
        w.set_pa11_rmp(true);
        w.set_pa12_rmp(true);
    });
}

/// The Oncore the board presents, its personality (PLAN.md): the VP, six
/// channels and eight from the first `@@Ea`; with the `vp-six`
/// feature the six-channel VP alone; with `z3817a` the eight-channel VP
/// from power-up; with `ut-plus` the UT Plus.
const PRESETS: Identities = if cfg!(feature = "ut-plus") {
    Identities::UT_PLUS
} else if cfg!(feature = "z3817a") {
    Identities::Z3817A
} else if cfg!(feature = "vp-six") {
    Identities::VP_SIX
} else {
    Identities::VP
};
/// How the engine powers up: with the `z3817a` feature as a 58503A
/// leaves its engine, and deaf, for a Z3817A, which cannot command its
/// receiver; otherwise as from the factory.
const CONFIGURATION: Configuration =
    if cfg!(feature = "z3817a") { Configuration::Z3817A } else { Configuration::FACTORY };

/// The emulator's state, built at compile time: too large for the stack.
static STATE: ConstStaticCell<State> = ConstStaticCell::new(State::new());
/// The identities the engine answers as, with the board's serial number
/// and manufacture date.
static IDENTITIES: ConstStaticCell<IdentityStore> = ConstStaticCell::new(IdentityStore::EMPTY);

#[embassy_executor::task]
async fn emulator(mut gnss: Gnss, mut onc: Instrument, board: Board) {
    let identity = BoardIdentity::new(&embassy_stm32::uid::uid(), BUILD_YEAR, BUILD_MONTH, BUILD_DAY);
    let identities = IDENTITIES.take().fill(&identity, PRESETS);
    let options = Options {
        identities,
        built_week: identity.built_week,
        module: ModuleSetup::Configure,
        configuration: CONFIGURATION,
    };
    match oncore_app::run(STATE.take(), options, &mut gnss, &mut onc, board).await {
        Exit::Reset => cortex_m::peripheral::SCB::sys_reset(),
        Exit::Bootloader => bootloader::request(),
    }
}

/// The build identity (`build.rs`): its tag, commit, features and date,
/// and the same NUL-padded in flash, where `tools/board_serial.py` finds
/// it over the debug port.
const BUILD_IDENTITY: &str = env!("ONCORE_BUILD_ID");
const BUILD_ID_LEN: usize = 96;
const _: () = assert!(BUILD_IDENTITY.len() < BUILD_ID_LEN, "the build identity is too long");
static BUILD_ID: [u8; BUILD_ID_LEN] = padded(BUILD_IDENTITY);

/// `s`, NUL-padded to `BUILD_ID_LEN` bytes.
const fn padded(s: &str) -> [u8; BUILD_ID_LEN] {
    let mut out = [0; BUILD_ID_LEN];
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        out[i] = b[i];
        i += 1;
    }
    out
}

/// The build's date, the manufacture date `@@Cj` reports (`build.rs`),
/// YYYYMMDD: where each field's digits are, and their base.
const BUILD_DATE: &str = env!("ONCORE_BUILD_DATE");
const BUILD_YEAR_DIGITS: Range<usize> = 0..4;
const BUILD_MONTH_DIGITS: Range<usize> = 4..6;
const BUILD_DAY_DIGITS: Range<usize> = 6..8;
const DECIMAL: u32 = 10;
const BUILD_YEAR: u16 = parse(BUILD_DATE, BUILD_YEAR_DIGITS) as u16;
const BUILD_MONTH: u8 = parse(BUILD_DATE, BUILD_MONTH_DIGITS) as u8;
const BUILD_DAY: u8 = parse(BUILD_DATE, BUILD_DAY_DIGITS) as u8;

/// The number the decimal digits `digits` of `s` write.
const fn parse(s: &str, digits: Range<usize>) -> u32 {
    let b = s.as_bytes();
    let mut v = 0;
    let mut i = digits.start;
    while i < digits.end {
        v = v * DECIMAL + (b[i] - b'0') as u32;
        i += 1;
    }
    v
}

/// How long STATUS lights on each 1 PPS rising edge: long enough to see, short against the
/// second, so a missing pulse leaves it dark.
const PPS_FLASH: Duration = Duration::from_millis(100);

#[embassy_executor::task]
async fn pps_led(mut pps: ExtiInput<'static, Async>, mut led: Output<'static>) {
    loop {
        pps.wait_for_rising_edge().await;
        led.set_high();
        Timer::after(PPS_FLASH).await;
        led.set_low();
    }
}
