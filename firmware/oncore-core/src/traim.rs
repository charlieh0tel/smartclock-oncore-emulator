//! Time RAIM (VP reference pp. 102-106): checks the time solution of the
//! satellites the module used for consistency, detects a satellite whose
//! time disagrees by more than the alarm limit, and isolates it when the
//! satellites suffice and the receiver holds its position.
//!
//! Each satellite's measurement is its pseudorange residual from the
//! module's position-hold solution (NAV-SAT), in ns: its estimate of the
//! receiver's time offset.  The module cannot be told to drop one, so an
//! isolated satellite is removed from this solution and its report, not
//! from the module's, and the engine reports it an alarm (`Engine`'s
//! `evaluate_traim`).

use crate::sky::PRNS;
use crate::units::Nanoseconds;
use crate::units::SquareNanoseconds;
use crate::units::square_root;

/// The most satellites: every GPS satellite the module may use.
pub(crate) const MAX: usize = PRNS;

/// The VP reference's table (p. 103): from each alarm limit, ns, to the
/// next above it, the satellites needed to detect a fault and to isolate
/// it.  Its ranges meet at their ends; a limit on one belongs to the
/// range above it.  Below the last, 250 ns, the false alarm rate cannot
/// be met.
const NEEDED: [(Nanoseconds, usize, usize); 5] = [
    (Nanoseconds::new(1000), 2, 3),
    (Nanoseconds::new(700), 3, 3),
    (Nanoseconds::new(500), 4, 4),
    (Nanoseconds::new(300), 4, 8),
    (Nanoseconds::new(250), 8, 8),
];

/// Satellites needed to detect and to isolate a fault for an alarm
/// limit; `None` where the false alarm rate cannot be met.
pub(crate) fn needed(alarm: Nanoseconds) -> Option<(usize, usize)> {
    NEEDED.iter().find(|&&(from, ..)| alarm >= from).map(|&(_, detect, isolate)| (detect, isolate))
}

/// The solution's status (p. 105).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Solution {
    /// Within the alarm limit.
    Ok = 0,
    /// The alarm limit is exceeded.
    Alarm = 1,
    /// Limit too low, Time RAIM off, or too few satellites.
    Unknown = 2,
}

/// What the algorithm can do with the satellites it has (p. 106).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Capability {
    DetectAndIsolate = 0,
    DetectOnly = 1,
    Neither = 2,
}

/// One evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Result {
    pub solution: Solution,
    pub(crate) capability: Capability,
    /// The time solution's one-sigma estimate from the residuals' spread:
    /// their standard deviation over the square root of their count.
    pub(crate) sigma: Nanoseconds<f32>,
    /// The time solution: the receiver's offset.
    pub(crate) offset: Nanoseconds<f32>,
    /// Measurements isolated and removed, a bit each.
    pub(crate) removed: u32,
}

/// The fewest measurements a spread is taken from.
const SPREAD_MIN: usize = 2;

/// The mean of the measurements not in `skip`, a bit each, and their
/// count.
fn mean_without(m: &[Nanoseconds<f32>], skip: u32) -> (Nanoseconds<f32>, usize) {
    let (mut sum, mut n) = (Nanoseconds::new(0.0), 0);
    for (i, v) in m.iter().enumerate() {
        if skip & (1 << i) == 0 {
            sum += *v;
            n += 1;
        }
    }
    (if n == 0 { Nanoseconds::new(0.0) } else { sum / n as f32 }, n)
}

/// Time RAIM on or off (`@@Bn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raim {
    Off,
    On,
}

/// Whether the receiver holds its position, which isolating a fault
/// needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hold {
    Navigating,
    Position,
}

/// Evaluates the measurements, at most [`MAX`], against an alarm limit.
pub(crate) fn evaluate(m: &[Nanoseconds<f32>], raim: Raim, alarm: Nanoseconds, hold: Hold) -> Result {
    let m = &m[..m.len().min(MAX)];
    let mut removed = 0u32;
    let unknown = |capability, removed| {
        let (offset, n) = mean_without(m, removed);
        Result {
            solution: Solution::Unknown,
            capability,
            sigma: sigma(m, removed, offset, n),
            offset,
            removed,
        }
    };
    let Some((detect, isolate)) = needed(alarm) else {
        return unknown(Capability::Neither, 0);
    };
    let alarm = alarm.to_f32();
    loop {
        let (_, n) = mean_without(m, removed);
        let capability = if n >= isolate && hold == Hold::Position {
            Capability::DetectAndIsolate
        } else if n >= detect {
            Capability::DetectOnly
        } else {
            Capability::Neither
        };
        if raim == Raim::Off || capability == Capability::Neither {
            return unknown(capability, removed);
        }
        // The satellite that disagrees most with the solution of the
        // others.
        let worst = (0..m.len())
            .filter(|i| removed & (1 << i) == 0)
            .map(|i| (i, (m[i] - mean_without(m, removed | (1 << i)).0).abs()))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let (offset, n) = mean_without(m, removed);
        let sigma = sigma(m, removed, offset, n);
        match worst {
            Some((i, d)) if d > alarm => {
                if capability != Capability::DetectAndIsolate {
                    return Result { solution: Solution::Alarm, capability, sigma, offset, removed };
                }
                removed |= 1 << i;
            }
            _ => return Result { solution: Solution::Ok, capability, sigma, offset, removed },
        }
    }
}

/// The one-sigma spread of the `n` measurements not in `removed` about
/// their `mean`, over the square root of `n`.
fn sigma(m: &[Nanoseconds<f32>], removed: u32, mean: Nanoseconds<f32>, n: usize) -> Nanoseconds<f32> {
    if n < SPREAD_MIN {
        return Nanoseconds::new(0.0);
    }
    let ss: SquareNanoseconds = m
        .iter()
        .enumerate()
        .filter(|(i, _)| removed & (1 << i) == 0)
        .map(|(_, v)| (*v - mean).squared())
        .sum();
    (ss / (n - 1) as f32).sqrt() / square_root(n as f32)
}

#[cfg(test)]
mod tests {
    use super::Capability;
    use super::Hold;
    use super::Nanoseconds;
    use super::Raim;
    use super::Solution;
    use super::evaluate;
    use super::needed;

    /// Six satellites' times, ns, about 12.3; the same with the third
    /// 2.5 us off, and the others' mean; a mean's tolerance, and a sigma
    /// such a spread stays under.
    const CONSISTENT: [f32; 6] = [12.0, 15.0, 9.0, 14.0, 11.0, 13.0];
    const CONSISTENT_MEAN: Nanoseconds<f32> = Nanoseconds::new(12.333);
    const ONE_OFF: [f32; 6] = [12.0, 15.0, 2500.0, 14.0, 11.0, 13.0];
    const OFF: usize = 2;
    const OTHERS_MEAN: Nanoseconds<f32> = Nanoseconds::new(13.0);
    const MEAN_TOLERANCE: Nanoseconds<f32> = Nanoseconds::new(0.01);
    const SIGMA_MAX: Nanoseconds<f32> = Nanoseconds::new(2.0);
    /// Alarm limits: 1000 ns; 400 ns, where isolation needs eight; 200
    /// ns, under the table.
    const LIMIT: Nanoseconds = Nanoseconds::new(1000);
    const EIGHT_TO_ISOLATE: Nanoseconds = Nanoseconds::new(400);
    const TOO_LOW: Nanoseconds = Nanoseconds::new(200);

    /// Measurements, ns.
    fn ns<const N: usize>(m: [f32; N]) -> [Nanoseconds<f32>; N] {
        m.map(Nanoseconds::new)
    }

    #[test]
    fn the_references_table() {
        /// Alarm limits, ns, in each of the reference's ranges and on an
        /// end, and what each needs, written from p. 103, not from
        /// `NEEDED`; one under the lowest range.
        const LIMITS: [(Nanoseconds, (usize, usize)); 6] = [
            (Nanoseconds::new(1200), (2, 3)),
            (Nanoseconds::new(1000), (2, 3)),
            (Nanoseconds::new(800), (3, 3)),
            (Nanoseconds::new(600), (4, 4)),
            (Nanoseconds::new(400), (4, 8)),
            (Nanoseconds::new(260), (8, 8)),
        ];
        const UNDER_THE_TABLE: Nanoseconds = Nanoseconds::new(249);
        for (limit, wanted) in LIMITS {
            assert_eq!(needed(limit), Some(wanted), "{limit:?}");
        }
        assert_eq!(needed(UNDER_THE_TABLE), None);
    }

    #[test]
    fn consistent_satellites_are_ok() {
        let m = ns(CONSISTENT);
        let r = evaluate(&m, Raim::On, LIMIT, Hold::Position);
        assert_eq!((r.solution, r.capability, r.removed), (Solution::Ok, Capability::DetectAndIsolate, 0));
        assert!((r.offset - CONSISTENT_MEAN).abs() < MEAN_TOLERANCE);
        assert!(r.sigma > Nanoseconds::new(0.0) && r.sigma < SIGMA_MAX);
    }

    #[test]
    fn a_bad_satellite_is_isolated_in_hold() {
        let m = ns(ONE_OFF);
        let r = evaluate(&m, Raim::On, LIMIT, Hold::Position);
        assert_eq!((r.solution, r.removed), (Solution::Ok, 1 << OFF));
        assert!((r.offset - OTHERS_MEAN).abs() < MEAN_TOLERANCE);
    }

    #[test]
    fn a_bad_satellite_alarms_when_it_cannot_be_isolated() {
        let m = ns(ONE_OFF);
        // Out of position hold: detection only.
        let r = evaluate(&m, Raim::On, LIMIT, Hold::Navigating);
        assert_eq!((r.solution, r.capability), (Solution::Alarm, Capability::DetectOnly));
        // At 400 ns isolation needs eight satellites.
        let r = evaluate(&m, Raim::On, EIGHT_TO_ISOLATE, Hold::Position);
        assert_eq!((r.solution, r.capability), (Solution::Alarm, Capability::DetectOnly));
    }

    #[test]
    fn unknown_when_off_too_low_or_too_few() {
        let m = ns(CONSISTENT);
        assert_eq!(evaluate(&m, Raim::Off, LIMIT, Hold::Position).solution, Solution::Unknown);
        assert_eq!(evaluate(&m, Raim::On, TOO_LOW, Hold::Position).capability, Capability::Neither);
        let r = evaluate(&m[..1], Raim::On, LIMIT, Hold::Position);
        assert_eq!((r.solution, r.capability), (Solution::Unknown, Capability::Neither));
    }
}
