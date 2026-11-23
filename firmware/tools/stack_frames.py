"""Each function's stack frame in a Thumb firmware image, from its
disassembly, and whether any is over a limit (AGENTS.md, Stack and RAM).

    uv run python stack_frames.py --build FEATURES [--limit BYTES] [--top N]
    uv run python stack_frames.py --elf ELF [--limit BYTES] [--top N]

`--build` builds oncore-fw's release firmware with `FEATURES` (and no
defaults) and checks the executable Cargo reports, so a redirected or
stale target directory cannot stand in for it.  Each check also fails
if the image's room for the stack, from its statics' end to the stack's
top, is under `STACK_RESERVE`, or if it is not a 32-bit ARM image.

A frame is what a function's prologue takes: the registers its
`push`es save (Thumb-1 saves r8 to r11 with a second), its
`sub sp, #imm`, and its `add sp, rN` with rN a negative literal from the
pool, as Thumb-1 code takes a frame over 508 bytes.  Each write to sp
is sized only in LLVM's own forms, read from the instructions just
before it; any other (a realignment, `sub sp, rN`, `msr msp`) is one
the checker cannot size: it is reported and fails the check, as does a
disassembly with no functions.  The exception is `HANDOVERS`, whose
`msr msp` gives the processor to other code with its own stack, never
to return: the ROM bootloader's jump.
Calls are not followed: a frame is one function's, not the worst case
down a call chain; the stack's peak is measured on a board (PLAN.md).
The disassembler is the pinned toolchain's own `llvm-objdump`
(`llvm-tools`, firmware/rust-toolchain.toml).
"""

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

# A function's first line: `08000230 <name>:`.
FUNCTION = re.compile(r"^[0-9a-f]+ <(?P<name>.*)>:$")
# A literal-pool word: ` 80000e0: 88 2c 00 20  \t.word\t0x20002c88`.
WORD = re.compile(r"^\s*(?P<address>[0-9a-f]+):.*\s\.word\s+0x(?P<value>[0-9a-f]+)")
PUSH = re.compile(r"\spush\s+\{(?P<registers>[^}]*)\}")
# An instruction writing sp: its mnemonic and the operands after sp.
WRITES_SP = re.compile(r"\t(?P<op>[a-z.]+)\tsp,\s*(?P<operands>.*)$")
# A write to a stack pointer through the special registers.
WRITES_STACK_REGISTER = re.compile(r"\tmsr\t(?:msp|psp),")
# Functions whose write to msp hands the processor over for good, with
# the stack the code taking it names (oncore-fw/src/bootloader.rs).
HANDOVERS = frozenset({"cortex_m::asm::bootload", "cortex_m::asm::bootstrap"})
IMMEDIATE = re.compile(r"^#(?P<value>0x[0-9a-f]+|\d+)$")
REGISTER = re.compile(r"^(?P<register>r\d+)$")
# `ldr r3, [pc, #0x3b8]  @ 0x8003da0 <...>`.
LOAD_LITERAL = re.compile(
    r"\sldr\s+(?P<register>r\d+),\s*\[pc, #\w+\]\s*@\s*0x(?P<address>[0-9a-f]+)"
)
# An epilogue's steps back from the frame pointer r7 to the saved
# registers: `subs r6, r7, #0x7`, then perhaps `subs r6, #0x5`.
FROM_FRAME_POINTER = re.compile(r"\tsubs\t(?P<register>r\d+), r7, #\w+$")
FROM_FRAME_POINTER_MORE = re.compile(r"\tsubs\t(?P<register>r\d+), #\w+$")

# Bytes a pushed register takes.
REGISTER_BYTES = 4
# Words at or above this are negative, two's complement.
NEGATIVE = 1 << 31
WORD_RANGE = 1 << 32
# The limit by default, bytes: AGENTS.md's.
DEFAULT_LIMIT = 1536
DEFAULT_TOP = 10
# The room for the stack every build must leave, bytes: the deepest chain
# traced in the production disassembly (2026-10-31), about 3.5 KB with
# an interrupt on top, 3.75 KB with RTT's logging, and margin for panic
# formatting and the paths not traced.
STACK_RESERVE = 6144
# The disassembler's name for a 32-bit ARM image.
ARM = "file format elf32-littlearm"
# oncore-fw, beside this directory.
FIRMWARE = Path(__file__).resolve().parent.parent / "oncore-fw"


@dataclass(frozen=True)
class Frame:
    """One function's stack frame, bytes."""

    function: str
    bytes: int


@dataclass
class Scan:
    """What a disassembly gave: each function's frame, largest first, and
    each write to sp the checker could not size, as `function: line`."""

    frames: list[Frame] = field(default_factory=list)
    unsized: list[str] = field(default_factory=list)


def register_count(registers: str) -> int:
    """How many registers a push's list names: `r4, r5, r7, lr` or `r4-r7, lr`."""
    count = 0
    for item in registers.split(","):
        first, _, last = item.strip().partition("-")
        count += int(last[1:]) - int(first[1:]) + 1 if last else 1
    return count


def frames(disassembly: str) -> Scan:
    """Each function's frame in `llvm-objdump -d` output.  A write to sp
    is sized only in LLVM's own forms, each from the instructions just
    before it, so nothing between can have changed the register."""
    lines = disassembly.splitlines()
    words = {}
    for line in lines:
        if match := WORD.match(line):
            words[int(match["address"], 16)] = int(match["value"], 16)
    found: dict[str, int] = {}
    unsized: list[str] = []
    function = None
    # This function's instructions so far, the latest last.
    before: list[str] = []
    for line in lines:
        if match := FUNCTION.match(line):
            function, before = match["name"], []
            found.setdefault(function, 0)
            continue
        if function is None:
            continue
        if match := PUSH.search(line):
            found[function] += REGISTER_BYTES * register_count(match["registers"])
        elif WRITES_STACK_REGISTER.search(line):
            if function not in HANDOVERS:
                unsized.append(f"{function}: {line.strip()}")
        elif match := WRITES_SP.search(line):
            taken = sp_bytes(match["op"], match["operands"], before, words)
            if taken is None:
                unsized.append(f"{function}: {line.strip()}")
            else:
                found[function] += taken
        before.append(line)
    return Scan(
        sorted(
            (Frame(name, size) for name, size in found.items()),
            key=lambda frame: frame.bytes,
            reverse=True,
        ),
        unsized,
    )


def sp_bytes(op: str, operands: str, before: list[str], words: dict[int, int]) -> int | None:
    """What a write to sp takes for the frame, bytes, or None if it is not
    one of LLVM's forms:

    - `sub sp, #imm` takes `imm`; `add sp, #imm` frees.
    - `ldr rN, [pc, ...]` then `add sp, rN`: a negative literal takes,
      a positive one frees.
    - `subs rN, r7, #imm` (then perhaps `subs rN, #imm`) then
      `mov sp, rN`: an epilogue, freeing.
    """
    immediate = IMMEDIATE.match(operands)
    register = REGISTER.match(operands)
    if immediate:
        return {"sub": int(immediate["value"], 0), "add": 0}.get(op)
    if not register or not before:
        return None
    name = register["register"]
    if op == "add":
        load = LOAD_LITERAL.search(before[-1])
        if load and load["register"] == name and int(load["address"], 16) in words:
            value = words[int(load["address"], 16)]
            return WORD_RANGE - value if value >= NEGATIVE else 0
        return None
    if op == "mov":
        steps = before[-2:] if FROM_FRAME_POINTER_MORE.search(before[-1]) else before[-1:]
        first = FROM_FRAME_POINTER.search(steps[0])
        more = [FROM_FRAME_POINTER_MORE.search(step) for step in steps[1:]]
        if first and first["register"] == name and all(m and m["register"] == name for m in more):
            return 0
    return None


def run(command: list[str], cwd: Path | None = None) -> str:
    """`command`'s standard output; its standard error, and an exit, if it
    fails."""
    done = subprocess.run(command, cwd=cwd, capture_output=True, text=True)
    if done.returncode != 0:
        sys.stderr.write(done.stderr)
        raise SystemExit(f"could not check: {command[0]} failed")
    return done.stdout


def llvm_tool(name: str) -> Path:
    """The pinned toolchain's `name` (`llvm-objdump`, `llvm-nm`): in its
    sysroot, under the host."""
    sysroot = run(["rustc", "--print", "sysroot"]).strip()
    host = next(
        line.removeprefix("host: ")
        for line in run(["rustc", "-vV"]).splitlines()
        if line.startswith("host: ")
    )
    return Path(sysroot, "lib", "rustlib", host, "bin", name)


def disassemble(elf: Path) -> str:
    """`elf`'s disassembly, demangled."""
    return run([str(llvm_tool("llvm-objdump")), "-d", "--no-show-raw-insn", "-C", str(elf)])


def stack_room(symbols: str) -> int | None:
    """Bytes from the statics' end, `_stack_end`, to the stack's top,
    `_stack_start` (cortex-m-rt's link.x), in `llvm-nm` output, or None
    without them."""
    found = {}
    for line in symbols.splitlines():
        match line.split():
            case [value, _, ("_stack_start" | "_stack_end") as name]:
                found[name] = int(value, 16)
    if len(found) < 2:
        return None
    return found["_stack_start"] - found["_stack_end"]


def build(features: str) -> Path:
    """oncore-fw's release executable with `features`, as Cargo reports it."""
    output = run(
        [
            "cargo",
            "build",
            "--release",
            "--no-default-features",
            "--features",
            features,
            "--message-format=json",
            "--bin",
            "oncore-fw",
        ],
        # its .cargo/config.toml sets the target
        cwd=FIRMWARE,
    )
    for line in output.splitlines():
        message = json.loads(line)
        if (
            message.get("reason") == "compiler-artifact"
            and message["target"]["name"] == "oncore-fw"
            and "bin" in message["target"]["kind"]
            and message.get("executable")
        ):
            return Path(message["executable"])
    raise SystemExit(f"could not check: cargo reported no executable for {features}")


def check(elf: Path, limit: int, top: int) -> bool:
    """Prints `elf`'s largest frames and its room for the stack: whether
    it passes."""
    disassembly = disassemble(elf)
    if ARM not in disassembly:
        print(f"could not check: {elf} is not a 32-bit ARM image", file=sys.stderr)
        return False
    scan = frames(disassembly)
    for frame in scan.frames[:top]:
        print(f"{frame.bytes:6d} {frame.function}")
    over = [frame for frame in scan.frames if frame.bytes > limit]
    for frame in over:
        print(f"over {limit} bytes: {frame.bytes} {frame.function}", file=sys.stderr)
    for line in scan.unsized:
        print(f"could not size: {line}", file=sys.stderr)
    if not scan.frames:
        print(f"could not check: no functions in {elf}", file=sys.stderr)
    room = stack_room(run([str(llvm_tool("llvm-nm")), str(elf)]))
    if room is None:
        print(f"could not check: no _stack_start or _stack_end in {elf}", file=sys.stderr)
    elif room < STACK_RESERVE:
        print(f"room for the stack {room} bytes, under {STACK_RESERVE}", file=sys.stderr)
    else:
        print(f"room for the stack: {room} bytes")
    return (
        not over
        and not scan.unsized
        and bool(scan.frames)
        and room is not None
        and room >= STACK_RESERVE
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    image = parser.add_mutually_exclusive_group(required=True)
    image.add_argument("--build", metavar="FEATURES")
    image.add_argument("--elf", type=Path)
    parser.add_argument("--limit", type=int, default=DEFAULT_LIMIT)
    parser.add_argument("--top", type=int, default=DEFAULT_TOP)
    args = parser.parse_args()
    # No features is a build too: --build "".
    elf = build(args.build) if args.build is not None else args.elf
    print(f"{args.build if args.build is not None else elf}:", flush=True)
    return 0 if check(elf, args.limit, args.top) else 1


if __name__ == "__main__":
    sys.exit(main())
