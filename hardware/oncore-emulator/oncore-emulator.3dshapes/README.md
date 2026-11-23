# 3D models

| File | Part | Source | License |
|------|------|--------|---------|
| ublox_LEA.step | u-blox LEA-M8T (U1) | `POS/LEA.STEP` from https://github.com/u-blox/3D-Step-Models-Library ("Generic LEA modules") | u-blox grants use, copy, modify and distribute for any purpose without fee; provided as is |
| HRO_TYPE-C-31-M-12.step | HRO TYPE-C-31-M-12 (J3) | `HRO  TYPE-C-31-M-12.step` from https://github.com/ai03-2725/Type-C.pretty gave the frame; that repository states no license (checked 2026-11-01) | this project's: a stand-in of boxes and faceted cylinders, drawn by `hardware/stand_in_models.py` in the maker's model's frame and extents, because the maker's model may not be redistributed |
| MCX_Jack_Horizontal_THT_5.08mm.wrl | right-angle THT MCX jack (J2), BAT Wireless BWMCX-KWE | drawn for this project from the BWMCX-KWE and HJ Tech HJ-MCX016 drawings (`third_party/`): 6 mm cube body 5.7 mm high, collar dia 5.3 x 0.8, interface dia 4.5 x 2.7 on an axis 2.73 mm up | this project's |
| J_Wurth_WR-MCX_60612202111309.step | Würth 60612202111309 (no longer used) | Würth Elektronik KiCad library `KiCad_WR-MCX (rev26a).zip` | CC BY-SA 4.0, Würth Elektronik eiSos GmbH & Co. KG, with Würth's exception for designs that use the parts |
| Keystone_5019.step | Keystone 5019 SMT test point (TP2) | Keystone's `Test_Point_Keystone_5019_SM.STEP` gave the frame; Keystone states no license with its models (checked 2026-11-01) | this project's: a stand-in of boxes and faceted cylinders, drawn by `hardware/stand_in_models.py` in the maker's model's frame and extents, because the maker's model may not be redistributed |

## Footprints that use them

The footprints copied from KiCad's library, and changed, are adapted
material of that library, so they stay CC-BY-SA-4.0 (KiCad library
license, with its exception for designs that use them), not
CERN-OHL-S-2.0.  So do the Würth footprint and symbol.

In `../oncore-emulator.pretty`:

- `ublox_LEA` is a copy of KiCad's `RF_GPS:ublox_LEA` with the model
  reference changed, the model turned 180 degrees so its pin-1 dot sits
  over pad 1, and u-blox's T-shaped paste (integration manual Figure 6)
  as paste-only pads in place of the copper pads' paste.
- `USB_C_Receptacle_HRO_TYPE-C-31-M-12` is a copy of KiCad's
  `Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12` with only the
  model reference changed.  The HRO model is placed at offset
  (-4.47, -3.65, 0) mm, rotation (90, 180, 180): the source repository's
  settings, moved 3.65 mm because KiCad's footprint origin sits that much
  further toward the board edge than the source footprint's.
- `MCX_Jack_Horizontal_THT_5.08mm` is drawn for this project: HJ Tech's
  drill pattern, with 1.5 mm ground holes for the BWMCX-KWE's 0.9 mm
  square posts (1.27 mm across the corners, +/-0.1 mm; IPC's 0.2 to
  0.25 mm over the lead) and 2.2 mm pads; the body and interface on Fab, and the interface on silk
  with an arrow to the mating face, so its direction shows on the board.
  The interface points -X from the center pin.  KiCad reads only a plain
  VRML subset, so its model is written as meshes, in 0.1 in units.
- `J_Wurth_WR-MCX_60612202111309` (no longer used; J2 was the Würth edge
  launch) and the `60612202111309` symbol in
  `../oncore-emulator.kicad_sym` come from the same Würth library (CC BY-SA
  4.0).  Changes to the footprint: each ground land, drawn by Würth as a
  net-less copper polygon over a smaller pad, is now a custom pad with that
  polygon as its shape, so DRC sees one GND land; the Edge.Cuts line across
  the mouth of the notch is removed, since the board outline leaves that gap;
  the model path points here.  The symbol's footprint field points here.
- `D_0402_1005Metric` is a copy of KiCad's `Diode_SMD:D_0402_1005Metric`
  with only the model reference changed: KiCad ships no 0402 diode model,
  so it uses KiCad's `R_0402_1005Metric`, the same size.  No longer used:
  D7 is now KiCad's `Diode_SMD:D_SOD-882`.
- `TestPoint_Keystone_5019_Miniature` is a copy of KiCad's footprint of the
  same name with only the model reference changed: KiCad ships no 5019
  model.  Keystone's model is centered with its base at z = 0 and its long
  side along x, as the footprint is, so it needs no offset or rotation.
