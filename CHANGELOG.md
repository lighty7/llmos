# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Phase 6, storage and installation:
  - virtio-blk driver (legacy PCI interface, DMA, polled) and a `BlockDevice` abstraction,
  - GPT reading and writing with CRC-32, and physically contiguous DMA buffers,
  - `llmfs` persistent files on an llmos data partition, saved after every change,
  - `install [disk]`: copies the boot partition onto an empty disk and adds a data partition,
    always after confirmation,
  - `disks` command, and disks in the hardware inventory and the LLM's machine summary,
  - LLM actions `list_disks` and `install` (agent accuracy 17/20 on `agent_eval`),
  - two-stage automated test: install onto a fresh disk, reboot from it, check the files.
- Phase 7, AArch64 port (`kernel-aarch64/`):
  - self-relocating static-PIE kernel with a Linux arm64 image header (QEMU `virt`,
    Raspberry Pi firmware),
  - EL2→EL1 drop, NEON, exception vectors, identity-mapped MMU with caches,
  - device-tree parser (memory, UART, initrd, CPUs, PSCI, `ranges` translation),
  - PL011 UART driver, a bootstrap allocator followed by the heap, the generic timer,
    PSCI power-off and reset,
  - serial shell running the same `llm` agent, with the model loaded as an initrd,
  - runner commands `cargo run -- arm` (also `--board raspi3b --dtb FILE`) and `arm-test`.

## [0.2.0] - 2026-11-07

The project was rewritten in Rust. The C prototype lives on the `c-initial-idea` branch.

### Added

- Host runner (`cargo run`): builds the kernel, creates a UEFI disk image with the model as a
  RAM disk, boots QEMU with OVMF, and runs an automated boot test (`cargo run -- test`).
- Kernel (`no_std`, custom `x86_64-llmos` target with SSE2):
  - serial (COM1) and framebuffer text console,
  - GDT/TSS with a double-fault stack, IDT with exception handlers,
  - frame allocator, page mapping and a kernel heap,
  - 8259 PIC, PIT timer, PS/2 keyboard driver,
  - hardware inventory: CPUID, ACPI (RSDP/XSDT/MADT), PCI enumeration, JSON export,
  - in-memory file system, shell, `selftest` command.
- `llm` crate: `no_std` Llama-architecture inference with f32/Q8 weights, a byte-level BPE
  tokenizer matching Hugging Face, a KV cache, grammar-constrained JSON action decoding,
  a baked prompt cache (`llm-bake`), and an agent prompt evaluation harness.
- LLM shell agent: natural-language requests become validated actions, with confirmation
  for destructive ones.
- Tools: `export_model.py` (safetensors → `.model`, numpy only) and `make_test_vectors.py`
  (reference outputs from `transformers`).
- Documentation: README, architecture, model format, contributing guide.

### Removed

- The C kernel, the hand-written bootloader and the Makefile.

[Unreleased]: https://github.com/lighty7/llmos/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/lighty7/llmos/releases/tag/v0.2.0
