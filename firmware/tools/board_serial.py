#!/usr/bin/env python3
"""Print the `@@Cj` serial of the board on the debug probe, from its unique ID, and
the build its flash holds.

Reads only.  The serial is `BoardIdentity::new`'s (oncore-core/src/identity.rs), so
the board about to be flashed can be told from one on a jig.  The build is the
identity `oncore-fw/build.rs` puts in the image: its commit, features and date.
"""

import struct
import subprocess
import sys

CHIP = "STM32C091FC"
# The STM32C0's unique ID (RM0490), the address embassy's `uid()` reads.
UID_ADDRESS = "0x1FFF7550"
UID_WORDS = 3
FLASH_ADDRESS = "0x08000000"
FLASH_BYTES = 256 * 1024
BUILD_TAG = b"ONCORE-BUILD "
BYTE_DIGITS = 2
FNV_OFFSET = 0x811C9DC5
FNV_PRIME = 0x01000193
MASK_32_BITS = 0xFFFFFFFF
SERIAL_MODULUS = 10_000_000
WORD_DIGITS = 8


def serial(unique: bytes) -> str:
    """`EMU` and seven digits: FNV-1a over the unique ID."""
    value = FNV_OFFSET
    for byte in unique:
        value = ((value ^ byte) * FNV_PRIME) & MASK_32_BITS
    return f"EMU{value % SERIAL_MODULUS:07d}"


def unique_id(output: str) -> bytes:
    """The unique ID's bytes from `probe-rs read b32`'s output."""
    words = [int(token, 16) for token in output.split() if len(token) == WORD_DIGITS]
    if len(words) != UID_WORDS:
        raise ValueError(f"expected {UID_WORDS} words, got {output!r}")
    return struct.pack(f"<{UID_WORDS}I", *words)


def build_identity(flash: bytes) -> str | None:
    """The build identity in a flash image, to its NUL; None if it has none."""
    at = flash.find(BUILD_TAG)
    if at < 0:
        return None
    end = flash.find(b"\0", at)
    return flash[at : end if end >= 0 else len(flash)].decode("ascii", errors="replace")


def flash_bytes(output: str) -> bytes:
    """The bytes from `probe-rs read b8`'s output."""
    return bytes(int(token, 16) for token in output.split() if len(token) == BYTE_DIGITS)


def probe_read(width: str, address: str, count: int) -> str:
    """`probe-rs read`'s output; raises RuntimeError if it fails."""
    read = subprocess.run(
        ["probe-rs", "read", "--chip", CHIP, "--protocol", "swd", width, address, str(count)],
        capture_output=True,
        text=True,
    )
    if read.returncode != 0:
        raise RuntimeError(read.stderr.strip())
    return read.stdout


def main() -> int:
    try:
        print(f"board on the probe: {serial(unique_id(probe_read('b32', UID_ADDRESS, UID_WORDS)))}")
        build = build_identity(flash_bytes(probe_read("b8", FLASH_ADDRESS, FLASH_BYTES)))
    except (RuntimeError, ValueError) as error:
        print(f"could not read the board: {error}", file=sys.stderr)
        return 1
    print(f"its build: {build or 'none recorded (built before the build identity)'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
