# Contributing to llmos

Thank you for your interest in contributing!

## Getting Started

1. **Fork the repository**
2. **Clone your fork**: `git clone https://github.com/YOUR_USERNAME/llmos.git`
3. **Create a branch**: `git checkout -b feature/your-feature`

## Development Setup

```bash
# Install dependencies (Ubuntu/Debian)
sudo apt install build-essential nasm qemu-system-x86

# Build
make

# Run in QEMU
make run
```

## Coding Standards

- Use C99 standard (`-std=gnu99`)
- 4 spaces for indentation (no tabs)
- Max line length: 100 characters
- Comment your code for complex sections

## Project Structure

```
llmos/
├── bootloader/   # Boot code (assembly + C)
├── kernel/       # Kernel source
│   ├── src/      # Implementation
│   └── include/  # Headers
├── llm/          # LLM integration
└── docs/         # Documentation
```

## Pull Request Process

1. Test your changes with `make` and `make run`
2. Update documentation if needed
3. Submit a clear PR description

## Areas to Contribute

- VGA/Keyboard drivers
- Filesystem (FAT16)
- LLM integration (llama.cpp)
- Process management
- Memory management
- Documentation

## Questions?

Open an issue for questions about contributing.
