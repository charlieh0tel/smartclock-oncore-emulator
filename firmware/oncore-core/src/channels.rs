//! The Oncore's channels, filled from the satellites the module sees by
//! the Oncore's rules (VP reference pp. 27-34, 37-38, 41-42, 52-53): automatic best
//! geometry by the chosen xDOP with hysteresis, highest in the sky, or
//! manual; Best-4 or all in view; above the mask angle and off the
//! ignore list.
//!
//! The best-geometry search evaluates every combination of up to 12
//! candidates, as the Oncore does, in `f32`.  It runs in steps
//! ([`Search::step`]) so the firmware can serve its UARTs between them.

use smartclock_gps::engine::settings::FixAlgorithm;
use smartclock_gps::engine::settings::SatelliteSelect;
use smartclock_gps::record::DopKind;

use crate::dop;
use crate::dop::Dops;
use crate::dop::Real;
use crate::dop::UNKNOWNS;
use crate::receiver::ReceiverSettings;
use crate::sky::Prn;
use crate::sky::Sky;
use crate::units::DEGREES_PER_CIRCLE;
use crate::units::DEGREES_PER_HALF_CIRCLE;
use crate::units::DEGREES_PER_QUARTER_CIRCLE;
use crate::units::Degrees;
use crate::units::TENTHS_PER_UNIT;
use crate::units::Tenths;
use crate::units::plus_half;
use crate::units::square_root;
use crate::vector::Enu;
use smartclock_gps::engine::settings::CHANNELS_MOST;

/// sin of 0 to 90 degrees, Q15.
const SIN_Q15: [i16; 91] = [
    0, 572, 1144, 1715, 2286, 2856, 3425, 3993, 4560, 5126, 5690, 6252, 6813, 7371, 7927, 8481, 9032, 9580,
    10126, 10668, 11207, 11743, 12275, 12803, 13328, 13848, 14364, 14876, 15383, 15886, 16383, 16876, 17364,
    17846, 18323, 18794, 19260, 19720, 20173, 20621, 21062, 21497, 21925, 22347, 22762, 23170, 23571, 23964,
    24351, 24730, 25101, 25465, 25821, 26169, 26509, 26841, 27165, 27481, 27788, 28087, 28377, 28659, 28932,
    29196, 29451, 29697, 29934, 30162, 30381, 30591, 30791, 30982, 31163, 31335, 31498, 31650, 31794, 31927,
    32051, 32165, 32269, 32364, 32448, 32523, 32587, 32642, 32687, 32722, 32747, 32762, 32767,
];

/// The most candidates the search takes: the Oncore reports at most 12
/// visible (p. 72).
pub(crate) const CANDIDATES: usize = 12;
/// The most combinations `@@Bc` reports (pp. 76-77).
pub(crate) const BEST: usize = 9;
/// Satellites a Best-4 combination holds.
const BEST4_SATELLITES: usize = 4;
/// Gosper's hack's shift of the changed bits (HAKMEM item 175).
const GOSPER_SHIFT: u32 = 2;
/// The DOPs in `@@Aj`'s order, its codes 0 to 4 (VP pp. 33 and 34).
pub(crate) const DOP_KINDS: [DopKind; 5] =
    [DopKind::Geometric, DopKind::Position, DopKind::Horizontal, DopKind::Vertical, DopKind::Time];
pub(crate) const DOP_TYPES: usize = DOP_KINDS.len();
/// A pivot this small or less is a singular geometry.
const SINGULAR_PIVOT: f32 = 1e-6;
/// The most DOP reported (`@@Ba`, `@@Bc`).
pub(crate) const DOP_MAX: Tenths = Tenths::new(999);
/// A quarter, a half and three quarters of a circle, and all of it, in
/// `i32` degrees.
const QUARTER_CIRCLE: Degrees<i32> = Degrees::new(DEGREES_PER_QUARTER_CIRCLE as i32);
const HALF_CIRCLE: Degrees<i32> = Degrees::new(DEGREES_PER_HALF_CIRCLE as i32);
const THREE_QUARTER_CIRCLE: Degrees<i32> = HALF_CIRCLE.plus(QUARTER_CIRCLE);
const CIRCLE: Degrees<i32> = Degrees::new(DEGREES_PER_CIRCLE as i32);
/// `SIN_Q15`'s full scale, sin 90 degrees.
const Q15_ONE: f32 = i16::MAX as f32;

/// The sine, from `SIN_Q15`.
fn sin_deg(d: Degrees<i32>) -> f32 {
    let d = d.rem_euclid(CIRCLE);
    let (q, neg) = if d <= QUARTER_CIRCLE {
        (d, false)
    } else if d <= HALF_CIRCLE {
        (HALF_CIRCLE - d, false)
    } else if d <= THREE_QUARTER_CIRCLE {
        (d - HALF_CIRCLE, true)
    } else {
        (CIRCLE - d, true)
    };
    let s = f32::from(SIN_Q15[q.get() as usize]) / Q15_ONE;
    if neg { -s } else { s }
}

/// The cosine, from `SIN_Q15`.
fn cos_deg(d: Degrees<i32>) -> f32 {
    sin_deg(QUARTER_CIRCLE - d)
}

/// Tenths, rounded, of a DOP from its square, held to `max`.
pub(crate) fn dop_tenths(dop2: f32, max: Tenths) -> Tenths {
    Tenths::new(plus_half(square_root(dop2) * f32::from(TENTHS_PER_UNIT)) as u16).min(max)
}

/// The unit vector to a satellite, east, north, up.
pub(crate) fn line_of_sight(elevation: Degrees<i8>, azimuth: Degrees<i16>) -> Enu<f32> {
    let (e, a) = (elevation.to_i32(), azimuth.to_i32());
    Enu { east: cos_deg(e) * sin_deg(a), north: cos_deg(e) * cos_deg(a), up: sin_deg(e) }
}

/// The squares of GDOP, PDOP, HDOP, VDOP and TDOP, in the order of
/// `@@Aj`'s types.
pub(crate) fn xdops<T: Real>(d: &Dops<T>) -> [T; DOP_TYPES] {
    [d.geometric(), d.position(), d.horizontal(), d.vertical(), d.time()]
}

/// The squares of the DOPs in `@@Aj`'s order ([`xdops`]) for satellites
/// in these directions; `None` for fewer than four or a singular
/// geometry.
pub(crate) fn dops(sats: impl Iterator<Item = Enu<f32>>) -> Option<[f32; DOP_TYPES]> {
    Some(xdops(&Dops::from_inverse(&inverse(sats)?)))
}

/// The inverse normal matrix ([`dop::inverse`]) in `f32`, as the Oncore
/// computes it.
pub(crate) fn inverse(sats: impl Iterator<Item = Enu<f32>>) -> Option<[[f32; UNKNOWNS]; UNKNOWNS]> {
    dop::inverse(sats, SINGULAR_PIVOT)
}

/// The satellites the channels may take: present, located, healthy or
/// of unknown health, at or above the mask angle and not ignored, the
/// highest first, at most [`CANDIDATES`].  VP p. 27 says "greater
/// than", but on whole degrees, as NAV-SAT and the VP give them, a
/// B4121 kept a satellite at the mask on a channel (PRN 11 at 73 with
/// `@@Ag` 73, `docs/vp-on-jig.md`, 2026-11-16).
pub(crate) fn candidates(settings: &ReceiverSettings, sky: &Sky) -> ([u8; CANDIDATES], usize) {
    let mut prns = [0u8; CANDIDATES];
    let mut n = 0;
    let ok = |prn: u8| {
        let s = sky.get(prn)?;
        let fine = s.located()
            && !s.unhealthy()
            && s.elevation.to_i32() >= settings.mask_angle.to_i32()
            && !settings.ignore.contains(Prn::new(prn)?);
        fine.then_some(s.elevation)
    };
    // Insertion by elevation, keeping the highest.
    for (prn, _) in sky.iter() {
        let prn = prn.get();
        let Some(e) = ok(prn) else { continue };
        let mut i = n;
        while i > 0 && ok(prns[i - 1]).unwrap_or(Degrees::new(i8::MIN)) < e {
            if i < CANDIDATES {
                prns[i] = prns[i - 1];
            }
            i -= 1;
        }
        if i < CANDIDATES {
            prns[i] = prn;
            n = (n + 1).min(CANDIDATES);
        }
    }
    (prns, n)
}

/// A combination of candidates, as a bit per candidate, and its xDOP
/// squared.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Combination {
    pub(crate) members: u16,
    pub(crate) dop2: f32,
}

/// The search for the best combinations of `k` of `n` candidates.
#[derive(Debug, Clone)]
pub(crate) struct Search {
    /// The candidates' lines of sight, east, north and up; the first `n`
    /// are used.
    los: [Enu<f32>; CANDIDATES],
    /// The candidates, and the satellites a combination takes.
    n: usize,
    k: usize,
    /// The xDOP minimized, its index in `@@Aj`'s types.
    dop_type: usize,
    /// The next combination, a bit for each candidate taken, `k` of the
    /// low `n`: the bitmask Gosper's hack steps through in order.
    next: u16,
    /// The best combinations so far, the least xDOP first.
    pub(crate) best: [Option<Combination>; BEST],
}

impl Search {
    /// `los` holds the candidates' directions; `dop_kind` is `@@Aj`'s.
    pub(crate) fn new(los: &[Enu<f32>], k: usize, dop_kind: DopKind) -> Self {
        let mut l = [Enu::default(); CANDIDATES];
        let n = los.len().min(CANDIDATES);
        l[..n].copy_from_slice(&los[..n]);
        let k = k.min(n);
        Self { los: l, n, k, dop_type: dop_index(dop_kind), next: (1 << k) - 1, best: [None; BEST] }
    }

    fn done(&self) -> bool {
        self.k == 0 || u32::from(self.next) >= 1 << self.n
    }

    /// Evaluates up to `budget` combinations; returns true when all are.
    pub(crate) fn step(&mut self, budget: usize) -> bool {
        for _ in 0..budget {
            if self.done() {
                return true;
            }
            let x = self.next;
            let members = (0..self.n).filter(|i| x & (1 << i) != 0).map(|i| self.los[i]);
            if let Some(d) = dops(members) {
                self.insert(Combination { members: x, dop2: d[self.dop_type] });
            }
            // The next combination of the same size (Gosper).
            let c = x.isolate_lowest_one();
            let r = x.wrapping_add(c);
            self.next = if r == 0 { u16::MAX } else { (((r ^ x) >> GOSPER_SHIFT) / c) | r };
        }
        self.done()
    }

    fn insert(&mut self, c: Combination) {
        let mut c = Some(c);
        for slot in self.best.iter_mut() {
            match (*slot, c) {
                (None, _) => {
                    *slot = c;
                    return;
                }
                (Some(s), Some(new)) if new.dop2 < s.dop2 => {
                    *slot = Some(new);
                    c = Some(s);
                }
                _ => {}
            }
        }
    }
}

/// The satellites on the channels, and which give position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Assignment {
    /// PRN per channel, 0 for none.
    pub(crate) prns: [u8; CHANNELS_MOST],
    /// Channels whose satellite the position uses (bit per channel):
    /// Best-4's four, or every channel in all-in-view.
    pub(crate) position: u8,
}

impl Default for Assignment {
    fn default() -> Self {
        Self::new()
    }
}

impl Assignment {
    /// No satellite on any channel.
    pub(crate) const fn new() -> Self {
        Self { prns: [0; CHANNELS_MOST], position: 0 }
    }

    /// Puts `chosen` on `channels` channels, each satellite already on a
    /// channel staying there.
    pub(crate) fn place(&self, chosen: &[u8], channels: usize) -> [u8; CHANNELS_MOST] {
        let mut out = [0u8; CHANNELS_MOST];
        for (c, &p) in self.prns.iter().enumerate().take(channels) {
            if p != 0 && chosen.contains(&p) {
                out[c] = p;
            }
        }
        for &p in chosen {
            if !out.contains(&p)
                && let Some(free) = out[..channels].iter_mut().find(|v| **v == 0)
            {
                *free = p;
            }
        }
        out
    }
}

/// Chooses the channels' satellites, a selection at a time: begun with
/// [`Selector::start`] and run with [`Selector::step`].
#[derive(Debug, Clone)]
pub struct Selector {
    pub(crate) assignment: Assignment,
    /// The best combinations of the last automatic selection, as PRNs,
    /// with their xDOP, for `@@Bc`.
    pub(crate) best: [Option<(Tenths, [u8; CHANNELS_MOST])>; BEST],
    /// Satellites per combination: Best-4's four, else the channels.
    pub(crate) per_combination: usize,
    search: Option<Search>,
    prns: [u8; CANDIDATES],
    n: usize,
}

impl Default for Selector {
    fn default() -> Self {
        Self::new()
    }
}

impl Selector {
    /// Nothing chosen, no search.
    pub(crate) const fn new() -> Self {
        Self {
            assignment: Assignment::new(),
            best: [None; BEST],
            per_combination: 0,
            search: None,
            prns: [0; CANDIDATES],
            n: 0,
        }
    }

    /// Whether an automatic selection is under way.
    pub fn busy(&self) -> bool {
        self.search.is_some()
    }

    /// Begins a selection from the module's satellites.  Manual and
    /// highest-in-sky selections finish at once.  Out of line, so its
    /// locals (744 bytes) are off the stack before the caller goes on:
    /// inlined into `Engine::alert`, they stayed under `@@Bb`'s.
    #[inline(never)]
    pub(crate) fn start(&mut self, settings: &ReceiverSettings, sky: &Sky, channels: usize) {
        (self.prns, self.n) = candidates(settings, sky);
        self.search = None;
        let best4 = settings.fix_algorithm == FixAlgorithm::BestFour;
        self.per_combination = if best4 { BEST4_SATELLITES } else { channels };
        match settings.satellite_select {
            SatelliteSelect::Manual => {
                let manual = &settings.manual[..channels];
                self.assignment = Assignment { prns: self.assignment.prns, position: 0 };
                self.assignment.prns[..channels].copy_from_slice(manual);
                self.assignment.position = self.position_mask(channels, best4, None);
            }
            SatelliteSelect::HighestInSky => {
                let chosen = &self.prns[..self.n.min(channels)];
                self.assignment.prns = self.assignment.place(chosen, channels);
                self.assignment.position = self.position_mask(channels, best4, None);
            }
            SatelliteSelect::BestGeometry => {
                let mut los = [Enu::default(); CANDIDATES];
                for (l, &prn) in los.iter_mut().zip(&self.prns[..self.n]) {
                    let s = sky.get(prn).copied().unwrap_or_default();
                    *l = line_of_sight(s.elevation, s.azimuth);
                }
                self.search = Some(Search::new(&los[..self.n], self.per_combination, settings.dop_kind));
            }
        }
    }

    /// Runs up to `budget` combinations of an automatic selection;
    /// returns true when there is none left to run.
    pub fn step(&mut self, settings: &ReceiverSettings, sky: &Sky, channels: usize, budget: usize) -> bool {
        let Some(search) = &mut self.search else { return true };
        if !search.step(budget) {
            return false;
        }
        let found = search.best;
        self.search = None;
        let best4 = settings.fix_algorithm == FixAlgorithm::BestFour;
        let members = |m: u16| {
            let mut p = [0u8; CHANNELS_MOST];
            for (slot, i) in p.iter_mut().zip((0..self.n).filter(|i| m & (1 << i) != 0)) {
                *slot = self.prns[i];
            }
            p
        };
        for (out, c) in self.best.iter_mut().zip(found) {
            *out = c.map(|c| (dop_tenths(c.dop2, DOP_MAX), members(c.members)));
        }
        // Fewer than four candidates, or no usable geometry: track them
        // all, without regard to geometry (p. 37, "three or four").
        let Some(top) = found[0] else {
            let all = self.prns;
            self.assignment.prns = self.assignment.place(&all[..self.n.min(channels)], channels);
            self.assignment.position = self.position_mask(channels, best4, None);
            return true;
        };
        // Keep the satellites giving position unless the best is better
        // by more than the hysteresis (p. 37).
        let current: [u8; CHANNELS_MOST] = {
            let mut p = [0u8; CHANNELS_MOST];
            let mut k = 0;
            for c in 0..channels {
                let prn = self.assignment.prns[c];
                if self.assignment.position & (1 << c) != 0
                    && self.prns[..self.n].contains(&prn)
                    && k < CHANNELS_MOST
                {
                    p[k] = prn;
                    k += 1;
                }
            }
            p
        };
        let k = self.per_combination.min(self.n);
        let held = current.iter().filter(|&&p| p != 0).count() == k;
        let dop_type = dop_index(settings.dop_kind);
        let current_dop = held
            .then(|| {
                dops(
                    current[..k]
                        .iter()
                        .filter_map(|&p| sky.get(p))
                        .map(|s| line_of_sight(s.elevation, s.azimuth)),
                )
            })
            .flatten()
            .map(|d| square_root(d[dop_type]));
        let hysteresis = settings.xdop_hysteresis.value();
        let keep = current_dop.is_some_and(|d| d - square_root(top.dop2) <= hysteresis);
        let primary = if keep { current } else { members(top.members) };
        // Best-4 keeps further satellites in reserve on the other
        // channels, the highest first.
        let mut chosen = [0u8; CHANNELS_MOST];
        let mut n = 0;
        for &p in primary[..k].iter().chain(self.prns[..self.n].iter()) {
            if n < channels && p != 0 && !chosen[..n].contains(&p) {
                chosen[n] = p;
                n += 1;
            }
        }
        self.assignment.prns = self.assignment.place(&chosen[..n], channels);
        self.assignment.position = self.position_mask(channels, best4, Some(&primary[..k]));
        true
    }

    /// The channels whose satellites give position: in all-in-view every
    /// occupied one; in Best-4 the four given, or the first four.
    fn position_mask(&self, channels: usize, best4: bool, primary: Option<&[u8]>) -> u8 {
        let mut mask = 0u8;
        let mut k = 0;
        for c in 0..channels {
            let prn = self.assignment.prns[c];
            let uses = prn != 0 && (!best4 || primary.map_or(k < BEST4_SATELLITES, |p| p.contains(&prn)));
            if uses {
                mask |= 1 << c;
                k += 1;
            }
        }
        mask
    }
}

/// A DOP's place in `@@Aj`'s order ([`DOP_KINDS`], [`xdops`]).
pub(crate) const fn dop_index(kind: DopKind) -> usize {
    match kind {
        DopKind::Geometric => 0,
        DopKind::Position => 1,
        DopKind::Horizontal => 2,
        DopKind::Vertical => 3,
        DopKind::Time => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::Assignment;
    use super::BEST;
    use super::BEST4_SATELLITES;
    use super::CANDIDATES;
    use super::DOP_MAX;
    use super::DOP_TYPES;
    use super::FixAlgorithm;
    use super::GOSPER_SHIFT;
    use super::HALF_CIRCLE;
    use super::QUARTER_CIRCLE;
    use super::SatelliteSelect;
    use super::Search;
    use super::Selector;
    use super::Sky;
    use super::THREE_QUARTER_CIRCLE;
    use super::candidates;
    use super::dop_index;
    use super::dop_tenths;
    use super::dops;
    use super::line_of_sight;
    use super::xdops;
    use crate::dop::Dops;
    use crate::dop::UNKNOWNS;
    use crate::fixtures::SPREAD_DOPS;
    use crate::fixtures::prn;
    use crate::receiver::DEFAULT_MASK_ANGLE;
    use crate::receiver::ReceiverSettings;
    use crate::sky::LOCKED;
    use crate::sky::PrnSet;
    use crate::sky::Satellite;
    use crate::units::Degrees;
    use crate::units::Tenths;
    use crate::units::square_root;
    use crate::vector::Enu;
    use smartclock_gps::engine::settings::CHANNELS_MOST;
    use smartclock_gps::record::DopKind;
    use smartclock_gps::record::Health;

    /// The DOPs squared by a plain f64 inverse, for comparison.
    fn reference(sats: &[(Degrees<i8>, Degrees<i16>)]) -> [f64; DOP_TYPES] {
        let mut m = [[0f64; UNKNOWNS]; UNKNOWNS];
        for &(e, a) in sats {
            let row = [-(e.cos() * a.sin()), -(e.cos() * a.cos()), -e.sin(), 1.0];
            for i in 0..UNKNOWNS {
                for j in 0..UNKNOWNS {
                    m[i][j] += row[i] * row[j];
                }
            }
        }
        // Cofactor inverse diagonal of the normal matrix by brute force:
        // solve for each unit vector with Gaussian elimination.
        let mut q = [0f64; UNKNOWNS];
        for k in 0..UNKNOWNS {
            let mut a = m;
            let mut b = [0f64; UNKNOWNS];
            b[k] = 1.0;
            for c in 0..UNKNOWNS {
                let p = (c..UNKNOWNS).max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs())).unwrap();
                a.swap(c, p);
                b.swap(c, p);
                for r in c + 1..UNKNOWNS {
                    let f = a[r][c] / a[c][c];
                    let pivot = a[c];
                    for (v, p) in a[r].iter_mut().zip(pivot).skip(c) {
                        *v -= f * p;
                    }
                    b[r] -= f * b[c];
                }
            }
            let mut x = [0f64; UNKNOWNS];
            for r in (0..UNKNOWNS).rev() {
                x[r] = (b[r] - (r + 1..UNKNOWNS).map(|j| a[r][j] * x[j]).sum::<f64>()) / a[r][r];
            }
            q[k] = x[k];
        }
        xdops(&Dops { east: q[0], north: q[1], up: q[2], clock: q[3] })
    }

    /// The line of sight to an elevation and azimuth.
    fn los((elevation, azimuth): (Degrees<i8>, Degrees<i16>)) -> Enu<f32> {
        line_of_sight(elevation, azimuth)
    }

    #[test]
    fn dops_match_a_reference() {
        /// Six satellites' elevations and azimuths, one overhead; the f32
        /// DOPs' relative error allowed.
        const SKY: [(Degrees<i8>, Degrees<i16>); 6] = [
            (Degrees::new(90), Degrees::new(0)),
            (Degrees::new(20), Degrees::new(0)),
            (Degrees::new(25), Degrees::new(120)),
            (Degrees::new(30), Degrees::new(240)),
            (Degrees::new(45), Degrees::new(60)),
            (Degrees::new(15), Degrees::new(300)),
        ];
        const TOLERANCE: f64 = 1e-3;
        for n in UNKNOWNS..=SKY.len() {
            let got = dops(SKY[..n].iter().copied().map(los)).unwrap();
            let want = reference(&SKY[..n]);
            for i in 0..DOP_TYPES {
                let (g, w) = (f64::from(got[i]), want[i]);
                assert!((g - w).abs() / w < TOLERANCE, "n {n} dop {i}: {g} vs {w}");
            }
        }
    }

    #[test]
    fn coplanar_is_singular() {
        // Four satellites on the horizon, a quarter circle apart.
        let flat = [Degrees::new(0), QUARTER_CIRCLE, HALF_CIRCLE, THREE_QUARTER_CIRCLE]
            .map(|a| (Degrees::new(0), Degrees::<i16>::try_from(a).unwrap()));
        assert_eq!(dops(flat.into_iter().map(los)), None);
    }

    #[test]
    fn dop_tenths_of_known_squares() {
        /// DOPs squared and their tenths: exact squares, one whose root
        /// rounds up (sqrt 2.25 is 1.5, 15 tenths; sqrt 2 is 1.41421, 14),
        /// and one past the most reported.
        const SQUARES: [(f32, Tenths); 6] = [
            (0.0, Tenths::new(0)),
            (1.0, Tenths::new(10)),
            (2.0, Tenths::new(14)),
            (2.25, Tenths::new(15)),
            (6.25, Tenths::new(25)),
            (1e6, DOP_MAX),
        ];
        for (dop2, tenths) in SQUARES {
            assert_eq!(dop_tenths(dop2, DOP_MAX), tenths, "{dop2}");
        }
    }

    /// A square below 0, as rounding can leave one, has the root 0.
    #[test]
    fn a_negative_square_has_root_zero() {
        const NEGATIVE: f32 = -1e-3;
        assert_eq!(square_root(NEGATIVE), 0.0);
        assert_eq!(dop_tenths(NEGATIVE, DOP_MAX), Tenths::new(0));
    }

    /// A search's step, a hundred combinations.
    const SEARCH_STEP: usize = 100;

    /// Twelve candidates rising from 10 degrees, 6 apart, and round the
    /// sky 30 apart.
    fn rising() -> Vec<Enu<f32>> {
        const LOWEST: Degrees<i8> = Degrees::new(10);
        const RISE: Degrees<i8> = Degrees::new(6);
        const AROUND: Degrees<i16> = Degrees::new(30);
        (0..CANDIDATES).map(|i| los((LOWEST + RISE * i as i8, AROUND * i as i16))).collect()
    }

    /// The combinations of six of [`rising`] by PDOP, 924 of them, in
    /// nine whole steps and a part, each combination of six.
    #[test]
    fn search_visits_every_combination_in_steps() {
        const WHOLE_STEPS: usize = 9;
        let los = rising();
        let mut s = Search::new(&los, CHANNELS, DopKind::Position);
        let mut steps = 0;
        while !s.step(SEARCH_STEP) {
            steps += 1;
        }
        assert_eq!(steps, WHOLE_STEPS);
        let best: Vec<_> = s.best.iter().flatten().collect();
        assert_eq!(best.len(), BEST);
        assert!(best.iter().all(|c| c.members.count_ones() as usize == CHANNELS));
    }

    #[test]
    fn search_keeps_the_best_in_order() {
        let los = rising();
        let mut s = Search::new(&los, CHANNELS, DopKind::Position);
        while !s.step(SEARCH_STEP) {}
        let best: Vec<_> = s.best.iter().flatten().collect();
        assert!(best.is_sorted_by(|a, b| a.dop2 <= b.dop2));
        // No other combination beats the best: each of six, in turn
        // (Gosper's hack).
        let mut x: u16 = (1 << CHANNELS) - 1;
        let mut min = f32::MAX;
        while x < 1 << CANDIDATES {
            let d = dops((0..CANDIDATES).filter(|i| x & (1 << i) != 0).map(|i| los[i])).unwrap()
                [dop_index(DopKind::Position)];
            min = min.min(d);
            let c = x.isolate_lowest_one();
            let r = x + c;
            x = (((r ^ x) >> GOSPER_SHIFT) / c) | r;
        }
        assert_eq!(best[0].dop2, min);
    }

    /// Degrees between `sky`'s satellites, by PRN.
    const AZIMUTH_STEP: Degrees<i16> = Degrees::new(20);

    /// A satellite of [`sky`]: its PRN and elevation, whether the module
    /// holds its ephemeris, and its health.
    #[derive(Debug, Clone, Copy)]
    struct Placed {
        prn: u8,
        elevation: Degrees<i8>,
        ephemeris: bool,
        health: Health,
    }

    impl Placed {
        /// With an ephemeris, of unknown health.
        const fn located(prn: u8, elevation: Degrees<i8>) -> Self {
            Self { prn, elevation, ephemeris: true, health: Health::Unknown }
        }
    }

    /// A sky of satellites, round the sky by PRN.
    fn sky(sats: &[Placed]) -> Sky {
        let mut s = Sky::new();
        for &Placed { prn: number, elevation, ephemeris, health } in sats {
            s.satellites[prn(number).index()] = Satellite {
                present: true,
                elevation,
                azimuth: AZIMUTH_STEP * i16::from(number),
                ephemeris,
                orbit: ephemeris,
                health,
                ..Satellite::default()
            };
        }
        s
    }

    #[test]
    fn candidates_by_elevation_above_mask_not_ignored_or_unhealthy() {
        /// PRN 1 below the mask, 5 unhealthy, 6 unlocated and 7 ignored;
        /// the rest, 4, 2, 3 and 8, at the mask, wanted highest first.
        const BELOW_MASK: Placed = Placed::located(1, Degrees::new(5));
        const AT_MASK: Placed = Placed::located(8, Degrees::new(DEFAULT_MASK_ANGLE.get() as i8));
        const HIGH: Placed = Placed::located(2, Degrees::new(60));
        const LOW: Placed = Placed::located(3, Degrees::new(30));
        const HIGHEST: Placed = Placed::located(4, Degrees::new(80));
        const SICK: Placed = Placed { health: Health::Unhealthy, ..Placed::located(5, Degrees::new(45)) };
        const UNLOCATED: Placed = Placed { ephemeris: false, ..Placed::located(6, Degrees::new(50)) };
        const IGNORED: Placed = Placed::located(7, Degrees::new(70));
        const WANTED: [u8; 4] = [HIGHEST.prn, HIGH.prn, LOW.prn, AT_MASK.prn];
        let s = sky(&[BELOW_MASK, AT_MASK, HIGH, LOW, HIGHEST, SICK, UNLOCATED, IGNORED]);
        let settings =
            ReceiverSettings { ignore: PrnSet::from_iter([prn(IGNORED.prn)]), ..ReceiverSettings::POWER_UP };
        let (prns, n) = candidates(&settings, &s);
        assert_eq!(&prns[..n], WANTED);
    }

    #[test]
    fn candidates_keep_the_twelve_highest() {
        /// Twenty satellites, each 3 degrees above the last from 13.
        const SATELLITES: u8 = 20;
        const LOWEST: Degrees<i8> = Degrees::new(10);
        const RISE: Degrees<i8> = Degrees::new(3);
        let sats: Vec<_> = (1..=SATELLITES).map(|p| Placed::located(p, LOWEST + RISE * p as i8)).collect();
        let (prns, n) = candidates(&ReceiverSettings::POWER_UP, &sky(&sats));
        assert_eq!(n, CANDIDATES);
        let highest: Vec<u8> = (1..=SATELLITES).rev().take(CANDIDATES).collect();
        assert_eq!(prns[..n], highest);
    }

    /// A satellite where the tests' sky has it.
    #[derive(Debug, Clone, Copy)]
    struct Sighting {
        prn: u8,
        elevation: Degrees<i8>,
        azimuth: Degrees<i16>,
    }

    /// A sky of satellites with ephemeris.
    fn spread(sats: &[Sighting]) -> Sky {
        let mut s = Sky::new();
        for sat in sats {
            s.satellites[prn(sat.prn).index()] = Satellite {
                present: true,
                elevation: sat.elevation,
                azimuth: sat.azimuth,
                quality: LOCKED,
                ephemeris: true,
                orbit: true,
                ..Satellite::default()
            };
        }
        s
    }

    fn run(sel: &mut Selector, settings: &ReceiverSettings, sky: &Sky, channels: usize) {
        sel.start(settings, sky, channels);
        while !sel.step(settings, sky, channels, usize::MAX) {}
    }

    /// Six channels; eight satellites spread round the sky, one high.
    const CHANNELS: usize = 6;
    const EIGHT: [Sighting; 8] = [
        Sighting { prn: 1, elevation: Degrees::new(80), azimuth: Degrees::new(0) },
        Sighting { prn: 2, elevation: Degrees::new(15), azimuth: Degrees::new(10) },
        Sighting { prn: 3, elevation: Degrees::new(20), azimuth: Degrees::new(100) },
        Sighting { prn: 4, elevation: Degrees::new(25), azimuth: Degrees::new(200) },
        Sighting { prn: 5, elevation: Degrees::new(30), azimuth: Degrees::new(290) },
        Sighting { prn: 6, elevation: Degrees::new(45), azimuth: Degrees::new(50) },
        Sighting { prn: 7, elevation: Degrees::new(60), azimuth: Degrees::new(160) },
        Sighting { prn: 8, elevation: Degrees::new(12), azimuth: Degrees::new(250) },
    ];

    #[test]
    fn automatic_all_in_view_fills_every_channel() {
        let sky = spread(&EIGHT);
        let mut sel = Selector::default();
        run(&mut sel, &ReceiverSettings::POWER_UP, &sky, CHANNELS);
        let on: Vec<_> = sel.assignment.prns[..CHANNELS].iter().filter(|&&p| p != 0).collect();
        assert_eq!(on.len(), CHANNELS);
        assert_eq!(sel.assignment.position, (1 << CHANNELS) - 1);
        assert_eq!(sel.per_combination, CHANNELS);

        let (dop, prns) = sel.best[0].unwrap();
        assert!(SPREAD_DOPS.contains(&dop));
        assert_eq!(prns.iter().filter(|&&p| p != 0).count(), CHANNELS);
        // The best combination is what the channels hold.
        let mut a = prns[..CHANNELS].to_vec();
        let mut b = sel.assignment.prns[..CHANNELS].to_vec();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    #[test]
    fn best_four_keeps_reserves() {
        let sky = spread(&EIGHT);
        let settings =
            ReceiverSettings { fix_algorithm: FixAlgorithm::BestFour, ..ReceiverSettings::POWER_UP };
        let mut sel = Selector::default();
        run(&mut sel, &settings, &sky, CHANNELS);
        assert_eq!(sel.per_combination, BEST4_SATELLITES);
        assert_eq!(sel.assignment.position.count_ones() as usize, BEST4_SATELLITES);
        assert_eq!(sel.assignment.prns[..CHANNELS].iter().filter(|&&p| p != 0).count(), CHANNELS);
    }

    #[test]
    fn hysteresis_holds_a_good_enough_set() {
        /// A ninth satellite, and a hysteresis no DOP exceeds.
        const NINTH: Sighting = Sighting { prn: 9, elevation: Degrees::new(50), azimuth: Degrees::new(330) };
        let sky = spread(&EIGHT);
        let mut sel = Selector::default();
        run(&mut sel, &ReceiverSettings::POWER_UP, &sky, CHANNELS);
        let first = sel.assignment;
        // Another satellite appears; with a large hysteresis nothing moves.
        let mut more = EIGHT.to_vec();
        more.push(NINTH);
        let sky = spread(&more);
        let settings = ReceiverSettings { xdop_hysteresis: DOP_MAX, ..ReceiverSettings::POWER_UP };
        run(&mut sel, &settings, &sky, CHANNELS);
        assert_eq!(sel.assignment, first);
    }

    #[test]
    fn highest_in_sky_and_manual() {
        /// The six highest of `EIGHT`, by PRN; a manual choice on
        /// channels 1 and 3, which give position.
        const HIGHEST: [u8; CHANNELS] = [1, 3, 4, 5, 6, 7];
        const MANUAL: [u8; CHANNELS_MOST] = [9, 0, 3, 0, 0, 0, 0, 0];
        const MANUAL_POSITION: u8 = 0b101;
        let sky = spread(&EIGHT);
        let mut sel = Selector::default();
        let settings = ReceiverSettings {
            satellite_select: SatelliteSelect::HighestInSky,
            ..ReceiverSettings::POWER_UP
        };
        run(&mut sel, &settings, &sky, CHANNELS);
        let mut on = sel.assignment.prns[..CHANNELS].to_vec();
        on.sort();
        assert_eq!(on, HIGHEST);
        let settings = ReceiverSettings {
            satellite_select: SatelliteSelect::Manual,
            manual: MANUAL,
            ..ReceiverSettings::POWER_UP
        };
        run(&mut sel, &settings, &sky, CHANNELS);
        assert_eq!(sel.assignment.prns, MANUAL);
        assert_eq!(sel.assignment.position, MANUAL_POSITION);
    }

    #[test]
    fn three_satellites_are_all_tracked() {
        /// Fewer than a fix needs.
        const THREE: usize = UNKNOWNS - 1;
        let sky = spread(&EIGHT[..THREE]);
        let mut sel = Selector::default();
        run(&mut sel, &ReceiverSettings::POWER_UP, &sky, CHANNELS);
        let mut on: Vec<u8> = sel.assignment.prns[..CHANNELS].iter().copied().filter(|&p| p != 0).collect();
        on.sort();
        assert_eq!(on, EIGHT[..THREE].iter().map(|s| s.prn).collect::<Vec<_>>());
    }

    #[test]
    fn placing_keeps_satellites_on_their_channels() {
        /// On the channels; chosen; placed, 5 and 12 where they were.
        const ON: [u8; CHANNELS_MOST] = [5, 9, 12, 0, 0, 0, 0, 0];
        const CHOSEN: [u8; 4] = [12, 3, 5, 20];
        const PLACED: [u8; CHANNELS_MOST] = [5, 3, 12, 20, 0, 0, 0, 0];
        let a = Assignment { prns: ON, position: 0 };
        assert_eq!(a.place(&CHOSEN, CHANNELS), PLACED);
    }
}
