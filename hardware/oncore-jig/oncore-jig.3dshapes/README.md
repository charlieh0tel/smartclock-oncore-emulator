# 3D models

| File | Part | Source | License |
|------|------|--------|---------|
| TE_1-1478035-0.step | TE Connectivity 1-1478035-0, right-angle PCB BNC jack (J3, J4) | TE's `C-1-1478035-0-B-3D.stp` gave the frame; TE publishes no license for its models (checked 2026-11-01) | this project's: a stand-in of boxes and faceted cylinders, drawn by `hardware/stand_in_models.py` in the maker's model's frame and extents, because the maker's model may not be redistributed |

## Footprints that use them

The footprints copied from KiCad's library, and changed, are adapted
material of that library, so they stay CC-BY-SA-4.0 (KiCad library
license, with its exception for designs that use them), not
CERN-OHL-S-2.0.

In `../oncore-jig.pretty`:

- `BNC_TEConnectivity_1478035_Horizontal` is a copy of KiCad's
  `Connector_Coaxial:BNC_TEConnectivity_1478035_Horizontal` with only the
  model reference changed: KiCad ships no model for it.  TE's model has
  the interface along +Z with its tip at the origin, the board's normal
  along +Y and its standoff feet at Y = -13.1 mm; it is placed at offset
  (0, 16.9, 13.1) mm, rotation (-90, 0, 180), putting the center pin on the
  footprint origin and the feet on the board.
- `SMA_BAT_Wireless_BWSMA-KWE-Z001` is a copy of KiCad's
  `Connector_Coaxial:SMA_BAT_Wireless_BWSMA-KWE-Z001` (J10, J11) with only
  the model reference changed: KiCad ships no model for it, and BAT
  Wireless's is not to be had by script.  It uses KiCad's model for the
  Molex 73251-2200, a right-angle SMA jack on the same 5.1 mm square pin
  pattern with the same origin and mating direction, as a stand-in, with
  no offset or rotation.
- `Sinhoo_SMTSO30110CTJ-B` (H1-H4) is KiCad's
  `Mounting_Wuerth:Mounting_Wuerth_WA-SMSI-M3_H11mm_9774110360` with
  Sinhoo's land pattern (LCSC C2916372 data sheet): a 4.3 mm NPTH
  (4.22 +0.08 mm) and the ring pad to its edge, 7.4 mm across (6.2 mm
  minimum), and a 5.56 mm body on F.Fab.  It keeps Wurth's model, a
  6.0 mm through-bore part, as a stand-in.
- `QFN-24-1EP_4x4mm_P0.5mm_EP2.6x2.6mm_CenterVia` (U2) is KiCad's
  `Package_DFN_QFN:QFN-24-1EP_4x4mm_P0.5mm_EP2.6x2.6mm` with the exposed
  pad's paste as four 0.9 mm squares at +/-0.73 mm (48%), clear of a
  0.5 mm via at its center.  It keeps KiCad's model.
- `PinSocket_2x05_P2.54mm_Vertical_SMD_BOOMELE` (J2) is KiCad's
  `Connector_PinSocket_2.54mm:PinSocket_2x05_P2.54mm_Vertical_SMD` with
  BOOMELE's land pattern for its 2.54-2*5P (LCSC C261072): 0.8 x 2.0 mm
  pads centered 3.0 mm from the row line, a 13.2 mm body.  It keeps
  KiCad's model.
