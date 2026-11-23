"""Tests for coverage.py."""

import copy
import json
import unittest
from typing import Any

from coverage import PAGE, SOURCE, problems, render, slug


def table() -> dict[str, Any]:
    data: dict[str, Any] = json.loads(SOURCE.read_text())
    return data


class Coverage(unittest.TestCase):
    def test_the_page_is_current(self) -> None:
        self.assertEqual(PAGE.read_text(), render(table()))

    def test_every_claim_is_recorded(self) -> None:
        self.assertEqual(problems(table()), [])

    def test_a_claim_needs_its_record(self) -> None:
        data = copy.deepcopy(table())
        cell = data["rows"][0]["cells"]["nucleo_image"]
        cell["status"] = "yes"
        self.assertEqual(len(problems(data)), 1)

    def test_a_record_must_exist(self) -> None:
        data = copy.deepcopy(table())
        cell = data["rows"][1]["cells"]["board_image"]
        cell["section"] = "No such heading"
        self.assertIn("no heading", problems(data)[0])

    def test_github_anchors(self) -> None:
        self.assertEqual(
            slug("The Z3801A's 3543 image, from power-on"), "the-z3801as-3543-image-from-power-on"
        )
        self.assertEqual(
            slug("Z3801A and Z3805A on the first board"), "z3801a-and-z3805a-on-the-first-board"
        )
        self.assertEqual(slug("58503B 1.01.04"), "58503b-10104")


if __name__ == "__main__":
    unittest.main()
