"""SCPI instruments on the bench: a raw socket (the 53230A, the scope) or VISA (a GPIB
instrument behind the E5810A), and the counters' time-interval set-ups.

An address with "::" in it is a VISA resource (`TCPIP::e5810a::gpib0,3::INSTR`); anything
else is a host whose SCPI socket is port 5025.  A query that gets no answer in time raises
`TimeoutError` either way.
"""

import socket
from dataclasses import dataclass
from typing import Protocol

import pyvisa
import pyvisa.constants
from pyvisa.resources import MessageBasedResource

SCPI_PORT = 5025
VISA_MARKER = "::"
MS_PER_S = 1000
NO_ANSWER = (
    pyvisa.constants.StatusCode.error_timeout,
    pyvisa.constants.StatusCode.error_io,
)
"""VISA errors meaning the instrument gave no answer: a GPIB timeout through the E5810A
comes back as an I/O error, not a timeout."""


class Scpi(Protocol):
    """A SCPI instrument: commands out, one line back per query."""

    def send(self, command: str) -> None: ...

    def query(self, command: str) -> str: ...

    def reconnect(self) -> None:
        """After a timeout, back to a state where the next query is answered."""
        ...

    def close(self) -> None: ...


class SocketScpi:
    """An instrument on its raw SCPI socket."""

    def __init__(self, host: str, timeout_s: float) -> None:
        self._host = host
        self._timeout_s = timeout_s
        self._connect()

    def _connect(self) -> None:
        self._socket = socket.create_connection((self._host, SCPI_PORT), timeout=self._timeout_s)
        self._file = self._socket.makefile("rwb")

    def send(self, command: str) -> None:
        self._file.write(command.encode() + b"\n")
        self._file.flush()

    def query(self, command: str) -> str:
        self.send(command)
        return self._file.readline().decode().strip()

    def reconnect(self) -> None:
        """A socket's file cannot be read again after a timeout: a new connection."""
        self.close()
        self._connect()

    def close(self) -> None:
        self._file.close()
        self._socket.close()


class VisaScpi:
    """An instrument through VISA (pyvisa-py)."""

    def __init__(self, resource: str, timeout_s: float) -> None:
        self._manager = pyvisa.ResourceManager("@py")
        opened = self._manager.open_resource(resource)
        if not isinstance(opened, MessageBasedResource):
            raise TypeError(f"{resource}: not a message-based instrument")
        self._instrument = opened
        self._instrument.timeout = timeout_s * MS_PER_S
        self._instrument.read_termination = "\n"
        self._instrument.write_termination = "\n"

    def send(self, command: str) -> None:
        self._instrument.write(command)

    def query(self, command: str) -> str:
        try:
            return str(self._instrument.query(command)).strip()
        except pyvisa.errors.VisaIOError as error:
            if error.error_code in NO_ANSWER:
                raise TimeoutError(command) from error
            raise

    def reconnect(self) -> None:
        """A device clear ends the pending measurement and empties the output queue."""
        self._instrument.clear()

    def close(self) -> None:
        self._instrument.close()
        self._manager.close()


def connect(address: str, timeout_s: float) -> Scpi:
    """The instrument at `address`: a VISA resource or a socket host."""
    if VISA_MARKER in address:
        return VisaScpi(address, timeout_s)
    return SocketScpi(address, timeout_s)


@dataclass(frozen=True)
class Counter:
    """A counter model's time interval from channel 1 to channel 2, in its own SCPI."""

    name: str
    session: tuple[str, ...]
    """Sent on every connection: its measurement timeout, where it has one."""
    prepare: tuple[str, ...]
    """Sent before the channels, after `session`: stop, clear and the function."""
    channel: tuple[str, ...]
    """Each channel's settings, `{c}` the channel, `{level}` and `{impedance}` filled in."""
    finish: tuple[str, ...]
    """Sent after the channels: sampling and triggering."""
    level_query: str
    impedance_query: str
    read: str
    """One measurement, triggered and returned."""
    stop: str
    """Ends a measurement that timed out."""

    def setup(self, levels: dict[int, float], impedance: float) -> list[str]:
        """The commands that set it up at each channel's level, rising edges, DC."""
        channels = [
            setting.format(c=c, level=f"{level:.3f}", impedance=f"{impedance:g}")
            for c, level in levels.items()
            for setting in self.channel
        ]
        return [*self.session, *self.prepare, *channels, *self.finish]


MEASUREMENT_TIMEOUT_S = 5
"""Longer than a 1 PPS period: the 53230A's default measurement timeout, 1 s, times out."""
HIGH_IMPEDANCE = 1e6
"""Mid-line, where the scope terminates the line."""

COUNTERS = {
    "53230A": Counter(
        name="53230A",
        session=(f"SYST:TIM {MEASUREMENT_TIMEOUT_S}",),
        prepare=("ABOR", "*CLS", "CONF:TINT (@1),(@2)"),
        channel=(
            "INP{c}:IMP {impedance}",
            "INP{c}:COUP DC",
            "INP{c}:SLOP POS",
            "INP{c}:LEV {level}",
        ),
        finish=("SAMP:COUN 1", "TRIG:SOUR IMM"),
        level_query="INP{c}:LEV?",
        impedance_query="INP{c}:IMP?",
        read="READ?",
        stop="ABOR",
    ),
    "53132A": Counter(
        name="53132A",
        session=(),
        prepare=("ABOR", "*CLS", ':FUNC "TINT 1,2"', "INIT:CONT OFF"),
        channel=(
            "INP{c}:IMP {impedance}",
            "INP{c}:COUP DC",
            "INP{c}:FILT OFF",
            "EVEN{c}:LEV:AUTO OFF",
            "EVEN{c}:SLOP POS",
            "EVEN{c}:LEV {level}",
        ),
        finish=("TINT:ARM:STAR:SOUR IMM", "TINT:ARM:STOP:SOUR IMM"),
        level_query="EVEN{c}:LEV?",
        impedance_query="INP{c}:IMP?",
        read="READ?",
        stop="ABOR",
    ),
}
