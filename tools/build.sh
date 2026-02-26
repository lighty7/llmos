#!/bin/bash

set -e

echo "==================================="
echo "  llmos Build Script"
echo "==================================="

export ARCH=x86_64
export CROSS_PREFIX=

echo "Checking dependencies..."
command -v nasm >/dev/null 2>&1 || { echo "nasm not found. Install: sudo apt install nasm"; exit 1; }
command -v gcc >/dev/null 2>&1 || { echo "gcc not found. Install: sudo apt install build-essential"; exit 1; }
command -v qemu-system-x86 >/dev/null 2>&1 || { echo "qemu-system-x86 not found. Install: sudo apt install qemu-system-x86"; exit 1; }

echo "Creating build directory..."
mkdir -p build

echo "Assembling bootloader..."
nasm -f elf64 -o build/boot.o bootloader/boot.S

echo "Compiling bootloader..."
gcc -m64 -c -o build/bootloader_main.o bootloader/main.c -nostdinc -nostdlib

echo "Linking bootloader..."
ld -T bootloader/linker.ld -o build/bootloader.bin build/boot.o build/bootloader_main.o
objcopy -O binary build/bootloader.bin build/bootloader.bin

echo "Compiling kernel..."
for src in kernel/src/*.c; do
    obj="${build}/$(basename ${src%.c}).o"
    gcc -m64 -std=gnu99 -ffreestanding -fno-pie -no-pie -Wall -Wextra -c -o "$obj" "$src" -Ikernel/include
done

echo "Linking kernel..."
ld -T kernel/linker.ld -o build/kernel.unstripped ${build}/*.o
objcopy -O binary build/kernel.unstripped build/kernel.bin

echo "Creating disk image..."
dd if=/dev/zero of=build/llmos.img bs=1M count=64 2>/dev/null
dd if=build/bootloader.bin of=build/llmos.img bs=512 conv=notrunc 2>/dev/null
dd if=build/kernel.bin of=build/llmos.img bs=512 seek=1 conv=notrunc 2>/dev/null

echo ""
echo "==================================="
echo "  Build Complete!"
echo "==================================="
echo ""
echo "Outputs:"
echo "  Bootloader: build/bootloader.bin"
echo "  Kernel:     build/kernel.bin"
echo "  OS Image:  build/llmos.img"
echo ""
echo "To run in QEMU:"
echo "  make run"
echo ""
echo "To run with GUI:"
echo "  make run-gui"
