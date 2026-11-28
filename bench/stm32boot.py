"""Flashes an STM32 through its ROM serial bootloader (ST AN3155, USART protocol), for parts
`stm32flash` does not know, such as the STM32C091 (ID 0x44d): connect, ID, mass erase, write,
read back, and with `--go` start the firmware (the emulator's then resets itself, out of the
bootloader's state).  A blank STM32C0's first flash needs a power cycle instead, to clear its
EMPTY flag (AN5673).  `build` prints the build identity the flash holds (`oncore-fw/build.rs`),
as `firmware/tools/board_serial.py` does over SWD.

    uv run python stm32boot.py id [--knock] DEVICE
    uv run python stm32boot.py write [--knock] [--go] DEVICE FIRMWARE.bin
    uv run python stm32boot.py build [--knock] [--go] DEVICE
    uv run python stm32boot.py {id,build} --relay DEVICE

The board's bootloader must be running: a blank part starts it itself; `--knock` has the
emulator's firmware start it, with the bench knock, `@@Zb`, on the instrument's port (J1 through
the jig, or J5) at 9600 8N1, answered with the same frame before the board jumps.

`--relay` reaches a board in a SmartClock through the instrument's console relay
(`smartclock-cli passthrough --baud 9600 --framing 8N1 --then-baud 19200 --then-framing 8E1`,
started first), DEVICE the instrument's port: zeros, the knock and its echo at 9600 8N1, a break
that switches the relay to 19200 8E1, the bootloader at that rate, Go, and a last break that resets
the instrument.
"""

import argparse
import sys
import time
from pathlib import Path
from typing import Protocol

import serial

import oncore

BAUD = 57600
"""8E1; the bootloader takes its rate from the first 0x7F (AN3155, 2)."""
SYNC = 0x7F
ACK = 0x79
NACK = 0x1F
GET_ID = 0x02
GO = 0x21
READ_MEMORY = 0x11
WRITE_MEMORY = 0x31
EXTENDED_ERASE = 0x44
MASS_ERASE = b"\xff\xff"
"""Extended erase's special code for all of flash (AN3155, 3.8)."""
FLASH = 0x0800_0000
FLASH_BYTES = 256 * 1024
"""The STM32C091's."""
BUILD_TAG = b"ONCORE-BUILD "
BUILD_ID_LEN = 96
"""The build identity's room in flash, NUL-padded (`oncore-fw/src/main.rs`)."""
CHUNK = 256
"""The most one read or write moves (AN3155, 3.4 and 3.5)."""
WORD = 4
"""Writes are whole words; the last is padded with erased bytes."""
ERASED = 0xFF
READ_TIMEOUT_S = 1.0
ERASE_TIMEOUT_S = 30.0
ONCORE_BAUD = 9600
"""The instrument's port before the knock, 8N1 (VP reference p. 2)."""
ECHO_WAIT_S = 2.0
"""How long the knock's echo may take: a command is answered within the second."""
ECHO_READ_TIMEOUT_S = 0.1
RELAY_BAUD = 19200
"""The bootloader's rate through the relay, 8E1: the relay's second setting."""
RELAY_BREAK_S = 0.02
"""A break: the first switches the relay to its second setting, the next resets the instrument."""
RELAY_SETTLE_S = 0.03
"""After the switch, time for the byte the relay sends on switching (`E`, often garbled, as the
port is still at 9600 8N1 when it comes) to arrive through a USB adapter's latency timer, to be
discarded before the sync."""
LONGEST_COMMAND = 22
"""The board's longest instrument command, `@@` through LF (smartclock-gps `MAX_COMMAND`)."""
RELAY_FILLER = bytes(LONGEST_COMMAND)
"""Zeros before the knock through the relay: a command the instrument had begun when the relay
froze it ends in them (a bad checksum), and the board's deframer is idle for the knock."""


class Port(Protocol):
    """What the bootloader needs of a serial port."""

    timeout: float | None

    def write(self, data: bytes, /) -> object: ...
    def read(self, size: int, /) -> bytes: ...
    def reset_input_buffer(self) -> None: ...
    def flush(self) -> None: ...


class RelayPort(Port, Protocol):
    """What the relay needs besides: the break condition, and the rate and parity changed while
    open."""

    baudrate: int
    parity: str
    break_condition: bool


class BootloaderError(Exception):
    """The bootloader refused or did not answer."""


def knocked(port: Port, wait_s: float = ECHO_WAIT_S, lead: bytes = b"") -> bool:
    """Sends `lead` and the knock on `port` and whether the knock's echo came back within
    `wait_s`."""
    port.reset_input_buffer()
    port.write(lead + oncore.BOOTLOADER_KNOCK)
    port.flush()
    heard = b""
    deadline = time.monotonic() + wait_s
    while time.monotonic() < deadline:
        heard += port.read(len(oncore.BOOTLOADER_KNOCK))
        if oncore.BOOTLOADER_KNOCK in heard:
            return True
    return False


def xor(data: bytes) -> int:
    """The XOR of `data`, AN3155's checksum."""
    total = 0
    for b in data:
        total ^= b
    return total


def address_frame(address: int) -> bytes:
    """An address, big-endian, and its checksum."""
    a = address.to_bytes(4, "big")
    return a + bytes([xor(a)])


def write_frame(data: bytes) -> bytes:
    """Write memory's count less one, the data, and their checksum."""
    body = bytes([len(data) - 1]) + data
    return body + bytes([xor(body)])


def padded(data: bytes) -> bytes:
    """`data` to a whole number of words, with erased bytes."""
    return data + bytes([ERASED]) * (-len(data) % WORD)


class Bootloader:
    """The ROM bootloader on `port`."""

    def __init__(self, port: Port) -> None:
        self._port = port

    def _ack(self, what: str) -> None:
        reply = self._port.read(1)
        if reply != bytes([ACK]):
            raise BootloaderError(f"{what}: {'NACK' if reply == bytes([NACK]) else 'no answer'}")

    def _send(self, data: bytes, what: str) -> None:
        self._port.write(data)
        self._ack(what)

    def _command(self, code: int, what: str) -> None:
        self._send(bytes([code, code ^ 0xFF]), what)

    def connect(self) -> None:
        """Syncs the bootloader's baud rate.  One already synced takes 0x7F as a command's
        first byte and waits; a second makes a bad command, which it answers NACK.  A stray
        byte before the answer (one late through a USB adapter) is passed over, not taken
        for no answer, which would send a second 0x7F to a bootloader that took the first."""
        self._port.reset_input_buffer()
        for _ in range(2):
            self._port.write(bytes([SYNC]))
            while reply := self._port.read(1):
                if reply in (bytes([ACK]), bytes([NACK])):
                    return
        raise BootloaderError("no answer to 0x7F: is the bootloader running?")

    def product_id(self) -> int:
        self._command(GET_ID, "get ID")
        count = self._port.read(1)
        if not count:
            raise BootloaderError("get ID: no answer")
        pid = self._port.read(count[0] + 1)
        self._ack("get ID")
        return int.from_bytes(pid, "big")

    def mass_erase(self) -> None:
        self._command(EXTENDED_ERASE, "erase")
        timeout, self._port.timeout = self._port.timeout, ERASE_TIMEOUT_S
        try:
            self._send(MASS_ERASE + bytes([xor(MASS_ERASE)]), "mass erase")
        finally:
            self._port.timeout = timeout

    def write(self, address: int, data: bytes) -> None:
        """`data`, at most [`CHUNK`] bytes, at `address`."""
        self._command(WRITE_MEMORY, "write")
        self._send(address_frame(address), f"write at {address:#010x}")
        self._send(write_frame(padded(data)), f"write at {address:#010x}")

    def read(self, address: int, count: int) -> bytes:
        """`count` bytes, at most [`CHUNK`], from `address`."""
        self._command(READ_MEMORY, "read")
        self._send(address_frame(address), f"read at {address:#010x}")
        self._send(bytes([count - 1, (count - 1) ^ 0xFF]), f"read at {address:#010x}")
        data = self._port.read(count)
        if len(data) != count:
            raise BootloaderError(f"read at {address:#010x}: {len(data)} of {count} bytes")
        return data

    def go(self, address: int) -> None:
        """Starts the code whose vector table is at `address`."""
        self._command(GO, "go")
        self._send(address_frame(address), f"go to {address:#010x}")

    def build_identity(self) -> str | None:
        """The build identity in flash, to its NUL, reading from flash's start until it is
        whole; None if flash has none."""
        held = b""
        for offset in range(0, FLASH_BYTES, CHUNK):
            searched = max(0, len(held) - len(BUILD_TAG) + 1)
            held += self.read(FLASH + offset, CHUNK)
            at = held.find(BUILD_TAG, searched)
            if at >= 0:
                return self._identity_from(held[at:], FLASH + offset + CHUNK)
        return None

    def _identity_from(self, held: bytes, next_address: int) -> str:
        """The identity starting `held`, reading on from `next_address` until it is whole."""
        while len(held) < BUILD_ID_LEN and next_address < FLASH + FLASH_BYTES:
            held += self.read(next_address, CHUNK)
            next_address += CHUNK
        identity = held[:BUILD_ID_LEN].split(b"\0", 1)[0]
        return identity.decode("ascii", errors="replace")

    def flash(self, image: bytes) -> None:
        """Erases flash, writes `image` from its start, and reads it back."""
        self.mass_erase()
        chunks = range(0, len(image), CHUNK)
        for offset in chunks:
            self.write(FLASH + offset, image[offset : offset + CHUNK])
        for offset in chunks:
            want = image[offset : offset + CHUNK]
            if self.read(FLASH + offset, len(want)) != want:
                raise BootloaderError(f"verify: differs at {FLASH + offset:#010x}")


def relay_break(port: RelayPort) -> None:
    """A break of [`RELAY_BREAK_S`], timed here: pyserial's `send_break` on Linux is
    `tcsendbreak(fd, int(duration / 0.25))`, 0.25 to 0.5 s for anything under 0.25 s."""
    port.break_condition = True
    time.sleep(RELAY_BREAK_S)
    port.break_condition = False


def relay_bootloader(port: RelayPort, wait_s: float = ECHO_WAIT_S) -> Bootloader:
    """The board's bootloader through the relay on `port`, open at 9600 8N1: the knock, its
    echo, and the break that switches both the relay and `port` to 19200 8E1."""
    if not knocked(port, wait_s, lead=RELAY_FILLER):
        raise BootloaderError("no answer to the knock")
    relay_break(port)
    port.baudrate = RELAY_BAUD
    port.parity = serial.PARITY_EVEN
    time.sleep(RELAY_SETTLE_S)
    return Bootloader(port)


def run(boot: Bootloader, action: str, image: Path | None, go: bool) -> bool:
    """`action` on the synced bootloader `boot`, then Go if `go`; whether it found what it
    looked for."""
    boot.connect()
    print(f"product ID {boot.product_id():#05x}")
    found = True
    if action == "write" and image is not None:
        data = image.read_bytes()
        boot.flash(data)
        print(f"wrote and verified {len(data)} bytes at {FLASH:#010x}")
    elif action == "build":
        identity = boot.build_identity()
        found = identity is not None
        print(f"its build: {identity}" if found else "its build: none in flash")
    if go:
        boot.go(FLASH)
        print("started")
    elif action != "id":
        print("power-cycle the board to start it")
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument(
        "--knock", action="store_true", help="start the emulator's bootloader with @@Zb first"
    )
    common.add_argument(
        "--relay",
        action="store_true",
        help="through a SmartClock's console relay: knock, switch, bootloader, Go, reset",
    )
    common.add_argument("device")
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("id", parents=[common])
    go = argparse.ArgumentParser(add_help=False)
    go.add_argument("--go", action="store_true", help="start the firmware after")
    write = sub.add_parser("write", parents=[common, go])
    write.add_argument("image", type=Path)
    sub.add_parser("build", parents=[common, go])
    args = parser.parse_args()
    if args.relay and args.action == "write":
        parser.error("--relay does not write yet: a failed write in a closed instrument needs SWD")
    image = getattr(args, "image", None)
    if args.relay:
        with serial.Serial(args.device, ONCORE_BAUD, timeout=ECHO_READ_TIMEOUT_S) as port:
            try:
                boot = relay_bootloader(port)
                print("knock answered; relay switched")
                port.timeout = READ_TIMEOUT_S
                found = run(boot, args.action, image, go=True)
            except BootloaderError as e:
                print(f"{args.device}: {e}; the instrument stays frozen", file=sys.stderr)
                return 1
            relay_break(port)
            print("instrument reset")
        return 0 if found else 1
    if args.knock:
        with serial.Serial(args.device, ONCORE_BAUD, timeout=ECHO_READ_TIMEOUT_S) as port:
            if not knocked(port):
                print(f"{args.device}: no answer to the knock", file=sys.stderr)
                return 1
        print("knock answered")
    with serial.Serial(
        args.device, BAUD, parity=serial.PARITY_EVEN, timeout=READ_TIMEOUT_S
    ) as port:
        try:
            found = run(Bootloader(port), args.action, image, go=getattr(args, "go", False))
        except BootloaderError as e:
            print(f"{args.device}: {e}", file=sys.stderr)
            return 1
    return 0 if found else 1


if __name__ == "__main__":
    sys.exit(main())
