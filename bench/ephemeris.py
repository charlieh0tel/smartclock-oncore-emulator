"""GPS satellites from broadcast ephemerides, and the timing error of a wrong held position.

  ephemeris.py hold-error --nav FILE [--nav FILE ...] --true LAT,LON,HEIGHT
      --held LAT,LON,HEIGHT --start UNIX --end UNIX [--step 60] [--mask 5]
      [--per-satellite]

FILE is RINEX 3 navigation data (`BRDC00WRD_R_2026313*_01D_MN.rnx` and the like, uncompressed);
only its GPS records are read.  Positions are WGS 84 geodetic: degrees, degrees, ellipsoidal
height in meters.  Times are UTC unix seconds.

A timing receiver holding a position solves only for its clock: from each satellite it takes
the pseudorange less the range to the held position as its clock's offset.  Held farther from a
satellite than it is, it takes its clock as behind by the difference and puts its pulse early.
So each satellite's error is (range from the true position - range from the held one) / c,
positive for a late pulse, and an all-in-view solution's is about their mean: one CSV row per
time with the satellites above the mask (elevations from the true position), the mean, and the
mean weighted by sin^2(elevation).  `--per-satellite` writes a row per satellite instead.
Satellite positions follow IS-GPS-200 (20.3.3.4.3), at the signal's transmission, in the
frame at its reception.
"""

import argparse
import csv
import itertools
import math
import sys
from collections.abc import Iterable, Iterator
from dataclasses import dataclass, fields
from pathlib import Path

Vector = tuple[float, float, float]
"""ECEF meters."""

SPEED_OF_LIGHT_M_PER_S = 299_792_458.0
EARTH_MU_M3_PER_S2 = 3.986005e14
"""IS-GPS-200's WGS 84 value."""
EARTH_ROTATION_RAD_PER_S = 7.2921151467e-5
WGS84_SEMI_MAJOR_M = 6_378_137.0
WGS84_FLATTENING = 1 / 298.257223563
WGS84_ECCENTRICITY_SQUARED = WGS84_FLATTENING * (2 - WGS84_FLATTENING)
GPS_EPOCH_UNIX_S = 315_964_800
"""1980-01-06 00:00:00 UTC."""
GPS_LESS_UTC_S = 18
"""Leap seconds since the GPS epoch, from 2017-01-01."""
SECONDS_PER_WEEK = 604_800
SECONDS_PER_HOUR = 3600
NS_PER_S = 1e9
KEPLER_ITERATIONS = 10
"""Fixed-point iterations of Kepler's equation; GPS eccentricities are below 0.03."""
LIGHT_TIME_ITERATIONS = 3
DEFAULT_FIT_INTERVAL_H = 4.0
"""RINEX's fit interval of 0: IS-GPS-200's 4 hours."""
HEALTHY = 0
DEFAULT_MASK_DEG = 5.0
DEFAULT_STEP_S = 60

RINEX_FIELD_WIDTH = 19
RINEX_FIRST_LINE_START = 23
"""The epoch line's first value: after the satellite and its time of clock."""
RINEX_CONTINUATION_START = 4
RINEX_FIELDS_PER_LINE = 4
RINEX_FIRST_LINE_FIELDS = RINEX_FIELDS_PER_LINE - 1
"""The epoch line holds the time of clock, then three values."""
RINEX_END_OF_HEADER = "END OF HEADER"
GPS_SYSTEM = "G"


@dataclass(frozen=True)
class Ephemeris:
    """One GPS broadcast ephemeris, its fields in RINEX 3's order (IS-GPS-200 names and units:
    seconds, meters, radians and radians per second)."""

    prn: int
    clock_bias: float
    clock_drift: float
    clock_drift_rate: float
    iode: float
    crs: float
    delta_n: float
    m0: float
    cuc: float
    eccentricity: float
    cus: float
    sqrt_a: float
    toe: float
    """Seconds of the GPS week."""
    cic: float
    omega0: float
    cis: float
    i0: float
    crc: float
    omega: float
    omega_dot: float
    idot: float
    l2_codes: float
    week: float
    """GPS week of toe, continuous (not mod 1024)."""
    l2p_flag: float
    accuracy: float
    health: float
    tgd: float
    iodc: float
    transmission_time: float
    fit_interval: float
    """Hours; 0 for unknown."""

    @classmethod
    def read(cls, prn: int, values: Iterator[float]) -> "Ephemeris":
        """The ephemeris from its values, in order after the PRN."""
        return cls(prn, *(next(values) for _ in fields(cls)[1:]))

    def toe_gps_s(self) -> float:
        """toe as seconds since the GPS epoch."""
        return self.week * SECONDS_PER_WEEK + self.toe

    def fit_half_s(self) -> float:
        """Half the fit interval, either side of toe."""
        hours = self.fit_interval or DEFAULT_FIT_INTERVAL_H
        return hours * SECONDS_PER_HOUR / 2

    def position(self, gps_s: float) -> Vector:
        """The satellite's ECEF position at a GPS time, seconds since the GPS epoch, in the
        frame at that time (IS-GPS-200, Table 20-IV)."""
        tk = gps_s - self.toe_gps_s()
        a = self.sqrt_a**2
        n = math.sqrt(EARTH_MU_M3_PER_S2 / a**3) + self.delta_n
        mean_anomaly = self.m0 + n * tk
        e = self.eccentricity
        eccentric_anomaly = mean_anomaly
        for _ in range(KEPLER_ITERATIONS):
            eccentric_anomaly = mean_anomaly + e * math.sin(eccentric_anomaly)
        true_anomaly = math.atan2(
            math.sqrt(1 - e * e) * math.sin(eccentric_anomaly), math.cos(eccentric_anomaly) - e
        )
        latitude = true_anomaly + self.omega
        sin2, cos2 = math.sin(2 * latitude), math.cos(2 * latitude)
        u = latitude + self.cus * sin2 + self.cuc * cos2
        r = a * (1 - e * math.cos(eccentric_anomaly)) + self.crs * sin2 + self.crc * cos2
        i = self.i0 + self.idot * tk + self.cis * sin2 + self.cic * cos2
        x, y = r * math.cos(u), r * math.sin(u)
        node = (
            self.omega0
            + (self.omega_dot - EARTH_ROTATION_RAD_PER_S) * tk
            - EARTH_ROTATION_RAD_PER_S * self.toe
        )
        return (
            x * math.cos(node) - y * math.cos(i) * math.sin(node),
            x * math.sin(node) + y * math.cos(i) * math.cos(node),
            y * math.sin(i),
        )


def rinex_value(text: str) -> float:
    """A RINEX number, Fortran's D exponent allowed; a blank (a missing trailing field) is 0."""
    text = text.strip().replace("D", "E").replace("d", "e")
    return float(text) if text else 0.0


def line_values(line: str, start: int, count: int) -> Iterator[float]:
    for k in range(count):
        offset = start + k * RINEX_FIELD_WIDTH
        yield rinex_value(line[offset : offset + RINEX_FIELD_WIDTH])


def read_nav(lines: Iterable[str]) -> list[Ephemeris]:
    """Every GPS ephemeris in RINEX 3 navigation data.  A record starts with its system letter
    in the first column; its continuation lines start with spaces."""
    lines = iter(lines)
    for line in lines:
        if RINEX_END_OF_HEADER in line:
            break
    records: list[list[str]] = []
    for line in lines:
        if line[:1].isalpha():
            records.append([line])
        elif records and line.strip():
            records[-1].append(line)
    ephemerides = []
    for first, *rest in records:
        if first[:1] != GPS_SYSTEM:
            continue
        values = itertools.chain(
            line_values(first, RINEX_FIRST_LINE_START, RINEX_FIRST_LINE_FIELDS),
            *(line_values(line, RINEX_CONTINUATION_START, RINEX_FIELDS_PER_LINE) for line in rest),
        )
        ephemerides.append(Ephemeris.read(int(first[1:3]), values))
    return ephemerides


def select(ephemerides: Iterable[Ephemeris], gps_s: float) -> dict[int, Ephemeris]:
    """Each satellite's healthy ephemeris with toe nearest a GPS time, within its fit."""
    chosen: dict[int, Ephemeris] = {}
    for eph in ephemerides:
        age = abs(gps_s - eph.toe_gps_s())
        if eph.health != HEALTHY or age > eph.fit_half_s():
            continue
        best = chosen.get(eph.prn)
        if best is None or age < abs(gps_s - best.toe_gps_s()):
            chosen[eph.prn] = eph
    return chosen


def unix_to_gps(unix_s: float) -> float:
    """UTC unix seconds as GPS seconds since the GPS epoch."""
    return unix_s - GPS_EPOCH_UNIX_S + GPS_LESS_UTC_S


@dataclass(frozen=True)
class Site:
    """A WGS 84 geodetic position."""

    latitude_deg: float
    longitude_deg: float
    height_m: float
    """Above the ellipsoid."""

    @classmethod
    def parse(cls, text: str) -> "Site":
        """From `LAT,LON,HEIGHT`."""
        latitude, longitude, height = (float(v) for v in text.split(","))
        return cls(latitude, longitude, height)

    def ecef(self) -> Vector:
        lat, lon = math.radians(self.latitude_deg), math.radians(self.longitude_deg)
        n = WGS84_SEMI_MAJOR_M / math.sqrt(1 - WGS84_ECCENTRICITY_SQUARED * math.sin(lat) ** 2)
        horizontal = (n + self.height_m) * math.cos(lat)
        return (
            horizontal * math.cos(lon),
            horizontal * math.sin(lon),
            (n * (1 - WGS84_ECCENTRICITY_SQUARED) + self.height_m) * math.sin(lat),
        )

    def look(self, target: Vector) -> tuple[float, float]:
        """Elevation and azimuth (from north, through east) of an ECEF point, degrees."""
        lat, lon = math.radians(self.latitude_deg), math.radians(self.longitude_deg)
        dx, dy, dz = (t - s for t, s in zip(target, self.ecef(), strict=True))
        east = -math.sin(lon) * dx + math.cos(lon) * dy
        north = (
            -math.sin(lat) * math.cos(lon) * dx
            - math.sin(lat) * math.sin(lon) * dy
            + math.cos(lat) * dz
        )
        up = (
            math.cos(lat) * math.cos(lon) * dx
            + math.cos(lat) * math.sin(lon) * dy
            + math.sin(lat) * dz
        )
        elevation = math.degrees(math.atan2(up, math.hypot(east, north)))
        azimuth = math.degrees(math.atan2(east, north)) % 360
        return elevation, azimuth


def rotate_earth(position: Vector, seconds: float) -> Vector:
    """A position in the ECEF frame of `seconds` later: the Earth turned under it."""
    angle = EARTH_ROTATION_RAD_PER_S * seconds
    x, y, z = position
    return (
        math.cos(angle) * x + math.sin(angle) * y,
        -math.sin(angle) * x + math.cos(angle) * y,
        z,
    )


def received_position(eph: Ephemeris, gps_s: float, receiver: Vector) -> Vector:
    """Where the satellite was when it sent the signal received at a GPS time, in the frame at
    reception."""
    flight_s = 0.0
    position = eph.position(gps_s)
    for _ in range(LIGHT_TIME_ITERATIONS):
        flight_s = math.dist(position, receiver) / SPEED_OF_LIGHT_M_PER_S
        position = rotate_earth(eph.position(gps_s - flight_s), flight_s)
    return position


@dataclass(frozen=True)
class SatelliteError:
    """One satellite's timing error from a held position."""

    prn: int
    elevation_deg: float
    azimuth_deg: float
    late_ns: float
    """Positive for a late pulse."""


def hold_late_ns(satellite: Vector, true: Vector, held: Vector) -> float:
    """How late a receiver's pulse is from one satellite, held at `held` while at `true`, ns."""
    return (
        (math.dist(satellite, true) - math.dist(satellite, held))
        / SPEED_OF_LIGHT_M_PER_S
        * NS_PER_S
    )


def satellite_errors(
    ephemerides: Iterable[Ephemeris], gps_s: float, true: Site, held: Site, mask_deg: float
) -> list[SatelliteError]:
    """Each satellite above the mask at the true position, and its timing error held at the
    other: (range from true - range from held) / c."""
    true_ecef, held_ecef = true.ecef(), held.ecef()
    errors = []
    for prn, eph in sorted(select(ephemerides, gps_s).items()):
        position = received_position(eph, gps_s, true_ecef)
        elevation, azimuth = true.look(position)
        if elevation < mask_deg:
            continue
        errors.append(
            SatelliteError(prn, elevation, azimuth, hold_late_ns(position, true_ecef, held_ecef))
        )
    return errors


@dataclass(frozen=True)
class SolutionError:
    """An all-in-view solution's timing error from a held position, positive for late."""

    satellites: int
    late_ns: float
    """Equal weights."""
    weighted_late_ns: float
    """Weighted by sin^2(elevation)."""


def solution_error(errors: list[SatelliteError]) -> SolutionError | None:
    """The mean of the satellites' errors, plain and weighted; None with no satellites."""
    if not errors:
        return None
    weights = [math.sin(math.radians(e.elevation_deg)) ** 2 for e in errors]
    weighted = sum(w * e.late_ns for w, e in zip(weights, errors, strict=True)) / sum(weights)
    return SolutionError(len(errors), sum(e.late_ns for e in errors) / len(errors), weighted)


def hold_error(args: argparse.Namespace) -> int:
    ephemerides: list[Ephemeris] = []
    for path in args.nav:
        with path.open() as nav:
            ephemerides += read_nav(nav)
    out = csv.writer(sys.stdout, lineterminator="\n")
    if args.per_satellite:
        out.writerow(["unix_s", "prn", "elevation_deg", "azimuth_deg", "late_ns"])
    else:
        out.writerow(["unix_s", "satellites", "late_ns", "weighted_late_ns"])
    missing = 0
    for unix_s in range(args.start, args.end + 1, args.step):
        errors = satellite_errors(ephemerides, unix_to_gps(unix_s), args.true, args.held, args.mask)
        if args.per_satellite:
            for e in errors:
                out.writerow(
                    [
                        unix_s,
                        e.prn,
                        f"{e.elevation_deg:.2f}",
                        f"{e.azimuth_deg:.2f}",
                        f"{e.late_ns:.3f}",
                    ]
                )
        elif (solution := solution_error(errors)) is not None:
            out.writerow(
                [
                    unix_s,
                    solution.satellites,
                    f"{solution.late_ns:.3f}",
                    f"{solution.weighted_late_ns:.3f}",
                ]
            )
        if not errors:
            missing += 1
    if missing:
        print(f"{missing} times without a satellite above the mask", file=sys.stderr)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawTextHelpFormatter
    )
    commands = parser.add_subparsers(required=True)
    p = commands.add_parser("hold-error")
    p.add_argument("--nav", type=Path, action="append", required=True)
    p.add_argument("--true", type=Site.parse, required=True, help="LAT,LON,HEIGHT")
    p.add_argument("--held", type=Site.parse, required=True, help="LAT,LON,HEIGHT")
    p.add_argument("--start", type=int, required=True, help="UTC unix seconds")
    p.add_argument("--end", type=int, required=True, help="UTC unix seconds, inclusive")
    p.add_argument("--step", type=int, default=DEFAULT_STEP_S, help="seconds")
    p.add_argument("--mask", type=float, default=DEFAULT_MASK_DEG, help="elevation, degrees")
    p.add_argument("--per-satellite", action="store_true")
    p.set_defaults(run=hold_error)
    args = parser.parse_args()
    return int(args.run(args))


if __name__ == "__main__":
    sys.exit(main())
