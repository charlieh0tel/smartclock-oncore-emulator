import pytest
import pyvisa
import pyvisa.constants

import instrument

LEVELS = {1: 1.89, 2: 1.435}
FIFTY_OHMS = 50.0
VISA_RESOURCE = "TCPIP::e5810a::gpib0,3::INSTR"


def test_the_53230a_setup_sets_its_timeout_first_and_each_channel() -> None:
    commands = instrument.COUNTERS["53230A"].setup(LEVELS, instrument.HIGH_IMPEDANCE)
    assert commands[0] == f"SYST:TIM {instrument.MEASUREMENT_TIMEOUT_S}"
    assert "INP1:LEV 1.890" in commands
    assert "INP2:LEV 1.435" in commands
    assert "INP2:IMP 1e+06" in commands
    assert commands[-1] == "TRIG:SOUR IMM"


def test_the_53132a_setup_fixes_its_levels_in_its_own_dialect() -> None:
    commands = instrument.COUNTERS["53132A"].setup(LEVELS, FIFTY_OHMS)
    assert ':FUNC "TINT 1,2"' in commands
    for c, level in LEVELS.items():
        auto_off = commands.index(f"EVEN{c}:LEV:AUTO OFF")
        assert commands.index(f"EVEN{c}:LEV {level:.3f}") > auto_off
        assert f"INP{c}:IMP 50" in commands
    assert not any(c.startswith("SYST:TIM") for c in commands)


@pytest.mark.parametrize("name", sorted(instrument.COUNTERS))
def test_every_channel_gets_every_setting(name: str) -> None:
    model = instrument.COUNTERS[name]
    commands = model.setup(LEVELS, FIFTY_OHMS)
    assert len(commands) == len(model.session + model.prepare + model.finish) + len(
        model.channel
    ) * len(LEVELS)


def test_a_visa_resource_is_told_from_a_host() -> None:
    assert instrument.VISA_MARKER in VISA_RESOURCE
    assert instrument.VISA_MARKER not in "A-53230A-01726.local"


class SilentInstrument:
    """A VISA instrument whose queries fail with a given VISA status."""

    def __init__(self, status: pyvisa.constants.StatusCode) -> None:
        self.status = status

    def query(self, command: str) -> str:
        raise pyvisa.errors.VisaIOError(self.status)


def silent(status: pyvisa.constants.StatusCode) -> instrument.VisaScpi:
    scpi = instrument.VisaScpi.__new__(instrument.VisaScpi)
    scpi._instrument = SilentInstrument(status)  # type: ignore[assignment]
    return scpi


@pytest.mark.parametrize("status", instrument.NO_ANSWER)
def test_no_answer_through_visa_is_a_timeout(status: pyvisa.constants.StatusCode) -> None:
    with pytest.raises(TimeoutError):
        silent(status).query("READ?")


def test_another_visa_error_is_raised_as_it_is() -> None:
    with pytest.raises(pyvisa.errors.VisaIOError):
        silent(pyvisa.constants.StatusCode.error_resource_not_found).query("READ?")
