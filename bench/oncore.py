"""Oncore binary framing and the Time RAIM fields the 1 PPS comparison needs: the VP's `@@Bn`
and `@@En` (third_party/motorola/VPCommands.pdf) and the M12+'s `@@Hn` (M12+ User's Guide rev 6.X).
"""

from dataclasses import dataclass


@dataclass(frozen=True)
class TimeRaimFormat:
    """Where a Time RAIM status message's fields are, offsets into the whole message."""

    id: bytes
    length: int
    """`@@` to LF."""
    solution: int
    sawtooth: int
    hour: int | None
    """The next pulse's hour, minute and second, if the message has them."""


BN = TimeRaimFormat(id=b"Bn", length=59, solution=21, sawtooth=25, hour=16)
"""The VP's 6-channel `@@Bn` (VP reference pp. 102-106)."""
EN = TimeRaimFormat(id=b"En", length=69, solution=21, sawtooth=25, hour=16)
"""The VP's 8-channel `@@En` (VP reference pp. 138-141)."""
HN = TimeRaimFormat(id=b"Hn", length=78, solution=6, sawtooth=14, hour=None)
"""The M12+'s 12-channel `@@Hn` (M12+ User's Guide rev 6.X, p. 166)."""


def checksum(body: bytes) -> int:
    """The XOR of a message's ID and data."""
    total = 0
    for b in body:
        total ^= b
    return total


def frame(body: bytes) -> bytes:
    """A whole message from its ID and data."""
    return b"@@" + body + bytes([checksum(body)]) + b"\r\n"


def time_raim_command(rate: int, pulse_mode: int, alarm_limit: int) -> bytes:
    """`@@Bn`'s command, Time RAIM on: rate, alarm limit (100 ns), pulse mode, ten zeros."""
    return frame(
        b"Bn" + bytes([rate, 1]) + alarm_limit.to_bytes(2, "big") + bytes([pulse_mode]) + bytes(10)
    )


def pulse_offset_command(offset_ns: int) -> bytes:
    """`@@Ay`, the 1 PPS offset, 0 to 999,999,999 ns."""
    if not 0 <= offset_ns <= 999_999_999:
        raise ValueError(f"1 PPS offset {offset_ns} ns out of range")
    return frame(b"Ay" + offset_ns.to_bytes(4, "big"))


BOOTLOADER_KNOCK = frame(b"Zb" + b"LOAD")
"""The emulator's bench knock, `@@Zb` with its argument: it answers with the same frame,
then starts the STM32's ROM serial bootloader (`firmware/oncore-fw/src/bootloader.rs`)."""


@dataclass(frozen=True)
class TimeRaim:
    """The fields of one `@@Bn` used here."""

    next_pulse: tuple[int, int, int] | None
    """Hour, minute and second of the next pulse, if the message gives them."""
    solution: int
    """Time RAIM solution status: 0 OK, 1 alarm, 2 unknown."""
    sawtooth_ns: int
    """The negative of the next pulse's quantization error."""


def parse_time_raim(message: bytes, form: TimeRaimFormat = BN) -> TimeRaim | None:
    """A Time RAIM status message's fields, or None if it is not a whole, good one."""
    if (
        len(message) != form.length
        or not message.startswith(b"@@" + form.id)
        or not message.endswith(b"\r\n")
        or checksum(message[2:-3]) != message[-3]
    ):
        return None
    next_pulse = None
    if form.hour is not None:
        hour, minute, second = message[form.hour : form.hour + 3]
        next_pulse = (hour, minute, second)
    return TimeRaim(
        next_pulse=next_pulse,
        solution=message[form.solution],
        sawtooth_ns=int.from_bytes(message[form.sawtooth : form.sawtooth + 1], "big", signed=True),
    )


class Deframer:
    """Splits a byte stream into the Time RAIM status messages of one format in it."""

    def __init__(self, form: TimeRaimFormat = BN) -> None:
        self._buffer = b""
        self._form = form

    def feed(self, data: bytes) -> list[TimeRaim]:
        """Every whole, good message completed by `data`."""
        self._buffer += data
        found: list[TimeRaim] = []
        length = self._form.length
        while (start := self._buffer.find(b"@@" + self._form.id)) >= 0:
            if len(self._buffer) < start + length:
                self._buffer = self._buffer[start:]
                return found
            parsed = parse_time_raim(self._buffer[start : start + length], self._form)
            if parsed is None:
                self._buffer = self._buffer[start + 2 :]
            else:
                found.append(parsed)
                self._buffer = self._buffer[start + length :]
        # Keep a trailing "@" that may start the next message.
        self._buffer = self._buffer[-1:]
        return found
