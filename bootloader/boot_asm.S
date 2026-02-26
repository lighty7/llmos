# llmos bootloader assembly - compiles with GCC as .S file
# Using AT&T syntax

.section .text
.global _start
.type _start, @function

_start:
    # Clear segments
    xor %rax, %rax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss
    
    # Setup stack
    mov $0x80000, %rsp
    
    # Enable A20
    inb $0x92, %al
    orb $2, %al
    outb %al, $0x92
    
    # Load GDT
    lgdt gdt64_ptr(%rip)
    
    # Enable protected mode
    mov %cr0, %rax
    orl $1, %eax
    mov %rax, %cr0
    
    # Far jump to set CS - use indirect jump via memory
    .byte 0x66  # Operand size override
    ljmp *ljmp_target(%rip)

ljmp_target:
    .word 0x0000  # Offset (low)
    .word 0x0008  # Selector (CS = 0x08)
    
1:
    # Setup segment registers
    mov $0x10, %rax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss
    
    # Setup PML4 - map virtual 0 to physical 0x11000
    lea pml4(%rip), %rdi
    xor %rax, %rax
    stosq
    stosq
    mov $0x12003, %rax
    stosq
    xor %rax, %rax
    stosq
    
    # Setup page directory (512 entries = 2MB)
    lea page_dir(%rip), %rdi
    mov $512, %rcx
    mov $3, %rax
    
2:
    stosq
    add $0x1000, %rax
    loop 2b
    
    # Enable PAE
    mov %cr4, %rax
    orl $0x20, %eax
    mov %rax, %cr4
    
    # Load page table pointer
    lea pml4(%rip), %rax
    mov %rax, %cr3
    
    # Enable long mode
    movl $0xC0000080, %ecx
    rdmsr
    orl $0x100, %eax
    wrmsr
    
    # Enable paging
    mov %cr0, %rax
    orl $0x80000000, %eax
    mov %rax, %cr0
    
    # Clear registers
    xor %rax, %rax
    xor %rbx, %rbx
    xor %rcx, %rcx
    xor %rdx, %rdx
    xor %rsi, %rsi
    xor %rdi, %rdi
    xor %rbp, %rbp
    
    # Call entry C function
    call entry
    
halt:
    hlt
    jmp halt

# GDT
.align 16
gdt64:
    .quad 0
    .quad 0x0020980000000000  # Code segment
    .quad 0x0000920000000000  # Data segment

gdt64_ptr:
    .word 23  # GDT limit
    .quad gdt64  # GDT address

# Page tables
.align 4096
pml4:
    .skip 4096

page_dir:
    .skip 4096
