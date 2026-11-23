"""Tests for broadcast-ephemeris satellite positions and a held position's timing error."""

import itertools
import math
from dataclasses import fields, replace

import pytest

import ephemeris
from ephemeris import Ephemeris, SatelliteError, Site

GPS_ORBIT_RADIUS_M = 26_560_000.0
TOE_S = 7200.0
WEEK = 2444.0
TOLERANCE_M = 1e-3
TOLERANCE_NS = 1e-6
TOLERANCE_DEG = 1e-9
OFFSET_M = 10.0
LATITUDE_DEG = 37.0
LONGITUDE_DEG = -122.0
EQUATOR = Site(0.0, 0.0, 0.0)
POLE = Site(90.0, 0.0, 0.0)
MID_LATITUDE = Site(LATITUDE_DEG, LONGITUDE_DEG, 0.0)
POLAR_RADIUS_M = ephemeris.WGS84_SEMI_MAJOR_M * (1 - ephemeris.WGS84_FLATTENING)
QUARTER_TURN_DEG = 90.0
HALF_TURN_DEG = 180.0
ELEVATION_DEG = 30.0


def circular(prn: int = 1, toe: float = TOE_S, health: float = 0.0) -> Ephemeris:
    """An equatorial circular orbit at the prime meridian at toe, every other term zero."""
    zeros = Ephemeris.read(prn, itertools.repeat(0.0))
    return replace(
        zeros,
        sqrt_a=math.sqrt(GPS_ORBIT_RADIUS_M),
        toe=toe,
        week=WEEK,
        omega0=ephemeris.EARTH_ROTATION_RAD_PER_S * toe,
        health=health,
    )


def test_site_ecef_on_the_equator_and_at_the_pole() -> None:
    assert EQUATOR.ecef() == pytest.approx((ephemeris.WGS84_SEMI_MAJOR_M, 0.0, 0.0))
    x, y, z = POLE.ecef()
    assert math.hypot(x, y) == pytest.approx(0.0, abs=TOLERANCE_M)
    assert z == pytest.approx(POLAR_RADIUS_M)


def test_site_parses_lat_lon_height() -> None:
    assert Site.parse(f"{LATITUDE_DEG},{LONGITUDE_DEG},{OFFSET_M}") == Site(
        LATITUDE_DEG, LONGITUDE_DEG, OFFSET_M
    )


def test_look_overhead_north_and_east() -> None:
    overhead = replace(MID_LATITUDE, height_m=GPS_ORBIT_RADIUS_M).ecef()
    assert MID_LATITUDE.look(overhead)[0] == pytest.approx(QUARTER_TURN_DEG)
    x, y, z = EQUATOR.ecef()
    elevation, azimuth = EQUATOR.look((x, y, z + OFFSET_M))
    assert (elevation, azimuth) == pytest.approx((0.0, 0.0), abs=TOLERANCE_DEG)
    elevation, azimuth = EQUATOR.look((x, y + OFFSET_M, z))
    assert (elevation, azimuth) == pytest.approx((0.0, QUARTER_TURN_DEG), abs=TOLERANCE_DEG)
    elevation, _ = EQUATOR.look((x - OFFSET_M, y, z))
    assert elevation == pytest.approx(-QUARTER_TURN_DEG)


def test_circular_orbit_position() -> None:
    eph = circular()
    assert eph.position(eph.toe_gps_s()) == pytest.approx((GPS_ORBIT_RADIUS_M, 0.0, 0.0))
    motion = math.sqrt(ephemeris.EARTH_MU_M3_PER_S2 / GPS_ORBIT_RADIUS_M**3)
    elapsed_s = ephemeris.SECONDS_PER_HOUR
    x, y, z = eph.position(eph.toe_gps_s() + elapsed_s)
    assert math.hypot(x, y) == pytest.approx(GPS_ORBIT_RADIUS_M)
    assert z == pytest.approx(0.0, abs=TOLERANCE_M)
    assert math.atan2(y, x) == pytest.approx(
        (motion - ephemeris.EARTH_ROTATION_RAD_PER_S) * elapsed_s
    )


def test_rotate_earth_turns_the_frame_east() -> None:
    quarter_s = math.radians(QUARTER_TURN_DEG) / ephemeris.EARTH_ROTATION_RAD_PER_S
    x, y, z = ephemeris.rotate_earth((GPS_ORBIT_RADIUS_M, 0.0, OFFSET_M), quarter_s)
    assert (x, y, z) == pytest.approx((0.0, -GPS_ORBIT_RADIUS_M, OFFSET_M), abs=TOLERANCE_M)


def rinex_line(start: str, values: list[str]) -> str:
    return start + "".join(v.rjust(ephemeris.RINEX_FIELD_WIDTH) for v in values) + "\n"


PRN = 7
FIELD_FRACTION = 0.5
"""Each test field's value is its 1-based position in the record plus this."""


def field_value(name: str) -> float:
    names = [f.name for f in fields(Ephemeris)]
    return names.index(name) + FIELD_FRACTION


def test_read_nav_takes_gps_records_only() -> None:
    header = "     3.05           NAVIGATION DATA     MIXED".ljust(60) + "RINEX VERSION / TYPE\n"
    header += " " * 60 + ephemeris.RINEX_END_OF_HEADER + "\n"
    glonass = rinex_line("R01 2026 11 10 00 00 00", ["1.0D+00"] * ephemeris.RINEX_FIRST_LINE_FIELDS)
    glonass += rinex_line("    ", ["2.0D+00"] * ephemeris.RINEX_FIELDS_PER_LINE)
    names = [f.name for f in fields(Ephemeris)][1:]
    numbers = [f"{field_value(name)}D+00" for name in names[:-1]] + [""]
    first = ephemeris.RINEX_FIRST_LINE_FIELDS
    gps = rinex_line(f"G{PRN:02d} 2026 11 10 02 00 00", numbers[:first])
    gps += "".join(
        rinex_line("    ", numbers[k : k + ephemeris.RINEX_FIELDS_PER_LINE])
        for k in range(first, len(numbers), ephemeris.RINEX_FIELDS_PER_LINE)
    )
    (eph,) = ephemeris.read_nav((header + glonass + gps).splitlines(keepends=True))
    assert eph.prn == PRN
    for name in ("clock_bias", "sqrt_a", "transmission_time"):
        assert getattr(eph, name) == field_value(name)
    assert eph.fit_interval == 0.0


def test_select_takes_the_nearest_healthy_ephemeris_within_its_fit() -> None:
    nearest = circular(toe=TOE_S)
    older = circular(toe=TOE_S - ephemeris.SECONDS_PER_HOUR)
    sick = circular(prn=2, health=1.0)
    stale = circular(
        prn=3, toe=TOE_S - ephemeris.DEFAULT_FIT_INTERVAL_H * ephemeris.SECONDS_PER_HOUR
    )
    chosen = ephemeris.select([older, nearest, sick, stale], nearest.toe_gps_s())
    assert chosen == {1: nearest}


def test_unix_to_gps() -> None:
    assert ephemeris.unix_to_gps(ephemeris.GPS_EPOCH_UNIX_S) == ephemeris.GPS_LESS_UTC_S


def test_held_closer_to_the_satellite_puts_the_pulse_late() -> None:
    overhead = replace(EQUATOR, height_m=GPS_ORBIT_RADIUS_M).ecef()
    true = EQUATOR.ecef()
    raised = replace(EQUATOR, height_m=OFFSET_M).ecef()
    late_ns = OFFSET_M / ephemeris.SPEED_OF_LIGHT_M_PER_S * ephemeris.NS_PER_S
    assert ephemeris.hold_late_ns(overhead, true, raised) == pytest.approx(late_ns)
    assert ephemeris.hold_late_ns(overhead, raised, true) == pytest.approx(-late_ns)


def test_solution_error_means() -> None:
    assert ephemeris.solution_error([]) is None
    high = SatelliteError(1, QUARTER_TURN_DEG, 0.0, OFFSET_M)
    low = SatelliteError(2, ELEVATION_DEG, HALF_TURN_DEG, -OFFSET_M)
    solution = ephemeris.solution_error([high, low])
    assert solution is not None
    assert solution.satellites == 2
    assert solution.late_ns == pytest.approx(0.0, abs=TOLERANCE_NS)
    low_weight = math.sin(math.radians(ELEVATION_DEG)) ** 2
    assert solution.weighted_late_ns == pytest.approx(
        OFFSET_M * (1 - low_weight) / (1 + low_weight)
    )


def test_satellite_errors_masks_by_elevation_from_the_true_position() -> None:
    eph = circular()
    gps_s = eph.toe_gps_s()
    raised = replace(EQUATOR, height_m=OFFSET_M)
    (error,) = ephemeris.satellite_errors([eph], gps_s, EQUATOR, raised, ephemeris.DEFAULT_MASK_DEG)
    assert error.prn == eph.prn
    assert error.elevation_deg > ephemeris.DEFAULT_MASK_DEG
    assert error.late_ns > 0
    assert ephemeris.satellite_errors([eph], gps_s, POLE, raised, ephemeris.DEFAULT_MASK_DEG) == []
