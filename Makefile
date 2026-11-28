# Repository chores.  The firmware builds with cargo in firmware/, the
# hardware release with jlcpcb-release (hardware/ee_tooling).

.PHONY: hooks

# Use the repository's git hooks (.githooks/), once per clone.
hooks:
	git config core.hooksPath .githooks

.PHONY: flash flash-rtt flash-ut-plus flash-vp-six flash-z3817a

# The build date from the commit's, so a commit builds the same image
# every time and a board's flash can be checked against it.
export SOURCE_DATE_EPOCH ?= $(shell git log -1 --format=%ct)

FIRMWARE_ELF = target/thumbv6m-none-eabi/release/oncore-fw
FLASH_CHIP = STM32C091FC

# The board on the debug probe: `flash`, the production build, then back
# to the prompt; `flash-rtt`, the logging build, staying attached for its
# log (ctrl-C to leave; the board keeps running).  Each prints the board's
# serial and the build it has first.  Neither is for a board mid-run: flashing restarts it.
flash:
	python3 firmware/tools/board_serial.py
	cd firmware/oncore-fw && cargo build --release --no-default-features \
		&& probe-rs download --chip $(FLASH_CHIP) $(FIRMWARE_ELF) \
		&& probe-rs reset --chip $(FLASH_CHIP)

flash-rtt:
	python3 firmware/tools/board_serial.py
	cd firmware/oncore-fw && cargo run --release

# The other models, each the production build with its feature
# (PLAN.md), as `flash`: `flash-ut-plus`, the UT Plus; `flash-vp-six`,
# the six-channel VP alone; `flash-z3817a`, the Z3817A's.
flash-ut-plus flash-vp-six flash-z3817a: flash-%:
	python3 firmware/tools/board_serial.py
	cd firmware/oncore-fw && cargo build --release --no-default-features -F $* \
		&& probe-rs download --chip $(FLASH_CHIP) $(FIRMWARE_ELF) \
		&& probe-rs reset --chip $(FLASH_CHIP)

.PHONY: images

# The production builds, one per model, as `flash` and `flash-%` build
# them, for CI to keep (`.github/workflows/ci.yml`): each image's ELF (for
# probe-rs) and raw binary from the flash's start (0x08000000), named by
# commit and model, and their SHA256SUMS, in $(IMAGES).  `vp` is the
# default build, no feature.
IMAGES = firmware/oncore-fw/target/images
IMAGE_MODELS = vp ut-plus vp-six z3817a
IMAGE_COMMIT = $(shell git describe --always --dirty --abbrev=12 --exclude='*')
# The pinned toolchain's (firmware/rust-toolchain.toml), as for llvm-objdump
# in stack_frames.py.
OBJCOPY = $(shell cd firmware && rustc --print sysroot)/lib/rustlib/$(shell cd firmware && rustc -vV | sed -n 's/^host: //p')/bin/llvm-objcopy

images:
	mkdir -p $(IMAGES) && rm -f $(IMAGES)/oncore-fw-* $(IMAGES)/SHA256SUMS
	for model in $(IMAGE_MODELS); do \
		features=$$( [ $$model = vp ] || echo $$model ); \
		name=oncore-fw-$(IMAGE_COMMIT)-$$model; \
		(cd firmware/oncore-fw && cargo build --release --no-default-features -F "$$features") || exit 1; \
		cp firmware/oncore-fw/$(FIRMWARE_ELF) $(IMAGES)/$$name.elf || exit 1; \
		$(OBJCOPY) -O binary $(IMAGES)/$$name.elf $(IMAGES)/$$name.bin || exit 1; \
	done
	cd $(IMAGES) && sha256sum *.elf *.bin > SHA256SUMS && cat SHA256SUMS

.PHONY: lint stack

# Before each commit (AGENTS.md): every crate formatted, the workspace's
# and oncore-fw's (its own workspace, which `cargo fmt` in firmware/ does
# not reach), clippy on the host crates and on every firmware build,
# warnings as errors, the Python checks (firmware/tools, bench, hardware;
# hardware's mypy files are its pyproject.toml's), and `stack`.
lint: stack
	cd firmware && cargo fmt --check
	cd firmware/oncore-fw && cargo fmt --check
	@# No wildcard imports, `use super::*` in tests included (AGENTS.md):
	@# clippy's wildcard_imports does not report these.
	@if git grep -nE '^\s*(pub(\([a-z]+\))? )?use [^;]*::\*;' -- 'firmware/*.rs'; then \
		echo "wildcard imports (above)"; exit 1; fi
	cd firmware && cargo clippy --all-targets -- -D warnings
	for features in "" rtt nucleo rtt,nucleo ut-plus rtt,ut-plus vp-six z3817a rtt,z3817a; do \
		(cd firmware/oncore-fw && cargo clippy --release --no-default-features \
			-F "$$features" --bins -- -D warnings) || exit 1; \
	done
	cd firmware/oncore-fw && cargo clippy --release --examples -- -D warnings
	cd firmware/tools && uv run ruff format --check . && uv run ruff check . && uv run mypy .
	cd bench && uv run ruff format --check . && uv run ruff check . && uv run mypy .
	cd hardware && uv run ruff format --check . && uv run ruff check . && uv run mypy

# Every firmware build, with and without rtt, nucleo, ut-plus,
# vp-six and z3817a,
# built and checked as Cargo reports it: fails on a function frame over
# the limit, a stack write it cannot size, or too little room for the
# stack (AGENTS.md, Stack and RAM; firmware/tools/stack_frames.py).
stack:
	cd firmware/tools && uv run python -m unittest -q
	for features in "" rtt nucleo rtt,nucleo ut-plus rtt,ut-plus vp-six z3817a rtt,z3817a; do \
		(cd firmware/tools && uv run python stack_frames.py --build "$$features" --top 3) || exit 1; \
	done
