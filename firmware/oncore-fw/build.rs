//! Gives the firmware its build date, YYYYMMDD in UTC, for the manufacture
//! date `@@Cj` reports; `SOURCE_DATE_EPOCH` sets it for a reproducible
//! build.  Gives it its build identity, the tag, the commit (`git
//! describe --always --dirty`), the features and the date, for a flash
//! dump to name (`tools/board_serial.py`).  Links defmt's script when the `rtt` feature is on, puts the
//! stack's top at the STM32C092's SRAM end with `nucleo`, and cuts the
//! trace's first epochs for `examples/timing.rs`.

use std::path::Path;
use std::process::Command;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use civil::UnixSeconds;
use civil::civil_from_days;
use smartclock_ubx::frame::Deframer;
use smartclock_ubx::message::NAV_EOE;

/// The engine's calendar arithmetic, from its source rather than through
/// a build dependency on `oncore-core`.
#[path = "../oncore-core/src/civil.rs"]
#[expect(dead_code, reason = "the build takes only the date of a day count")]
#[expect(unreachable_pub, reason = "civil.rs's public items, for oncore-host, are private here")]
mod civil;

fn main() {
    let seconds = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64));
    let (year, month, day) = civil_from_days(UnixSeconds::new(seconds).day());
    let date = format!("{year:04}{month:02}{day:02}");
    println!("cargo:rustc-env=ONCORE_BUILD_DATE={date}");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    build_identity(&date);
    // defmt's linker script, only with RTT logging.
    if std::env::var_os("CARGO_FEATURE_RTT").is_some() {
        println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
        println!("cargo:rustc-link-arg-examples=-Tdefmt.x");
    }
    // embassy-stm32's memory.x is the C091's, 36 KB; the NUCLEO-C092RC's
    // C092 has 30 KB (DS14720 3.4), so the stack starts at its end.
    if std::env::var_os("CARGO_FEATURE_NUCLEO").is_some() {
        println!("cargo:rustc-link-arg=--defsym=_stack_start={C092_SRAM_END:#x}");
    }
    timing_trace();
}

/// What starts the build identity, as `tools/board_serial.py` finds it.
const BUILD_TAG: &str = "ONCORE-BUILD";
/// The firmware's features that change what it is, as Cargo names them.
const FEATURES: [&str; 5] = ["rtt", "nucleo", "ut-plus", "vp-six", "z3817a"];
/// What the sources this firmware is built from are, for `build_identity`
/// to run again when any of them changes.
const SOURCES: [&str; 6] =
    ["src", "Cargo.toml", "Cargo.lock", "../oncore-core", "../oncore-app", "../Cargo.lock"];

/// `ONCORE_BUILD_ID`: the tag, the commit, the features (`vp` for none)
/// and `date`, spaces between.
fn build_identity(date: &str) {
    let commit =
        git(&["describe", "--always", "--dirty", "--abbrev=12", "--exclude=*"]).unwrap_or_else(|| {
            println!("cargo:warning=no git commit for the build identity: unknown");
            "unknown".to_owned()
        });
    let features: Vec<&str> = FEATURES
        .into_iter()
        .filter(|f| {
            std::env::var_os(format!("CARGO_FEATURE_{}", f.to_uppercase().replace('-', "_"))).is_some()
        })
        .collect();
    let features = if features.is_empty() { "vp".to_owned() } else { features.join(",") };
    println!("cargo:rustc-env=ONCORE_BUILD_ID={BUILD_TAG} {commit} {features} {date}");
    for source in SOURCES {
        println!("cargo:rerun-if-changed={source}");
    }
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        for file in ["HEAD", "index"] {
            println!("cargo:rerun-if-changed={}", Path::new(&dir).join(file).display());
        }
    }
    if let (Some(common), Some(head)) =
        (git(&["rev-parse", "--git-common-dir"]), git(&["symbolic-ref", "-q", "HEAD"]))
    {
        println!("cargo:rerun-if-changed={}", Path::new(&common).join(head).display());
        println!("cargo:rerun-if-changed={}", Path::new(&common).join("packed-refs").display());
    }
}

/// `git`'s answer to `args`, trimmed, or `None` if it fails.
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The end of the STM32C092's 30 KB of SRAM, from `0x2000_0000`.
const C092_SRAM_END: u32 = SRAM_START + C092_SRAM_KB * BYTES_PER_KB;
const SRAM_START: u32 = 0x2000_0000;
const C092_SRAM_KB: u32 = 30;
const BYTES_PER_KB: u32 = 1024;

/// The trace's first `TIMING_EPOCHS` epochs, each ending in a NAV-EOE,
/// for `examples/timing.rs`: two minutes, about 120 KB of flash.
fn timing_trace() {
    const TRACE: &str = "../testdata/neo-m8t-2026-10-28.ubx";
    const TIMING_EPOCHS: usize = 120;
    /// The longest payload a UBX frame's length can give, so that the
    /// deframer frames every message of the trace.
    const LONGEST_PAYLOAD: usize = u16::MAX as usize;
    println!("cargo:rerun-if-changed={TRACE}");
    let trace = std::fs::read(TRACE).expect("the trace");
    let mut deframer = Box::new(Deframer::<LONGEST_PAYLOAD>::new());
    let (mut at, mut epochs) = (0, 0);
    while at < trace.len() && epochs < TIMING_EPOCHS {
        let (taken, ended) = deframer.feed(&trace[at..]);
        at += taken;
        match ended {
            Some(Ok(frame)) if frame.id() == NAV_EOE => epochs += 1,
            Some(Err(e)) => panic!("the trace, at byte {at}: {e}"),
            _ => {}
        }
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("timing.ubx"), &trace[..at]).expect("timing.ubx");
}
