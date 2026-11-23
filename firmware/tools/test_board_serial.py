"""Tests for board_serial.py."""

import unittest

from board_serial import build_identity, flash_bytes, serial, unique_id

IDENTITY = "ONCORE-BUILD 2eca1855abcd-dirty vp,rtt 20261118"


class BoardSerial(unittest.TestCase):
    def test_a_board_s_serial(self) -> None:
        # Read from a board and its `@@Cj`, 2026-11-06.
        unique = unique_id("00220011 47435001 20373257 \n")
        self.assertEqual(serial(unique), "EMU6050839")

    def test_an_address_or_warning_is_not_a_word(self) -> None:
        with self.assertRaises(ValueError):
            unique_id("WARN target voltage 0.00 V\n")
        with self.assertRaises(ValueError):
            unique_id("00220011 47435001\n")

    def test_the_build_identity_is_read_to_its_nul(self) -> None:
        flash = b"\xff\x00code" + IDENTITY.encode() + b"\0\0\0more"
        self.assertEqual(build_identity(flash), IDENTITY)

    def test_an_image_without_one_has_none(self) -> None:
        self.assertIsNone(build_identity(b"\x00\x90\x00\x20" * 8))

    def test_flash_bytes_skip_addresses(self) -> None:
        self.assertEqual(flash_bytes("08000000: 00 90 00 20\n"), b"\x00\x90\x00\x20")


if __name__ == "__main__":
    unittest.main()
