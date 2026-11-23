"""Tests for stack_frames.py, on disassembly cut from the firmware."""

import unittest

from stack_frames import Frame, frames, register_count, stack_room

# A large frame taken through the literal pool, as `oncore_app::run`'s
# was.
LARGE = """
08001c78 <run>:
 8001c78:      \tpush\t{r4, r5, r6, r7, lr}
 8001c7a:      \tadd\tr7, sp, #0xc
 8001c7c:      \tldr\tr6, [pc, #0x228]        @ 0x8001ea8 <run+0x230>
 8001c7e:      \tadd\tsp, r6
 8001c80:      \tbx\tlr
 8001ea8: c4 d6 ff ff  \t.word\t0xffffd6c4
"""

# A small frame in two steps, and an `add sp, rN` whose register holds
# no literal.
SMALL = """
08000300 <tick>:
 8000300:      \tpush\t{r4-r7, lr}
 8000302:      \tsub\tsp, #0x1fc
 8000304:      \tsub\tsp, #0x64
 8000306:      \tadd\tsp, r3
 8000308:      \tpop\t{r4-r7, pc}
"""


# An epilogue restoring sp from the frame pointer, and a second push
# saving r8 to r11 through low registers.
EPILOGUE = """
08000400 <apply>:
 8000400:      \tpush\t{r4, r5, r6, r7, lr}
 8000402:      \tadd\tr7, sp, #0xc
 8000404:      \tmov\tr4, r8
 8000406:      \tpush\t{r4}
 8000408:      \tsub\tsp, #0x10
 800040a:      \tsubs\tr6, r7, #0x7
 800040c:      \tsubs\tr6, #0x5
 800040e:      \tmov\tsp, r6
 8000410:      \tpop\t{r4, r5, r6, r7, pc}
"""

# Realigning sp, as LLVM does for an over-aligned local: not sizable.
REALIGN = """
08000500 <aligned>:
 8000500:      \tpush\t{r4, r7, lr}
 8000502:      \tmov\tr4, sp
 8000504:      \tlsrs\tr4, r4, #0x9
 8000506:      \tlsls\tr4, r4, #0x9
 8000508:      \tmov\tsp, r4
"""


class StackFramesTest(unittest.TestCase):
    def test_registers_are_counted_in_lists_and_ranges(self) -> None:
        self.assertEqual(register_count("r4, r5, r7, lr"), 4)
        self.assertEqual(register_count("r4-r7, lr"), 5)

    def test_a_frame_through_the_literal_pool_is_found(self) -> None:
        # 0x100000000 - 0xffffd6c4 is 10556, and five registers
        self.assertEqual(frames(LARGE).frames, [Frame("run", 10556 + 20)])

    def test_frames_are_summed_and_sorted(self) -> None:
        scan = frames(LARGE + SMALL)
        self.assertEqual(scan.frames, [Frame("run", 10556 + 20), Frame("tick", 508 + 100 + 20)])

    def test_every_push_counts_and_an_epilogue_takes_nothing(self) -> None:
        scan = frames(EPILOGUE)
        self.assertEqual((scan.frames, scan.unsized), ([Frame("apply", 20 + 4 + 16)], []))

    def test_an_add_sp_of_an_unknown_register_is_not_sized(self) -> None:
        self.assertEqual(frames(SMALL).unsized, ["tick: 8000306:      \tadd\tsp, r3"])

    def test_a_realignment_is_not_sized(self) -> None:
        self.assertEqual(frames(REALIGN).unsized, ["aligned: 8000508:      \tmov\tsp, r4"])

    def test_a_frame_pointer_step_changed_before_mov_sp_is_not_sized(self) -> None:
        shifted = EPILOGUE.replace(
            " 800040e:      \tmov\tsp, r6",
            " 800040d:      \tlsrs\tr6, r6, #0xc\n 800040e:      \tmov\tsp, r6",
        )
        self.assertEqual(frames(shifted).unsized, ["apply: 800040e:      \tmov\tsp, r6"])

    def test_a_literal_changed_before_add_sp_is_not_sized(self) -> None:
        shifted = LARGE.replace(
            " 8001c7e:      \tadd\tsp, r6",
            " 8001c7d:      \tlsls\tr6, r6, #0x4\n 8001c7e:      \tadd\tsp, r6",
        )
        self.assertEqual(frames(shifted).unsized, ["run: 8001c7e:      \tadd\tsp, r6"])

    def test_a_write_to_msp_is_not_sized(self) -> None:
        msp = SMALL.replace("\tadd\tsp, r3", "\tmsr\tmsp, r0")
        self.assertEqual(frames(msp).unsized, ["tick: 8000306:      \tmsr\tmsp, r0"])

    def test_the_bootloaders_handover_is_not_a_frame(self) -> None:
        msp = SMALL.replace("\tadd\tsp, r3", "\tmsr\tmsp, r0").replace(
            "<tick>", "<cortex_m::asm::bootload>"
        )
        self.assertEqual(frames(msp).unsized, [])

    def test_the_room_for_the_stack_is_from_the_statics_to_its_top(self) -> None:
        symbols = "20003f90 B _stack_end\n20007800 A _stack_start\n200000c0 B __sbss\n"
        self.assertEqual(stack_room(symbols), 0x20007800 - 0x20003F90)
        self.assertIsNone(stack_room("20007800 A _stack_start\n"))


if __name__ == "__main__":
    unittest.main()
