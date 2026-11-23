"""Tests for the ROM bootloader's protocol against a fake bootloader."""

import pytest

import oncore
import stm32boot
from stm32boot import ACK, FLASH, NACK, Bootloader, BootloaderError

PRODUCT_ID = 0x44D
"""The STM32C091/92's."""
IMAGE_BYTES = 600
"""An image over two chunks, not a whole number of words."""


class FakeBootloader:
    """Flash as a dict, answering AN3155's commands as a port."""

    def __init__(self) -> None:
        self.timeout: float | None = stm32boot.READ_TIMEOUT_S
        self.memory: dict[int, int] = {}
        self._out = b""
        self._in = b""
        self.erased = False
        self.started: int | None = None

    def reset_input_buffer(self) -> None:
        self._out = b""

    def flush(self) -> None:
        pass

    def read(self, size: int, /) -> bytes:
        data, self._out = self._out[:size], self._out[size:]
        return data

    def write(self, data: bytes, /) -> None:
        self._in += data
        self._step()

    def _take(self, n: int) -> bytes | None:
        if len(self._in) < n:
            return None
        data, self._in = self._in[:n], self._in[n:]
        return data

    def _step(self) -> None:
        if self._in[:1] == bytes([stm32boot.SYNC]):
            self._in = self._in[1:]
            self._out += bytes([ACK])
        if len(self._in) < 2:
            return
        code = self._in[0]
        if self._in[1] != code ^ 0xFF:
            self._in = b""
            self._out += bytes([NACK])
            return
        body = self._in[2:]
        if code == stm32boot.GET_ID:
            self._in = b""
            self._out += bytes([ACK, 1]) + PRODUCT_ID.to_bytes(2, "big") + bytes([ACK])
        elif code == stm32boot.EXTENDED_ERASE:
            self._out += bytes([ACK]) if not body else b""
            if len(body) == 3:
                self.memory.clear()
                self.erased = True
                self._in = b""
                self._out += bytes([ACK])
        elif code == stm32boot.GO:
            if not body:
                self._out += bytes([ACK])
            elif len(body) == 5:
                self.started = int.from_bytes(body[:4], "big")
                self._in = b""
                self._out += bytes([ACK])
        elif code == stm32boot.WRITE_MEMORY:
            self._memory_op(body, write=True)
        elif code == stm32boot.READ_MEMORY:
            self._memory_op(body, write=False)

    def _memory_op(self, body: bytes, write: bool) -> None:
        if not body:
            self._out += bytes([ACK])
            return
        if len(body) == 5:
            self._out += bytes([ACK])
            return
        address = int.from_bytes(body[:4], "big")
        count = body[5] + 1
        if write:
            if len(body) < 5 + 1 + count + 1:
                return
            data = body[6 : 6 + count]
            for i, b in enumerate(data):
                self.memory[address + i] = b
            self._out += bytes([ACK])
        else:
            if len(body) < 7:
                return
            self._out += bytes([ACK]) + bytes(
                self.memory.get(address + i, stm32boot.ERASED) for i in range(count)
            )
        self._in = b""


def test_checksums_and_frames() -> None:
    assert stm32boot.address_frame(FLASH) == b"\x08\x00\x00\x00\x08"
    assert stm32boot.write_frame(b"\x01\x02\x03\x04") == b"\x03\x01\x02\x03\x04\x07"
    assert stm32boot.padded(b"\x01") == b"\x01\xff\xff\xff"


def test_id_and_flash_and_verify() -> None:
    fake = FakeBootloader()
    boot = Bootloader(fake)
    boot.connect()
    assert boot.product_id() == PRODUCT_ID
    image = bytes(i % 251 for i in range(IMAGE_BYTES))
    boot.flash(image)
    assert fake.erased
    assert bytes(fake.memory[FLASH + i] for i in range(IMAGE_BYTES)) == image
    assert fake.timeout == stm32boot.READ_TIMEOUT_S
    boot.go(FLASH)
    assert fake.started == FLASH


def test_a_silent_port_is_reported() -> None:
    class Silent(FakeBootloader):
        def write(self, data: bytes, /) -> None:
            pass

    with pytest.raises(BootloaderError):
        Bootloader(Silent()).connect()


class FakePort:
    """A serial port that answers each write with `answer`."""

    def __init__(self, answer: bytes) -> None:
        self.timeout: float | None = stm32boot.ECHO_READ_TIMEOUT_S
        self.written = b""
        self._answer = answer
        self._pending = b""

    def reset_input_buffer(self) -> None:
        self._pending = b""

    def write(self, data: bytes) -> None:
        self.written += data
        self._pending += self._answer

    def flush(self) -> None:
        pass

    def read(self, n: int) -> bytes:
        out, self._pending = self._pending[:n], self._pending[n:]
        return out


def test_the_knock_and_its_echo() -> None:
    assert oncore.BOOTLOADER_KNOCK == b"@@ZbLOAD" + bytes([oncore.checksum(b"ZbLOAD")]) + b"\r\n"
    echoed = FakePort(oncore.BOOTLOADER_KNOCK)
    assert stm32boot.knocked(echoed)
    assert echoed.written == oncore.BOOTLOADER_KNOCK
    assert not stm32boot.knocked(FakePort(b""), wait_s=0.05)


IDENTITY = "ONCORE-BUILD fe3802acd01d vp 20261121"
"""A release image's build identity."""


def flashed_identity(fake: FakeBootloader, offset: int) -> None:
    """`IDENTITY`, NUL-padded as the firmware has it, at `offset` into flash."""
    stored = IDENTITY.encode().ljust(stm32boot.BUILD_ID_LEN, b"\0")
    for i, b in enumerate(stored):
        fake.memory[FLASH + offset + i] = b


@pytest.mark.parametrize(
    "offset",
    [
        IMAGE_BYTES,
        stm32boot.CHUNK - len(stm32boot.BUILD_TAG) // 2,
        stm32boot.CHUNK - len(IDENTITY) // 2,
    ],
    ids=["in a chunk", "tag across chunks", "identity across chunks"],
)
def test_the_build_identity_is_read_from_flash(offset: int) -> None:
    fake = FakeBootloader()
    flashed_identity(fake, offset)
    assert Bootloader(fake).build_identity() == IDENTITY


def test_flash_without_a_build_identity() -> None:
    assert Bootloader(FakeBootloader()).build_identity() is None
