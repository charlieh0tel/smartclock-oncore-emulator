---
name: jlcpcb-release
description: Build the JLCPCB release package for a board, the emulator or the test jig (Gerber/drill zip, BOM, placement, source tgz) under production/BOARD/YYYYMMDD-REV/, with each board's fab settings. Use when the user asks to release, package, or order the board from JLCPCB.
---

# JLCPCB release

Read `hardware/ee_tooling/skills/jlcpcb-release/SKILL.md` first: it
says how `jlcpcb-release` works, what it writes and how to fix
rotations.  This file adds the boards' specifics.  From `hardware/`:

```
uv run jlcpcb-release              # the emulator
uv run jlcpcb-release oncore-jig   # the test jig
```

`jlcpcb.toml` gives each board's directory, copper layers, production
directory and source files: each board's in `production/BOARD/`
(`oncore-emulator/`, `oncore-jig/`).

## Fab settings (PLAN.md's ordering and assembly notes)

The emulator:

- 4 layers, 1.6 mm, JLC04161H-7628 stackup, impedance control.
- ENIG.
- Assembly: top side SMT; J2 is through-hole on the bottom: confirm
  JLCPCB's bottom-side through-hole assembly.  JLCPCB has no model for
  J2, so the preview can't show it; put this in the order's remark
  (the assembly step's notes if it has a field, else the PCB remark):

  > J2 (C5137196) is mounted on the BOTTOM side, through-hole. The MCX
  > connector's mating face points outward, off the board's left edge,
  > as the bottom silkscreen outline and arrow show.
  >
  > J2（C5137196）安装在底面（BOTTOM 面），插件焊接。MCX 连接器的插口朝外，伸出板子左边缘，方向见底面丝印轮廓和箭头。

- 150 um stencil (u-blox's T-shaped paste for U1).
- No rail or V-score on the left edge: J2 and J3 overhang it.
- U1 is C2763452: check JLCPCB has enough.
- In the BOM preview, compare each matched part's MPN and package with
  the BOM's.

The test jig:

- 2 layers, 1.6 mm, HASL or ENIG; top side SMT only (the through-hole
  parts are DNP, fitted by hand).
- One design: the jig and its driver are joined by three mouse-bite
  tabs across a 2 mm slot.  JLCPCB may count two designs: say in the
  order's remark that it is one board with a breakaway, and check the
  DFM review before paying.
- In the BOM preview, compare each matched part's MPN and package with
  the BOM's.

## Rotations and offsets

The corrections are in `hardware/ee_tooling/src/ee_tooling/parts.toml`.
A screenshot of the emulator's preview can be measured: the four
mounting holes are 76.55 x 44.80 mm apart.

Found on the first order (2026-10-26): -90 for C2763452 (LEA-M8T),
C5456198 (STM32C011), C51118 (AP2112K), C7519 (USBLC6-2); 180 for C163712
(SN74AHCT1G126), C840096 (SN74LVC1G34), C82045 (BSS138); J3 (C165948)
moved 1.5 mm back from its face (JLCPCB's model sat that far out).
Passives, LEDs and SOD diodes needed none.  U2 is now the STM32C091FCP6
(C45338507), whose correction is not yet known: it is in `parts.toml`'s
`[unchecked]`, and the tool says so; check it in the preview and move it
to `[rotation]`.

## Confirmed and not yet confirmed

Confirmed in JLCPCB's preview (2026-10-26, release 1.0.0): KiCad's
absolute coordinates (Y negative) line up with the Gerbers, and with the
corrections above every pin-1 dot sits at our pin 1 and J3's face
overhangs the edge by about 0.5 mm.

Not yet confirmed: J2's bottom-side through-hole placement (JLCPCB has no
model for it, so it goes by the order remark and the silk), until a
board comes back.
