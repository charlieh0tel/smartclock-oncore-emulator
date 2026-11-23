# The Oncore's ten-pin connector (J1)

What the Motorola drawings give for the Oncore's power and data header,
and how this board's J1 matches it.  Sources: UT+ Oncore engineering
notes rev 1.0, 2000 (`third_party/motorola/ut-engg-notes.pdf`, p.4
mechanical, p.7 pinout, p.10 idle levels) and the VP Oncore engineering
notes archive (`third_party/motorola/vponcoreengnotesarchive.pdf`,
Appendix A, p.9).

## Pinout

As seen by the Oncore, Motorola's names (UT+ p.7, VP Appendix A):

| Pin | Oncore | This board |
|-----|--------|------------|
| 1 | BATT, external backup | not connected |
| 2 | +5 V regulated | +5V |
| 3 | GND | GND |
| 4 | VPP, flash programming | not connected |
| 5 | not used | not connected |
| 6 | 1 PPS output | ONC_1PPS (U3, R3 33 ohm) |
| 7 | 1 PPS return | GND |
| 8 | TTL TXD1, primary output | ONC_TXD (U6, R4 33 ohm; R25 holds it at mark) |
| 9 | TTL RXD1, primary input | ONC_RXD (U7; R11 10k pull-up) |
| 10 | TXD/RXD return | GND |

Pins 8 and 9 follow Motorola; Synergy's notes describe pin 8 the other
way (PLAN.md, open question 3).

## Position

The Oncore's J1 is on the bottom side.  The VP drawing gives its center
at X 72.44, Y 9.45 "nominal to center", measured from the lower-left
mounting hole (the holes are 3.0 mm in from the edges).  On this board,
seen from the top with the origin at the lower-left corner of the
outline:

| | X | Y |
|---|---|---|
| Pin 1 | 74.20 | 7.40 |
| Pin 2 | 76.74 | 7.40 |
| Pin 9 | 74.20 | 17.56 |
| Pin 10 | 76.74 | 17.56 |
| Center | 75.47 | 12.48 |

From the hole that is 72.47, 9.48: within 0.03 mm of the VP figure.
Odd pins are in the column nearer the left edge, pins 1 and 2 nearest
the bottom edge, as in the UT+ bottom view.  Holes are 1.0 mm on 1.7 mm
pads.

## Height

The instrument holds the Oncore on standoffs through its four mounting
holes, so the board's height above the motherboard is fixed, and J1's
pins reach down into the motherboard's socket.

| | Body | Mating pins below the body | Below the board |
|---|---|---|---|
| Oncore: AMP 104326-05 (VP Appendix A) | about 2.36 | 5.84 REF | 8.2 nominal (4.9 x 12.3 mm) |
| This board: Samtec TSW-105-07-G-D | 2.54 | 5.84 | 8.38 |
| LCSC alternative: CJT A2541WV-2x5P (C225520) | about 2.5 | 6.0 | about 8.5 |

The Oncore's figures are nominal and reference dimensions; Motorola
does not tolerance them.  The Samtec's pins go into the socket as far as
the Oncore's, but its body is about 0.18 mm taller.  If the Oncore's
body sat on the socket, J1's body meets the socket 0.18 mm early: put a
washer of about 0.2 mm on each standoff.

The rest of the envelope, for reference: components to 7.18 mm above
the board (UT+; VP 7.35 max), 15.93 +/- 0.5 mm overall (UT+; VP 16.26
max).  This board's tallest top-side part is J3 at about 3.2 mm.

## Parts

- **J1:** Samtec TSW-105-07-G-D, 2 x 5, 2.54 mm, vertical through hole:
  gold on the mating end, tin tails.  DigiKey SAM1028-05-ND.  Its solder
  tails are 2.54 mm, 0.94 mm proud of the 1.6 mm board.
  TSW-105-07-T-D is the same part in tin throughout; match the plating
  of the instrument's socket.
- **Alternative:** CJT A2541WV-2x5P, LCSC C225520, with 6.0 mm mating
  pins.
- J1 is not fitted by the assembler (DNP); it is ordered loose and
  fitted by hand: on the bottom to plug in as the Oncore did, or on top
  for a ribbon jumper down to the motherboard.

## Ribbon jumper

With J1 fitted on top, a ribbon jumper reaches down to the
motherboard's socket.  Seen from above, both ends of a straight-through
ribbon mate the same way, so pin n meets pin n.

The motherboard's socket is female, as the Oncore's J1 was male.  The
simplest jumper is female at both ends, with a double-ended (male to
male) 2 x 5 header plugged into the motherboard's socket as an adapter:

- **Jumper:** a 2 x 5, 2.54 mm IDC socket at each end of 10-way 1.27 mm
  (0.050 in) ribbon: 3M 89110-0101 sockets (Amphenol 71600-110LF is a
  second source) on 3M 3365/10 or similar; or a ready-made
  socket-to-socket assembly (Samtec IDSD-05 series, or a stocked 2 x 5
  female-female IDC cable such as SparkFun's).
- **Adapter:** a 2 x 5, 2.54 mm header with square 0.64 mm posts at
  both ends, each long enough to mate: about 5.84 mm into the
  motherboard's socket, as the Oncore's pins, and about 6 mm into the
  IDC socket.  Not yet chosen; Samtec's configurable headers (the DW
  series sets the body anywhere along the pin) or "dual-length" or
  stacking 2 x 5 headers are the places to look.  Check the post
  section (0.64 mm square) and both lengths on the drawing.

Alternatively, a jumper with a socket at the board end and a male plug
at the motherboard end (Samtec IDMD-05 series, made to order) needs no
adapter; male IDC plugs are harder to find in stock.
