"""Load a GPS almanac into a VP Oncore with `@@Cb`, from a u-blox module's RXM-SFRBX.

  almanac.py send --ubx FILE --device DEVICE
  almanac.py show --ubx FILE

FILE is raw UBX, such as `firmware/testdata/neo-m8t-2026-10-28.ubx`.  The almanac is the 34
pages of subframes 5 (pages 1 to 25) and 4 (pages 2 to 5, 7 to 10 and 25), each page's words 3
to 10, 24 data bits each (VPCommands.pdf, "Almanac Data Input Message (@@Cb)"; IS-GPS-200,
20.3.3.5).  Subframe 5 page 1 goes first; the VP echoes each page with `@@Ch`.
"""

import argparse
import sys
import time
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path

import serial

import oncore

UBX_SYNC = b"\xb5\x62"
RXM_SFRBX = (0x02, 0x13)
GNSS_GPS = 0
WORDS = 10
BAUD = 9600
ECHO_LENGTH = 9
ECHO_SECONDS = 3.0
SV_PAGE_25 = {5: 51, 4: 63}
"""Word 3's SV ID on each subframe's page 25."""


def subframe_4_page(sv: int) -> int | None:
    """Subframe 4's page for an almanac SV ID (25 to 32), page 25's ID, or None."""
    if 25 <= sv <= 28:
        return sv - 23
    if 29 <= sv <= 32:
        return sv - 22
    return 25 if sv == SV_PAGE_25[4] else None


def subframe_5_page(sv: int) -> int | None:
    """Subframe 5's page for an almanac SV ID (1 to 24), page 25's ID, or None."""
    if 1 <= sv <= 24:
        return sv
    return 25 if sv == SV_PAGE_25[5] else None


ORDER = [(5, page) for page in range(1, 26)] + [(4, page) for page in [2, 3, 4, 5, 7, 8, 9, 10, 25]]
"""The 34 pages, subframe 5 page 1 first."""


def data_bits(word: int) -> int:
    """A 30-bit GPS word's 24 data bits (u-blox: bits 29 to 6, uninverted)."""
    return (word >> 6) & 0xFFFFFF


def ubx_messages(stream: bytes) -> Iterator[tuple[int, int, bytes]]:
    """Each UBX message with a good checksum: class, ID, payload."""
    i = 0
    while (i := stream.find(UBX_SYNC, i)) >= 0 and i + 6 <= len(stream):
        length = int.from_bytes(stream[i + 4 : i + 6], "little")
        end = i + 6 + length + 2
        if end > len(stream):
            return
        body = stream[i + 2 : end - 2]
        a = b = 0
        for byte in body:
            a = (a + byte) & 0xFF
            b = (b + a) & 0xFF
        if bytes([a, b]) == stream[end - 2 : end]:
            yield body[0], body[1], body[4:]
            i = end
        else:
            i += 2


@dataclass(frozen=True)
class Page:
    """One almanac page: its subframe and page number, and words 3 to 10's data bits."""

    subframe: int
    page: int
    words: tuple[int, ...]

    def toa(self) -> int | None:
        """The almanac reference time, units of 4096 s: word 4's top byte on an SV's page,
        word 3's middle byte on subframe 5 page 25."""
        if self.subframe == 5 and self.page == 25:
            return (self.words[0] >> 8) & 0xFF
        if self.page == 25:
            return None
        return (self.words[1] >> 16) & 0xFF

    def command(self) -> bytes:
        """The page's `@@Cb`."""
        data = b"".join(w.to_bytes(3, "big") for w in self.words)
        return oncore.frame(b"Cb" + bytes([self.subframe, self.page]) + data)


def almanac_pages(stream: bytes) -> dict[tuple[int, int], Page]:
    """The latest of each almanac page in a UBX stream's GPS RXM-SFRBX."""
    pages: dict[tuple[int, int], Page] = {}
    for cls, msg_id, payload in ubx_messages(stream):
        if (cls, msg_id) != RXM_SFRBX or payload[0] != GNSS_GPS or payload[4] < WORDS:
            continue
        raw = [int.from_bytes(payload[8 + 4 * k : 12 + 4 * k], "little") for k in range(WORDS)]
        subframe = (data_bits(raw[1]) >> 2) & 7
        sv = (data_bits(raw[2]) >> 16) & 0x3F
        page = {4: subframe_4_page, 5: subframe_5_page}.get(subframe, lambda _: None)(sv)
        if page is not None:
            pages[(subframe, page)] = Page(subframe, page, tuple(data_bits(w) for w in raw[2:]))
    return pages


def check(pages: dict[tuple[int, int], Page]) -> list[str]:
    """What is wrong with an almanac: missing pages, mixed reference times."""
    problems = [f"subframe {s} page {p} missing" for s, p in ORDER if (s, p) not in pages]
    toas = {t for page in pages.values() if (t := page.toa()) is not None}
    if len(toas) > 1:
        problems.append(f"pages from more than one almanac: toa {sorted(toas)}")
    return problems


def send(pages: dict[tuple[int, int], Page], device: str) -> int:
    """Each page's `@@Cb`, in order, waiting for its `@@Ch` echo."""
    failures = 0
    with serial.Serial(device, BAUD, timeout=0.1) as port:
        port.reset_input_buffer()
        for key in ORDER:
            page = pages[key]
            port.write(page.command())
            expected = oncore.frame(b"Ch" + bytes(key))
            buffer = b""
            end = time.monotonic() + ECHO_SECONDS
            while expected not in buffer and time.monotonic() < end:
                buffer += port.read(256)
            if expected not in buffer:
                failures += 1
                print(f"subframe {key[0]} page {key[1]}: no echo", file=sys.stderr)
    print(f"sent {len(ORDER)} pages, {failures} without an echo", file=sys.stderr)
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawTextHelpFormatter
    )
    parser.add_argument("action", choices=["send", "show"])
    parser.add_argument("--ubx", type=Path, required=True)
    parser.add_argument("--device", help="the VP's serial device, for send")
    args = parser.parse_args()
    pages = almanac_pages(args.ubx.read_bytes())
    problems = check(pages)
    toas = sorted({t for page in pages.values() if (t := page.toa()) is not None})
    print(f"{len(pages)} pages, toa {toas} (x 4096 s)", file=sys.stderr)
    for problem in problems:
        print(problem, file=sys.stderr)
    if problems:
        return 1
    if args.action == "send":
        if not args.device:
            parser.error("send needs --device")
        return send(pages, args.device)
    return 0


if __name__ == "__main__":
    sys.exit(main())
