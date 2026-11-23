# LEA-M8T version

U1 is a LEA-M8T-0 (TIM 1.10), JLCPCB C2763452.

| Type number | Firmware | Protocol |
|---|---|---|
| LEA-M8T-0-10 | FW 3.01, TIM 1.10 | 22.00 |
| LEA-M8T-1-00 | FW 3.01, TIM 1.11 | 22.01 |
| LEA-M8T-1-01 | FW 3.01, TIM 1.12 | 22.01 |

Sources: data sheet UBX-15025193 R10, p. 2; receiver description
UBX-13003221 R28, p. 8 and its product table before p. 1; PCN
UBX-23001089 (`third_party/u-blox/`).

- One data sheet and integration manual cover all three: same pinout,
  footprint and electrical figures.
- TIM 1.12 fixes a 1 PPS jump of 500 ns below -30 C in TIM 1.11
  (PCN UBX-23001089).  The PCN does not cover TIM 1.10.
- Protocol 22.01 adds UBX-NAV-NMI.  The firmware does not use it.
- The receiver description lists UBX-TIM-TP and the CFG-TP5 poll for
  protocol 22, not 22.01.  The firmware uses TIM-TP's qErr.  On a -1,
  poll UBX-TIM-TP before relying on it.
- CFG-TP5 and CFG-ANT defaults are the same on TIM 1.10 and 1.1x.

JLCPCB had 8 of C2763452 on 2026-10-25; the listing gives no -10
suffix.  A -1 comes from Mouser or DigiKey, consigned to JLCPCB.
