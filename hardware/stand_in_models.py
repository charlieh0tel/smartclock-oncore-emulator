"""Stand-in 3D models for parts whose makers' models may not be redistributed.

Writes, in each model's own frame (the one the boards' footprints place,
taken from the maker's model's extents), a STEP file of boxes and faceted
cylinders, by ee_tooling's step_model:

  oncore-emulator.3dshapes/HRO_TYPE-C-31-M-12.step   USB-C receptacle (J3)
  oncore-emulator.3dshapes/Keystone_5019.step        SMT test point (TP2)
  oncore-jig.3dshapes/TE_1-1478035-0.step            right-angle BNC jack (J3, J4)

Run from hardware/: `uv run python stand_in_models.py`.
"""

from pathlib import Path

from ee_tooling.step_model import BLACK, GOLD, NICKEL, TIN, Solid, box, cylinder, write

HERE = Path(__file__).resolve().parent


def usb_c() -> list[Solid]:
    """HRO TYPE-C-31-M-12: x across the mouth, y up from the board, -z back from the face."""
    return [
        box((0.0, 0.0, -7.35), (8.94, 3.21, 0.0), NICKEL),  # shell
        box((1.0, 0.8, -0.2), (7.94, 2.41, 0.01), BLACK),  # mouth
        *(box((x, -1.0, -6.6), (x + 0.6, 0.0, -5.8), NICKEL) for x in (0.3, 8.04)),  # legs
        *(box((x, -1.0, -2.6), (x + 0.6, 0.0, -1.8), NICKEL) for x in (0.0, 8.34)),
    ]


def test_point() -> list[Solid]:
    """Keystone 5019: a strip loop, long side along x, base at z = 0."""
    return [
        *(box((x0, -1.01, 0.0), (x1, 1.01, 0.25), TIN) for x0, x1 in ((-1.91, -0.9), (0.9, 1.91))),
        *(box((x0, -1.01, 0.25), (x1, 1.01, 2.04), TIN) for x0, x1 in ((-1.15, -0.9), (0.9, 1.15))),
        box((-1.15, -1.01, 2.04), (1.15, 1.01, 2.29), TIN),
    ]


def bnc() -> list[Solid]:
    """TE 1-1478035-0: the interface along +z with its tip at the origin, feet at y = -13.1."""
    return [
        box((-5.5, -13.1, -22.4), (5.5, 5.5, -11.0), BLACK),  # body
        cylinder(2, (0.0, 0.0), 4.8, (-11.0, 0.0), NICKEL),  # barrel
        cylinder(2, (0.0, 0.0), 6.05, (-4.4, -3.6), NICKEL),  # bayonet studs' ring
        *(box((x, -17.1, -17.6), (x + 0.95, -13.1, -16.6), TIN) for x in (-5.5, 4.55)),  # posts
        box((-0.4, -17.35, -17.5), (0.4, -13.1, -16.7), GOLD),  # center pin
    ]


MODELS = {
    "oncore-emulator/oncore-emulator.3dshapes/HRO_TYPE-C-31-M-12.step": usb_c,
    "oncore-emulator/oncore-emulator.3dshapes/Keystone_5019.step": test_point,
    "oncore-jig/oncore-jig.3dshapes/TE_1-1478035-0.step": bnc,
}


def main() -> None:
    for path, part in MODELS.items():
        out = HERE / path
        write(out, part())
        print(path)


if __name__ == "__main__":
    main()
