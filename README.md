# llmos - LLM-Integrated Operating System

A custom x86-64 operating system kernel where the LLM is the primary shell interface. Users type natural language commands, the LLM interprets them, and executes system actions.

## Overview

llmos is a from-scratch operating system with:

- **Custom x86-64 kernel** written in C
- **Bootloader** that switches to long mode and loads the kernel
- **FAT16 filesystem** for persistent storage
- **VGA text mode** for display
- **PS/2 keyboard** driver for input
- **Embedded TinyLlama-1.1B** as the shell interface

## Architecture

```
┌─────────────────────────────────────────┐
│           LLM Shell (User)              │
│  "open file, type hello, save"          │
│  ↓                                      │
│  LLM interprets → JSON action           │
│  ↓                                      │
│  System calls (file, process, etc)      │
└─────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────┐
│         Kernel (C)                       │
│  - Memory management (paging)           │
│  - Process scheduler                     │
│  - FAT16 filesystem                      │
│  - Device drivers (VGA, Keyboard)       │
│  - System call interface                │
└─────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────┐
│         Bootloader                      │
│  - BIOS/UEFI boot                       │
│  - Long mode setup                      │
│  - Kernel loading                       │
└─────────────────────────────────────────┘
```

## Features

| Feature | Status |
|---------|--------|
| Bootloader | ✅ Basic |
| VGA Text Output | ✅ Basic |
| Keyboard Input | ✅ Basic |
| Memory Management | ✅ Paging |
| GDT/IDT | ✅ Interrupts |
| FAT16 Filesystem | 🔄 In Progress |
| LLM Integration | 🔄 In Progress |
| Natural Language Shell | 🔄 In Progress |

## Requirements

### Host System
- Linux (Ubuntu 22.04+ recommended)
- GCC (cross-compiler for x86-64)
- NASM assembler
- QEMU (for testing)
- Make
- Git

### Target System (minimum)
- x86-64 CPU
- 2GB RAM
- Keyboard
- VGA display

## Quick Start

### 1. Install Dependencies

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install build-essential nasm qemu-system-x86 git make gcc-multilib clang

# Verify installations
nasm --version
qemu-system-x86_64 --version
gcc --version
```

### 2. Clone & Build

```bash
git clone https://github.com/YOUR_USERNAME/llmos.git
cd llmos
make
```

### 3. Run in QEMU

```bash
make run
```

### 4. Build for Real Hardware

```bash
make usb
# Then write to USB: sudo dd if=build/llmos.bin of=/dev/sdX bs=512
```

## Project Structure

```
llmos/
├── README.md              # This file
├── Makefile               # Build system
├── CONTRIBUTING.md        # Contribution guidelines
│
├── bootloader/           # Bootloader code
│   ├── boot.S            # Boot sector (assembly)
│   ├── main.c            # Bootloader C code
│   ├── linker.ld        # Linker script
│   └── Makefile         # Bootloader build
│
├── kernel/               # Kernel source
│   ├── src/
│   │   ├── main.c       # Kernel entry point
│   │   ├── memory.c     # Memory management
│   │   ├── paging.c     # Page table setup
│   │   ├── gdt.c        # Global Descriptor Table
│   │   ├── idt.c        # Interrupt Descriptor Table
│   │   ├── keyboard.c   # Keyboard driver
│   │   ├── vga.c        # VGA text mode driver
│   │   ├── fs.c         # FAT16 filesystem
│   │   ├── process.c    # Process management
│   │   ├── syscall.c    # System calls
│   │   └── shell.c      # LLM shell interface
│   ├── include/
│   │   ├── kernel.h
│   │   ├── memory.h
│   │   ├── vga.h
│   │   ├── keyboard.h
│   │   ├── fs.h
│   │   └── syscall.h
│   ├── linker.ld        # Kernel linker script
│   └── Makefile         # Kernel build
│
├── llm/                  # LLM integration
│   ├── shell.c          # LLM shell
│   ├── shell.h          # Shell header
│   ├── model/           # Model files
│   │   └── README.md    # Instructions to add TinyLlama
│   └── Makefile
│
├── tools/               # Build utilities
│   ├── build.sh         # Main build script
│   └── iso.sh          # ISO creation
│
└── docs/                # Documentation
    ├── architecture.md
    ├── syscall.md
    └── filesystem.md
```

## Using llmos

### Basic Commands

The LLM shell accepts natural language:

```
You: open a text document and type hello world
llmos: [creates file, writes "hello world", displays success]

You: list my files
llmos: [lists directory contents]

You: create a new folder called projects
llmos: [creates directory]
```

### System Commands

| Command | Action |
|---------|--------|
| `help` | Show available commands |
| `exit` | Shutdown system |
| `clear` | Clear screen |
| `reboot` | Reboot system |

## LLM Integration

### Adding the Model

1. Download TinyLlama-1.1B Q4 quantized:
```bash
# From https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF
# Download tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf
cp tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf llm/model/
```

2. Build with LLM support:
```bash
make llm-enabled
```

### System Prompt

The LLM is configured with a system prompt that instructs it to:
- Parse natural language commands
- Output JSON actions
- Have access to: file operations, process control, system info

## Development

### Building

```bash
# Clean build
make clean

# Full build
make

# Debug build (with debug symbols)
make debug

# Just kernel
make kernel

# Just bootloader
make bootloader
```

### Testing

```bash
# Run in QEMU (with display)
make run

# Run in QEMU (serial only, faster)
make run-no-gui

# Run with GDB
make debug
```

### Adding New System Calls

1. Define syscall number in `kernel/include/syscall.h`
2. Implement in `kernel/src/syscall.c`
3. Add to syscall table
4. Call from shell/LLM

## Roadmap

- [ ] Basic bootloader → kernel boot
- [ ] VGA text output
- [ ] Keyboard input
- [ ] Memory management (paging)
- [ ] Interrupt handling (IDT/GDT)
- [ ] FAT16 filesystem
- [ ] System call interface
- [ ] Basic shell (CLI)
- [ ] LLM integration (TinyLlama)
- [ ] Natural language command parsing
- [ ] Real hardware boot support

## Resources

- [OSDev Wiki](https://wiki.osdev.org/)
- [llama.cpp](https://github.com/ggerganov/llama.cpp)
- [TinyLlama](https://github.com/jzhang38/TinyLlama)
- [Writing a Simple Operating System from Scratch](https://www.cs.bham.ac.uk/~exr/lectures/opsys/10_11/lectures/os-dev.pdf)

## License

MIT License - See LICENSE file

## Contributing

See CONTRIBUTING.md for guidelines.
