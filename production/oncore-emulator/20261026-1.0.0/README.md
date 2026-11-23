# Release 1.0.0, ordered 2026-10-26

**Do not re-order this BOM as it is.**  It lists U2 as the
STM32C011F6P6 (LCSC C5456198).  Before DFM the order's U2 was swapped
for an STM32C091FCP6 (C45338507) from our own JLCPCB stock: the firmware
needs its 256 KB of flash and 36 KB of RAM.  The design files will carry
the change in the next release (`TODO.md`, "Board update").

The repository's history was rewritten on 2026-10-28 to remove
attribution lines from commit messages; commit IDs changed, not the
files.  The source archive's `COMMIT` file and the release commit's
message name `d32d5b8`, now `34dc941` (tag
`oncore-emulator-20261026-1.0.0`); the release commit, `06002a2`, is
now `aace058`.
