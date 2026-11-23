"""UBX framing and the TIM-TP fields the 1 PPS comparison needs (u-blox M8 protocol,
UBX-13003221 R28): polls only, nothing written to the receiver's configuration.
"""

import struct
from dataclasses import dataclass

SYNC = b"\xb5\x62"
HEADER_LEN = 6
"""Sync, class, ID and length."""
CHECKSUM_LEN = 2
TIM_TP = (0x0D, 0x01)
"""UBX-TIM-TP, the next time pulse's time and quantization error (R28 32.20.3)."""
TIM_TP_LEN = 16
PS_PER_NS = 1000


def checksum(body: bytes) -> bytes:
    """The 8-bit Fletcher checksum over class, ID, length and payload."""
    a = b = 0
    for byte in body:
        a = (a + byte) & 0xFF
        b = (b + a) & 0xFF
    return bytes([a, b])


def frame(message: tuple[int, int], payload: bytes = b"") -> bytes:
    """A whole message; an empty payload polls."""
    body = bytes(message) + struct.pack("<H", len(payload)) + payload
    return SYNC + body + checksum(body)


@dataclass(frozen=True)
class TimePulse:
    """TIM-TP's fields used here."""

    tow_ms: int
    """Time of week of the next pulse, ms."""
    q_err_ns: float
    """Its quantization error: the pulse is late by -qErr."""


def time_pulse(payload: bytes) -> TimePulse | None:
    """TIM-TP's payload read, or None if it is not one."""
    if len(payload) != TIM_TP_LEN:
        return None
    tow_ms, _sub_ms, q_err_ps = struct.unpack_from("<IIi", payload)
    return TimePulse(tow_ms, q_err_ps / PS_PER_NS)


class Deframer:
    """Whole, good messages from a byte stream."""

    def __init__(self) -> None:
        self._buffer = bytearray()

    def feed(self, data: bytes) -> list[tuple[tuple[int, int], bytes]]:
        """Each complete message so far: its class and ID, and its payload."""
        self._buffer += data
        out = []
        while (at := self._buffer.find(SYNC)) >= 0:
            del self._buffer[:at]
            if len(self._buffer) < HEADER_LEN:
                break
            (length,) = struct.unpack_from("<H", self._buffer, 4)
            end = HEADER_LEN + length + CHECKSUM_LEN
            if len(self._buffer) < end:
                break
            body = bytes(self._buffer[2 : HEADER_LEN + length])
            if checksum(body) == bytes(self._buffer[end - CHECKSUM_LEN : end]):
                out.append(((body[0], body[1]), body[4:]))
                del self._buffer[:end]
            else:
                del self._buffer[: len(SYNC)]
        return out
