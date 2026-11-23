import math
import struct

import pytest

import analysis
import stability
import ubx

TI_S = 1.0e-6
QERR_NS = -4.0
SAWTOOTH_NS = 7.0
FALSE_START_S = 0.8
SECONDS = 4
START_S = 1000.0
READ_DELAY_S = 0.002
SAW_LEAD_S = 0.4
SLOW_READ_DELAY_S = 0.52
TOW_MS = 343_559_000
QERR_PS = -2_392
ALTERNATING = [1.0, -1.0] * 8
PHASE_RAMP = [float(i) for i in range(16)]


def log(source: str, sawtooth_ns: float, readings: list[float]) -> analysis.Log:
    """Readings each a second apart, each after a sawtooth that came before it."""
    return analysis.Log(
        readings=[
            analysis.Reading(START_S + i + READ_DELAY_S, ti) for i, ti in enumerate(readings)
        ],
        sawtooths={
            source: [
                analysis.Sawtooth(START_S + i - SAW_LEAD_S, sawtooth_ns)
                for i in range(len(readings))
            ]
        },
        events=[],
    )


def test_a_module_s_pulse_is_late_by_minus_qerr() -> None:
    seconds, outside = stability.seconds(log(stability.MODULE, QERR_NS, [TI_S]), stability.MODULE)
    (second,) = seconds.values()
    assert outside == 0
    assert second.corrected_ns == pytest.approx(TI_S / stability.NS + QERR_NS)


def test_an_oncore_s_pulse_is_late_by_its_sawtooth() -> None:
    seconds, _ = stability.seconds(log("vp", SAWTOOTH_NS, [TI_S]), "vp")
    (second,) = seconds.values()
    assert second.corrected_ns == pytest.approx(TI_S / stability.NS - SAWTOOTH_NS)


def test_a_source_with_no_sawtooth_is_taken_as_it_is() -> None:
    seconds, _ = stability.seconds(log("vp", SAWTOOTH_NS, [TI_S]), None)
    (second,) = seconds.values()
    assert second.sawtooth_ns == 0
    assert second.corrected_ns == pytest.approx(TI_S / stability.NS)


def test_a_reading_slow_to_come_back_keeps_its_pulse_s_second() -> None:
    late = log(stability.MODULE, QERR_NS, [TI_S] * SECONDS)
    slow = 1
    late.readings[slow] = analysis.Reading(START_S + slow + SLOW_READ_DELAY_S, TI_S)
    seconds, _ = stability.seconds(late, stability.MODULE)
    assert sorted(seconds) == [int(START_S) + i for i in range(SECONDS)]


def test_a_false_start_is_outside_and_leaves_its_second_missing() -> None:
    readings = [TI_S, FALSE_START_S, TI_S, TI_S]
    a, outside = stability.seconds(log(stability.MODULE, QERR_NS, readings), stability.MODULE)
    b, _ = stability.seconds(log("vp", SAWTOOTH_NS, [TI_S] * SECONDS), "vp")
    joined = stability.series(a, b, "ublox", "vp")
    assert outside == 1
    assert joined.missing == [1]
    assert [r["s"] for r in joined.rows] == [0, 2, 3]
    assert joined.rows[0]["diff"] == pytest.approx(QERR_NS + SAWTOOTH_NS)


def test_avar_of_alternating_phase() -> None:
    # x[i+2] - 2 x[i+1] + x[i] = +-4 every time.
    assert stability.avar(ALTERNATING, 1) == pytest.approx(16 / 2)


def test_a_phase_ramp_a_frequency_offset_has_no_variance() -> None:
    assert stability.avar(PHASE_RAMP, 2) == pytest.approx(0)
    assert stability.mvar(PHASE_RAMP, 2) == pytest.approx(0)


def test_mvar_at_one_is_avar() -> None:
    assert stability.mvar(ALTERNATING, 1) == pytest.approx(stability.avar(ALTERNATING, 1))


def test_tdev_is_tau_mdev_over_root_3() -> None:
    m, variance = 10, 3e-20
    assert stability.tdev_ns(m, variance) == pytest.approx(m * math.sqrt(variance / 3) / 1e-9)


def test_the_hat_splits_independent_variances() -> None:
    a, b, c = 1.0, 4.0, 9.0
    assert stability.hat(a + b, a + c, b + c) == pytest.approx((a, b, c))


def test_tim_tp_read_from_a_frame() -> None:
    payload = struct.pack("<IIiHBB", TOW_MS, 0, QERR_PS, 0, 0, 0)
    (message, body), *_ = ubx.Deframer().feed(b"\x00junk" + ubx.frame(ubx.TIM_TP, payload))
    tp = ubx.time_pulse(body)
    assert message == ubx.TIM_TP
    assert tp == ubx.TimePulse(TOW_MS, QERR_PS / 1000)


def test_a_bad_checksum_is_dropped() -> None:
    bad = bytearray(ubx.frame(ubx.TIM_TP, bytes(ubx.TIM_TP_LEN)))
    bad[-1] ^= 0xFF
    assert ubx.Deframer().feed(bytes(bad)) == []
