# llmos Build System

.PHONY: all clean kernel bootloader run run-no-gui debug iso usb help

# Configuration
ARCH = x86_64
CROSS_PREFIX = 
CC = $(CROSS_PREFIX)gcc
AS = $(CROSS_PREFIX)as
LD = $(CROSS_PREFIX)ld
OBJCOPY = $(CROSS_PREFIX)objcopy

# Paths
BUILD_DIR = build
BOOTLOADER_DIR = bootloader
KERNEL_DIR = kernel
LLM_DIR = llm

# Files
KERNEL_BIN = $(BUILD_DIR)/kernel.bin
BOOTLOADER_BIN = $(BUILD_DIR)/bootloader.bin
OS_IMAGE = $(BUILD_DIR)/llmos.img

# Compiler flags
CFLAGS = -m64 -std=gnu99 -Wall -Wextra -Wno-unused-parameter -fno-pie -no-pie
CFLAGS += -fno-tree-loop-distribute-patterns -fno-stack-protector
CFLAGS += -I$(KERNEL_DIR)/include -I$(BOOTLOADER_DIR)
CFLAGS_BOOTLOADER = -m64 -ffreestanding -fno-pie -no-pie -nostdinc -nostdlib -fno-stack-protector -fno-PIE

# Kernel linker script
KERNEL_LDS = $(KERNEL_DIR)/linker.ld

# Default target
all: $(BUILD_DIR) $(BOOTLOADER_BIN) $(KERNEL_BIN) $(OS_IMAGE)

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

# ============================================
# BOOTLOADER (Assembly + C)
# ============================================

$(BOOTLOADER_DIR)/boot_asm.o: $(BOOTLOADER_DIR)/boot_asm.S
	$(CC) $(CFLAGS_BOOTLOADER) -c -o $@ $<

$(BOOTLOADER_DIR)/main.o: $(BOOTLOADER_DIR)/main.c
	$(CC) $(CFLAGS_BOOTLOADER) -c -o $@ $<

$(BOOTLOADER_BIN): $(BOOTLOADER_DIR)/boot_asm.o $(BOOTLOADER_DIR)/main.o
	$(LD) -T $(BOOTLOADER_DIR)/linker.ld -o $@ $^

# ============================================
# KERNEL
# ============================================

KERNEL_SRC = $(wildcard $(KERNEL_DIR)/src/*.c)
KERNEL_OBJ = $(patsubst $(KERNEL_DIR)/src/%.c,$(BUILD_DIR)/%.o,$(KERNEL_SRC))

$(BUILD_DIR)/%.o: $(KERNEL_DIR)/src/%.c
	$(CC) $(CFLAGS) -ffreestanding -c -o $@ $<

$(KERNEL_BIN): $(KERNEL_OBJ) $(KERNEL_LDS)
	$(LD) -T $(KERNEL_LDS) -o $@ $(KERNEL_OBJ)

# ============================================
# COMBINED OS IMAGE
# ============================================

$(OS_IMAGE): $(BOOTLOADER_BIN) $(KERNEL_BIN)
	@echo "Creating OS image..."
	dd if=/dev/zero of=$@ bs=1M count=64 2>/dev/null
	dd if=$(BOOTLOADER_BIN) of=$@ bs=512 conv=notrunc 2>/dev/null
	dd if=$(KERNEL_BIN) of=$@ bs=512 seek=1 conv=notrunc 2>/dev/null
	@echo "OS image created: $@"

# ============================================
# RUNTIME TARGETS
# ============================================

run: $(OS_IMAGE)
	qemu-system-x86_64 -kernel $(KERNEL_BIN) -m 512M -append "console=ttyS0" -nographic \
		-display none 2>/dev/null || \
	qemu-system-x86_64 -kernel $(KERNEL_BIN) -m 512M

run-gui: $(OS_IMAGE)
	qemu-system-x86_64 -kernel $(KERNEL_BIN) -m 512M

run-no-gui: $(OS_IMAGE)
	qemu-system-x86_64 -kernel $(KERNEL_BIN) -m 512M -nographic

debug: $(OS_IMAGE)
	qemu-system-x86_64 -s -S -kernel $(KERNEL_BIN) -m 512M &

iso: $(OS_IMAGE)
	mkdir -p $(BUILD_DIR)/iso/boot/grub
	cp $(OS_IMAGE) $(BUILD_DIR)/iso/boot/
	echo 'set default=0\nset timeout=0\nmenuentry "llmos" {\n\tlinux16 /boot/llmos.bin\n}' > $(BUILD_DIR)/iso/boot/grub/grub.cfg
	grub-mkrescue -o $(BUILD_DIR)/llmos.iso $(BUILD_DIR)/iso 2>/dev/null || \
		echo "Install grub to create ISO"

usb: $(OS_IMAGE)
	@echo "To write to USB:"
	@echo "  sudo dd if=$(OS_IMAGE) of=/dev/sdX bs=512"
	@echo "Replace /dev/sdX with your USB device"

# ============================================
# CLEAN
# ============================================

clean:
	rm -rf $(BUILD_DIR)
	rm -f $(BOOTLOADER_DIR)/*.o
	rm -f $(KERNEL_DIR)/*.o

# ============================================
# HELP
# ============================================

help:
	@echo "llmos Build System"
	@echo ""
	@echo "Targets:"
	@echo "  all          - Build everything (default)"
	@echo "  clean        - Remove build artifacts"
	@echo "  run          - Run in QEMU (terminal)"
	@echo "  run-gui      - Run in QEMU (GUI window)"
	@echo "  debug        - Run in QEMU with GDB"
	@echo "  iso          - Create bootable ISO"
	@echo "  usb          - Show USB write instructions"
	@echo "  help         - Show this help"
	@echo ""
	@echo "Files:"
	@echo "  Bootloader:  $(BOOTLOADER_BIN)"
	@echo "  Kernel:      $(KERNEL_BIN)"
	@echo "  OS Image:    $(OS_IMAGE)"
