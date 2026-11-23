//! The translation between u-blox UBX and Motorola Oncore binary, without
//! the hardware: `no_std`, no allocation, tested on the host.

#![cfg_attr(not(test), no_std)]

mod channels;
pub mod civil;
pub mod control;
pub mod differential;
pub mod dop;
pub mod emulator;
#[cfg(test)]
mod fixtures;
pub mod gnss;
pub mod identity;
pub mod module;
pub mod nav;
pub mod receiver;
pub mod record;
pub mod sky;
pub mod solution;
pub mod traim;
pub mod units;
pub mod vector;
pub mod wire;
mod wmm;
