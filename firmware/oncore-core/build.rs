//! Turns NOAA's WMM2025 coefficient file into a table for `wmm`: for each
//! degree n 1 to 12 and order m 0 to n, g, h, and their secular
//! variations, nT and nT/year.

use std::fmt::Write;
use std::path::PathBuf;

const COF: &str = "../../third_party/noaa/WMM2025COF/WMM.COF";

fn main() {
    println!("cargo:rerun-if-changed={COF}");
    let text = std::fs::read_to_string(COF).expect("WMM.COF");
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().expect("header").split_whitespace().collect();
    let epoch: f64 = header[0].parse().expect("epoch");
    let mut rows = String::new();
    let mut count = 0;
    for line in lines {
        if line.starts_with("9999") {
            break;
        }
        let f: Vec<f64> = line.split_whitespace().map(|v| v.parse().expect("number")).collect();
        let (n, m) = (f[0] as usize, f[1] as usize);
        // In the table's order, n(n+1)/2 + m, from n = 1.
        assert_eq!(n * (n + 1) / 2 + m, count + 1, "coefficients out of order at {n} {m}");
        writeln!(rows, "    [{:?}, {:?}, {:?}, {:?}],", f[2], f[3], f[4], f[5]).unwrap();
        count += 1;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("wmm.rs");
    let code = format!(
        "/// The model's epoch.\npub(crate) const EPOCH: DecimalYear = DecimalYear::new({epoch:?});\n\
         /// g, h, g', h' for (n, m) at n(n+1)/2 + m - 1.\n\
         pub(crate) static COEFFICIENTS: [[f32; 4]; {count}] = [\n{rows}];\n"
    );
    std::fs::write(out, code).unwrap();
}
