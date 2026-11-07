# llmos

**An operating system kernel, written in Rust, whose shell is a language model.**

llmos boots on bare metal through UEFI, works out what hardware the machine has, loads a
small LLM from the boot disk and lets you operate the computer in plain English. The model
does not run code or touch hardware: it picks one action from a fixed, validated list, and
anything destructive needs your confirmation.

```text
llmos> install llmos onto the empty disk
{"action": "install"}
llm -> install llmos onto vd1 (512 MiB): EFI system partition 174 MiB (boot loader, kernel, model) + data partition 64 MiB
Allow this? [y/N] y
llmos is installed on vd1. Files are now saved there; remove the boot media and reboot to start from it.
llmos> create a file called hi.txt that says hello there
{"action": "write_file", "path": "hi.txt", "content": "hello there"}
llm -> write 11 bytes to hi.txt
wrote 11 bytes to hi.txt
llmos> cat hi.txt
hello there
```

> **Status:** experimental. All phases of the [roadmap](#roadmap) so far work in QEMU. On
> x86-64 (UEFI), llmos can install itself onto an empty disk and keeps your files there. The
> AArch64 port boots on QEMU `virt` and on QEMU's Raspberry Pi 3 model and runs the same
> LLM. Physical hardware has not been tested yet.

## Features

| Area | What exists |
|------|-------------|
| Boot | UEFI via the [`bootloader`](https://github.com/rust-osdev/bootloader) crate. The model ships as a RAM disk. |
| CPU | GDT/TSS (double faults get their own stack), IDT with exception handlers, SSE2 enabled for the inference maths |
| Memory | Page-table helpers, a physical frame allocator built from the UEFI memory map, a kernel heap (`alloc`) |
| Devices | 8259 PIC, 100 Hz PIT timer, PS/2 keyboard (interrupt driven), COM1 serial port, framebuffer text console |
| Inventory | CPUID, ACPI (RSDP/XSDT/MADT), PCI bus scan, disks, all exported as JSON (`hw`) |
| Storage | virtio-blk driver (DMA, polled), GPT read/write with CRC-32, `llmfs` persistent files on a data partition |
| Install | `install` copies the boot partition (bootloader, kernel, model) onto an empty disk and adds a data partition |
| LLM | `no_std` Llama-architecture inference: f32 or int8 (Q8) weights, BPE tokenizer, KV cache, baked prompt cache |
| Shell | Built-in commands plus natural-language requests through **grammar-constrained decoding** to validated actions |
| AArch64 | Self-relocating Linux-style image, EL2→EL1, MMU, device tree, PL011, model as initrd ([docs/aarch64.md](docs/aarch64.md)) |
| Testing | Host unit tests, comparison against Hugging Face `transformers`, automated QEMU tests (x86-64 and AArch64) |

## Quick start

### 1. Install the prerequisites

```bash
# Rust: the pinned nightly is installed automatically from rust-toolchain.toml
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# QEMU and UEFI firmware (Ubuntu/Debian; use your distribution's equivalents)
sudo apt install qemu-system-x86 ovmf python3-numpy   # + qemu-system-arm for the ARM port
```

### 2. Boot without a model (built-in commands only)

```bash
cargo run -- run --no-model
```

A QEMU window opens. The same console is mirrored on your terminal over the serial port.
Type `help` in either.

### 3. Add the language model

```bash
# Download SmolLM2-135M-Instruct (Apache-2.0, ~270 MB) and convert it (~140 MB, int8)
python3 tools/export_model.py --download HuggingFaceTB/SmolLM2-135M-Instruct
# Optional but strongly recommended: pre-compute the fixed part of the prompt
cargo run --release -p llm --bin llm-bake
# Boot with the model (models/llmos.model is picked up automatically)
cargo run
```

On Linux with KVM, inference runs at near-native speed (around 20 tokens/s on one core).
Without KVM, QEMU emulates the CPU in software at roughly 1 token/s, so each request takes
tens of seconds. See [models/README.md](models/README.md) for details.

### Runner commands

| Command | Purpose |
|---------|---------|
| `cargo run` | Build the kernel and disk image, then boot in QEMU with an empty second disk (`target/disk.img`) |
| `cargo run -- run --installed` | Boot from the disk llmos was installed onto |
| `cargo run -- run --headless` | Boot with no window (serial console only) |
| `cargo run -- image` | Only build `target/llmos-uefi.img` |
| `cargo run -- test` | Automated two-stage test: install onto a fresh disk, then boot from it and check persistence |
| `cargo run -- arm` / `arm-test` | Build the AArch64 kernel and boot it on QEMU `virt` (`--board raspi3b --dtb FILE` for the Pi 3 model) / test it |
| `cargo test --release -p llm` | Inference-engine tests (the reference tests need the model files) |
| `cargo run --release -p llm --example agent_eval` | Measure how accurately the prompt maps requests to actions |

Options: `--release`, `--model PATH`, `--no-model`, `--memory MIB`, `--no-disk`, `--installed`.

## Shell

| Command | Description |
|---------|-------------|
| `help` | List the commands |
| `ls`, `cat <f>`, `write <f> <text>`, `rm <f>` | Files (saved to disk once llmos is installed) |
| `disks` | Disks, partitions and where files are stored |
| `install [disk]` | Install llmos onto an empty disk. Shows the plan and asks first. |
| `info` / `hw` | Hardware summary / full inventory as JSON |
| `mem`, `uptime` | Memory usage, time since boot |
| `model` | Model details and speed of the last request |
| `ask <request>` | Send a request to the LLM. Any unknown input is sent there too. |
| `selftest` | In-kernel checks of interrupts, the heap, devices and action validation |
| `clear`, `reboot`, `poweroff` | Clear the screen, restart, power off |

## How the LLM shell works

```text
 "make a note that says buy milk"
        │
        ▼
 prompt = instructions + examples + hardware summary + request      (ChatML)
        │                      └── baked KV cache ──┘ └ processed once per boot
        ▼
 constrained decoder:  {"action": "▢", "path": "▢", "content": "▢"}
        │   the model only fills ▢, and only with tokens valid at that point
        ▼
 Action::from_json  →  validation (known action, legal file name, ...)
        │
        ▼
 confirmation for destructive actions  →  execute
```

The kernel never runs model output as code. The model can only choose from the
actions in `llm::agent::SHELL_ACTIONS`. See [docs/architecture.md](docs/architecture.md).

## Repository layout

```text
llmos/
├── Cargo.toml, src/main.rs   Runner: builds the kernel, creates the UEFI image, starts QEMU
├── kernel/                   The kernel (no_std, custom x86_64-llmos target)
│   └── src/
│       ├── main.rs           Boot sequence
│       ├── serial.rs, framebuffer.rs, console.rs, keyboard.rs   I/O
│       ├── gdt.rs, interrupts.rs, pic.rs, time.rs, cpu.rs       CPU and interrupts
│       ├── memory.rs, allocator.rs                              Paging, frames, heap
│       ├── inventory/        CPUID, ACPI, PCI
│       ├── storage/          virtio-blk, GPT, llmfs, installer
│       ├── fs.rs, actions.rs, shell.rs, agent.rs, selftest.rs
├── kernel-aarch64/            The AArch64 kernel (boot.rs, dtb.rs, mmu.rs, uart.rs, shell.rs)
├── llm/                      Inference engine (no_std, host-testable)
│   ├── src/                  format, tokenizer, model, constrained, agent, prompt_cache
│   ├── src/bin/llm-bake.rs   Bakes the prompt cache into a model file
│   ├── examples/agent_eval.rs
│   └── tests/reference.rs    Comparison against Hugging Face transformers
├── tools/                    export_model.py, make_test_vectors.py
├── models/                   Model files go here (git-ignored)
└── docs/                     architecture.md, model-format.md, aarch64.md
```

## Roadmap

1. ✅ First boot: UEFI image, serial and framebuffer output
2. ✅ Kernel core: GDT/IDT, exceptions, PIC/PIT, keyboard, paging, heap
3. ✅ Hardware inventory: ACPI, PCI, CPUID → JSON
4. ✅ Inference: `no_std` transformer, Q8 weights, RAM-disk model, prompt cache
5. ✅ LLM shell: natural language → constrained JSON → validated actions
6. ✅ Storage and install: virtio block driver, GPT, persistent files, installing llmos to a disk
7. ✅ AArch64 port: QEMU `virt` and the Raspberry Pi 3 model, sharing the LLM core
8. ⏳ Next: physical hardware, AHCI/NVMe drivers, ARM storage and interrupts, RISC-V, SMP,
   remote management of microcontrollers

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## License

MIT, see [LICENSE](LICENSE). Model weights you download keep their own licence.
SmolLM2 is released under Apache-2.0.

The original C prototype is preserved on the `c-initial-idea` branch.
