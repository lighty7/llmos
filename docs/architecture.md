# llmos Architecture

## Overview

llmos is a custom x86-64 operating system kernel with integrated LLM shell. It follows a monolithic kernel architecture with a userspace shell that interprets natural language.

## System Layers

```
┌─────────────────────────────────────────────┐
│           Userspace                          │
│  ┌─────────────────────────────────────┐    │
│  │     LLM Shell                        │    │
│  │  - Natural language input           │    │
│  │  - Command parsing                  │    │
│  │  - Action execution                 │    │
│  └─────────────────────────────────────┘    │
└─────────────────────────────────────────────┘
                    ↓ System Calls
┌─────────────────────────────────────────────┐
│              Kernel                          │
│  ┌─────────────────────────────────────┐    │
│  │  Process Management                 │    │
│  │  Memory Management (Paging)         │    │
│  │  File System (FAT16)                │    │
│  │  Device Drivers                     │    │
│  │  System Call Interface              │    │
│  └─────────────────────────────────────┘    │
└─────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────┐
│           Bootloader                         │
│  - BIOS/UEFI Boot                            │
│  - Protected Mode → Long Mode               │
│  - Kernel Loading                            │
└─────────────────────────────────────────────┘
```

## Memory Layout

```
0x00000000 - 0x000FFFFF    BIOS/IVT (1MB)
0x000F0000 - 0x000FFFFF    BIOS Area
0x00100000 - 0x0FFFFFFF   Kernel (16MB - loaded at 1MB)
0x10000000 -               Free Memory
0xB8000 - 0xBFFFF         VGA Text Buffer
0xFFFFFFFF80000000+        Kernel Virtual Addresses
```

## Boot Process

1. **BIOS**: Loads first 512 bytes from boot device
2. **Boot Sector**: Enables A20, loads GDT, enables protected mode
3. **32-bit Mode**: Setup PAE, page tables
4. **64-bit Mode**: Load kernel, jump to kernel_main()

## System Calls

| Number | Name | Description |
|--------|------|-------------|
| 1 | write | Write to screen |
| 2 | read | Read input |
| 3 | create | Create file |
| 4 | close | Close file |
| 5 | open | Open file |
| 6 | delete | Delete file |
| 7 | list | List directory |
| 8 | exit | Exit/shutdown |
| 9 | getmem | Get memory info |
| 10 | clear | Clear screen |

## File System

In-memory filesystem with FAT16-like structure:

- Max files: 32
- Max filename: 64 chars
- Max file size: 4KB
- Stored in memory initially (future: persist to disk)

## LLM Integration

The LLM shell works as follows:

1. User types natural language command
2. Input sent to LLM (or command parser)
3. LLM returns JSON action
4. Shell parses JSON and executes system calls
5. Results displayed to user

### System Prompt
```
You are the shell of LLMOS. Interpret user commands and output JSON actions.
```

## Device Drivers

### VGA Driver
- Text mode 80x25
- Memory-mapped at 0xB8000
- Color attributes in high byte

### Keyboard Driver
- PS/2 keyboard
- Scancode to ASCII mapping
- Ring buffer for input

## Future Enhancements

- [ ] Full LLM inference (llama.cpp)
- [ ] Disk filesystem (FAT16)
- [ ] Process scheduler
- [ ] Multi-threading
- [ ] GPU support (VESA)
- [ ] USB support
- [ ] Network stack
