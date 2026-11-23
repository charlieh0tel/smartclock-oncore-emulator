//! The ROM serial bootloader, entered at the bench's knock (`@@Zb`), so
//! the board flashes over the instrument's port (J1 or J5) with
//! `stm32flash`, without SWD.  The request is a word in RAM that start-up
//! leaves alone, so it outlasts a reset; the next start, before anything
//! is set up, holds the module in reset and jumps to the bootloader in
//! system memory.  The bootloader listens on USART1 and on USART2, the
//! module's link, and takes the first to send it 0x7F (AN2606,
//! STM32C091xx/92xx), hence the module held, though only until the
//! bootloader takes its pin for SPI.  A reset or power cycle
//! leaves it: the factory `NBOOT_SEL` boots main flash (AN5673).

#![expect(
    unsafe_code,
    reason = "a word that outlasts a reset, and the jump to the ROM bootloader (owner's OK)"
)]

use core::arch::asm;
use core::mem::MaybeUninit;
use core::ptr;

use embassy_stm32::pac;
use embassy_stm32::pac::gpio::vals::Moder;
use embassy_stm32::pac::gpio::vals::Odr;
use embassy_stm32::pac::gpio::vals::Ot;
use embassy_stm32::pac::syscfg::vals::MemMode;

/// The request word's value when the bootloader is asked for; any other,
/// as RAM after power-up almost always holds, is none.
const REQUEST: u32 = 0xb007_10ad;
/// System memory, the bootloader's vector table (RM0490, 2.2).
const SYSTEM_MEMORY: u32 = 0x1fff_0000;
/// The module's `RESET_N`, PA4.
const MODULE_RESET_PIN: usize = 4;

/// The request, in `.uninit`, which start-up neither zeroes nor loads.
#[unsafe(link_section = ".uninit.BOOTLOADER_REQUEST")]
static mut BOOTLOADER_REQUEST: MaybeUninit<u32> = MaybeUninit::uninit();

/// Asks for the bootloader at the next start, and resets.
pub(crate) fn request() -> ! {
    // SAFETY: an aligned word of this firmware's RAM, nothing else using
    // it while it runs.
    unsafe { ptr::write_volatile((&raw mut BOOTLOADER_REQUEST).cast::<u32>(), REQUEST) };
    cortex_m::peripheral::SCB::sys_reset()
}

/// At start, before anything is set up: the bootloader, if asked for;
/// else a reset, if the bootloader's Go (AN3155) started the firmware,
/// so it starts from reset state, not the bootloader's.
pub(crate) fn at_start() {
    if take_request() {
        enter();
    }
    if after_bootloader() {
        cortex_m::peripheral::SCB::sys_reset();
    }
}

/// Whether the bootloader ran since reset: system memory still mapped at
/// 0, as the jump to it leaves it, or USART1's clock on, as the
/// bootloader leaves it and reset does not.
fn after_bootloader() -> bool {
    pac::RCC.apbenr2().modify(|w| w.set_syscfgen(true));
    pac::SYSCFG.cfgr1().read().mem_mode() == MemMode::SYSTEM_FLASH || pac::RCC.apbenr2().read().usart1en()
}

/// Whether the bootloader was asked for; clears the request, so a reset
/// out of the bootloader starts the firmware.
fn take_request() -> bool {
    let word = (&raw mut BOOTLOADER_REQUEST).cast::<u32>();
    let value: u32;
    // SAFETY: an aligned word of this firmware's RAM.  Read by `ldr`, as
    // Rust may not read memory nothing initialized; whatever it holds is
    // a valid `u32`.
    unsafe {
        asm!("ldr {value}, [{word}]", value = out(reg) value, word = in(reg) word, options(nostack, readonly, preserves_flags));
        ptr::write_volatile(word, 0);
    }
    value == REQUEST
}

/// The module held in reset, system memory at 0, and the bootloader
/// started.  SYSCFG's pin remaps stay at reset: the bootloader sets
/// them itself to put USART1 on pins 16 and 17, and with them set
/// beforehand it left PA9 (TX) off pin 16 (read over SWD, 2026-11-15).
/// The bootloader takes PA4 for its SPI port, so the module is held
/// only until it starts.
fn enter() -> ! {
    pac::RCC.gpioenr().modify(|w| w.set_gpioaen(true));
    pac::GPIOA.odr().modify(|w| w.set_odr(MODULE_RESET_PIN, Odr::LOW));
    pac::GPIOA.otyper().modify(|w| w.set_ot(MODULE_RESET_PIN, Ot::OPEN_DRAIN));
    pac::GPIOA.moder().modify(|w| w.set_moder(MODULE_RESET_PIN, Moder::OUTPUT));
    pac::RCC.apbenr2().modify(|w| w.set_syscfgen(true));
    pac::SYSCFG.cfgr1().modify(|w| w.set_mem_mode(MemMode::SYSTEM_FLASH));
    // SAFETY: system memory holds the ROM bootloader's vector table, and
    // nothing runs or is enabled that it does not find at reset but the
    // pins above.
    unsafe { cortex_m::asm::bootload(SYSTEM_MEMORY as *const u32) }
}
