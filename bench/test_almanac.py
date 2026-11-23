"""Tests for the almanac's extraction from RXM-SFRBX and its `@@Cb` messages."""

from pathlib import Path

import almanac
import oncore

TRACE = Path(__file__).parent.parent / "firmware/testdata/neo-m8t-2026-10-28.ubx"


def word(data: int) -> int:
    """A u-blox SFRBX word holding 24 data bits, parity bits zero."""
    return data << 6


def sfrbx(subframe: int, sv: int, toa: int) -> bytes:
    """A GPS RXM-SFRBX message for one almanac page."""
    words = [word(0x8B0000), word(subframe << 2), word(sv << 16), word(toa << 16)]
    words += [word(n) for n in range(6)]
    payload = bytes([0, 1, 0, 0, 10, 0, 2, 0]) + b"".join(w.to_bytes(4, "little") for w in words)
    body = bytes([0x02, 0x13]) + len(payload).to_bytes(2, "little") + payload
    a = b = 0
    for byte in body:
        a = (a + byte) & 0xFF
        b = (b + a) & 0xFF
    return almanac.UBX_SYNC + body + bytes([a, b])


def test_page_numbers() -> None:
    assert [almanac.subframe_4_page(sv) for sv in (25, 28, 29, 32, 63, 57)] == [
        2,
        5,
        7,
        10,
        25,
        None,
    ]
    assert [almanac.subframe_5_page(sv) for sv in (1, 24, 51, 0)] == [1, 24, 25, None]


def test_pages_from_sfrbx_and_their_command() -> None:
    stream = b"junk" + sfrbx(5, 3, 99) + sfrbx(4, 30, 99) + sfrbx(1, 3, 99)
    pages = almanac.almanac_pages(stream)
    assert sorted(pages) == [(4, 8), (5, 3)]
    page = pages[(5, 3)]
    assert page.toa() == 99
    command = page.command()
    assert len(command) == 33
    assert command[:6] == b"@@Cb\x05\x03"
    assert command[6:9] == (3 << 16).to_bytes(3, "big")
    assert command == oncore.frame(command[2:-3])


def test_a_bad_ubx_checksum_is_skipped() -> None:
    message = bytearray(sfrbx(5, 3, 99))
    message[-1] ^= 1
    assert almanac.almanac_pages(bytes(message)) == {}


def test_check_reports_missing_pages_and_mixed_almanacs() -> None:
    pages = almanac.almanac_pages(sfrbx(5, 1, 99) + sfrbx(5, 2, 100))
    problems = almanac.check(pages)
    assert "subframe 5 page 3 missing" in problems
    assert any("more than one almanac" in p for p in problems)


def test_the_trace_holds_a_whole_almanac() -> None:
    pages = almanac.almanac_pages(TRACE.read_bytes())
    assert almanac.check(pages) == []
    assert len(pages) == len(almanac.ORDER) == 34
