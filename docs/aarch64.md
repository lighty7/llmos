# llmos on AArch64 (64-bit ARM)

`kernel-aarch64/` is the ARM port. One image runs on QEMU's `virt` board and on the Raspberry
Pi 3 machine model, and is intended for Raspberry Pi class boards. It shares the
hardware-independent core with the x86-64 kernel:

| Shared | Location |
|--------|----------|
| Inference engine, tokenizer, constrained decoding | `llm/` |
| Agent protocol (actions, prompt) and prompt cache | `llm/src/agent.rs`, `llm/src/prompt_cache.rs` |
| In-memory file system | `kernel/src/fs.rs` (included with `#[path]`) |
| Model file | the same `models/llmos.model`, including the baked prompt cache |

## Quick start

```bash
sudo apt install qemu-system-arm
cargo run -- arm                    # QEMU virt, Cortex-A72; uses models/llmos.model as the initrd
cargo run -- arm --no-model         # without the LLM
cargo run -- arm-test --no-model    # automated boot test

# Raspberry Pi 3 machine model (needs the board's device tree):
curl -LO https://raw.githubusercontent.com/raspberrypi/firmware/master/boot/bcm2710-rpi-3-b.dtb
cargo run -- arm --board raspi3b --dtb bcm2710-rpi-3-b.dtb --no-model
```

The image is written to `target/kernel8.img`.

## Boot flow

```text
firmware / QEMU loads kernel8.img at RAM base + 0x80000, x0 = device tree
  └─ boot.rs (assembly)
       • Linux arm64 image header (understood by QEMU and the Pi firmware)
       • park cores 1..n; drop EL2 → EL1 if needed; enable FP/NEON
       • stack, zero .bss, apply R_AARCH64_RELATIVE relocations
         (linked at 0 as a static PIE: runs at 0x80000 on a Pi, 0x40080000 on virt)
       • install the exception vectors
  └─ rust_main (main.rs)
       • device tree → PL011 UART (address translated through bus `ranges`)
       • MMU on: identity map, 2 MiB blocks, Normal for RAM, Device elsewhere
       • heap: largest RAM gap not used by the image, device tree or initrd
         (a 1 MiB bootstrap bump allocator serves the device-tree parser before that)
       • board info → LLM summary; model from the initrd
       • serial shell
```

## Tested

| Machine | Result |
|---------|--------|
| QEMU `virt`, Cortex-A72, 2 GiB | boots, selftest passes, LLM requests work (about 0.7 tokens/s under emulation) |
| QEMU `raspi3b` with `bcm2710-rpi-3-b.dtb` | boots at EL2 → EL1, finds "Raspberry Pi 3 Model B", 4× Cortex-A53, 960 MiB; selftest passes |
| Physical Raspberry Pi | **not tested yet** |

## Running on a real Raspberry Pi (3 or 4), untested

On the FAT boot partition of a Raspberry Pi OS SD card:

```ini
# config.txt
arm_64bit=1
kernel=kernel8.img
initramfs llmos.model followkernel
enable_uart=1
dtoverlay=disable-bt     # put the PL011 on GPIO 14/15 instead of Bluetooth
```

Copy `target/kernel8.img` and `models/llmos.model` next to it, connect a 3.3 V USB serial
adapter to GPIO 14/15 at 115200 baud, and power on.

## Limitations

* Serial console only: there are no framebuffer, keyboard or USB drivers yet.
* No storage drivers, so `install` and `list_disks` are not available. Files stay in RAM.
* Single core, polling only (no GIC or interrupt handling yet).
* The MMU maps only the first 4 GiB. Boards with more RAM work, but the extra memory is unused.
* Raspberry Pi 5 is not supported: its UART sits behind the RP1 south bridge.
* Power-off uses PSCI where the device tree offers it (QEMU `virt`). Otherwise the CPU halts.
