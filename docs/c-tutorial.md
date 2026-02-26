# C Programming for OS Development - Complete Tutorial

This tutorial teaches you C programming specifically for building operating systems. Each chapter builds on the previous, and every concept is demonstrated with code you can run and experiment with.

---

## Table of Contents

1. [Chapter 1: C Fundamentals](#chapter-1-c-fundamentals)
2. [Chapter 2: Pointers and Memory](#chapter-2-pointers-and-memory)
3. [Chapter 3: Structures and Bit Operations](#chapter-3-structures-and-bit-operations)
4. [Chapter 4: Inline Assembly](#chapter-4-inline-assembly)
5. [Chapter 5: Memory Management](#chapter-5-memory-management)
6. [Chapter 6: Hardware I/O](#chapter-6-hardware-io)
7. [Chapter 7: Building an OS Component](#chapter-7-building-an-os-component)

---

## Chapter 1: C Fundamentals

### 1.1 Your First C Program

Create a file called `hello.c`:

```c
// hello.c - Your first C program
#include <stdio.h>

int main() {
    printf("Hello, OS World!\n");
    return 0;
}
```

Compile and run:
```bash
gcc hello.c -o hello
./hello
```

### 1.2 Variables and Data Types

C has several basic data types:

```c
// data_types.c
#include <stdio.h>

int main() {
    // Integer types
    char c = 'A';           // 1 byte (-128 to 127)
    short s = 100;          // 2 bytes
    int i = 1000;           // 4 bytes (typically)
    long l = 100000;        // 8 bytes (on 64-bit)
    
    // Unsigned versions (0 to positive only)
    unsigned char uc = 255;
    unsigned int ui = 4000;
    
    // Floating point
    float f = 3.14f;
    double d = 3.14159;
    
    // Print sizes
    printf("char: %zu bytes\n", sizeof(char));
    printf("int: %zu bytes\n", sizeof(int));
    printf("pointer: %zu bytes\n", sizeof(int*));
    
    return 0;
}
```

**Key concept**: On a 64-bit system, pointers are 8 bytes. On 32-bit, they're 4 bytes.

### 1.3 Control Flow

```c
// control_flow.c
#include <stdio.h>

int main() {
    // If-else
    int x = 10;
    if (x > 5) {
        printf("x is greater than 5\n");
    } else {
        printf("x is 5 or less\n");
    }
    
    // For loop
    for (int i = 0; i < 5; i++) {
        printf("i = %d\n", i);
    }
    
    // While loop
    int count = 0;
    while (count < 3) {
        printf("count = %d\n", count);
        count++;
    }
    
    // Switch
    char grade = 'B';
    switch (grade) {
        case 'A': printf("Excellent!\n"); break;
        case 'B': printf("Good!\n"); break;
        case 'C': printf("Fair\n"); break;
        default:  printf("Unknown\n");
    }
    
    return 0;
}
```

### 1.4 Functions

```c
// functions.c
#include <stdio.h>

// Function declaration
int add(int a, int b);
void greet(char *name);

// Main function - entry point
int main() {
    int result = add(5, 3);
    printf("5 + 3 = %d\n", result);
    
    greet("OS Developer");
    
    return 0;
}

// Function definition
int add(int a, int b) {
    return a + b;
}

void greet(char *name) {
    printf("Hello, %s!\n", name);
}
```

**Exercise**: Write a function that calculates the factorial of a number.

---

## Chapter 2: Pointers and Memory

This is the MOST IMPORTANT chapter for OS development. Understanding pointers is essential.

### 2.1 What is a Pointer?

A pointer is a variable that stores a memory address.

```c
// pointers_intro.c
#include <stdio.h>

int main() {
    int var = 42;
    
    // & means "address of"
    int *ptr = &var;
    
    printf("var = %d\n", var);
    printf("ptr (address) = %p\n", (void*)ptr);
    printf("*ptr (value at address) = %d\n", *ptr);
    
    // Modify value through pointer
    *ptr = 100;
    printf("var after modification = %d\n", var);
    
    return 0;
}
```

**Visual representation:**
```
Memory:
┌─────────────┬─────────────┬─────────────┐
│   var: 42   │             │             │
└─────────────┘─────────────┘─────────────┘
      │
      │ &var (address: 0x7fff5fbff8ac)
      ▼
┌─────────────┐
│ ptr: 0x7fff │───► 0x7fff5fbff8ac ──► var: 42
└─────────────┘
```

### 2.2 Pointer Arithmetic

```c
// pointer_arithmetic.c
#include <stdio.h>

int main() {
    int arr[] = {10, 20, 30, 40, 50};
    int *p = arr;
    
    // Array name is pointer to first element
    printf("arr = %p\n", (void*)arr);
    printf("p = %p\n", (void*)p);
    printf("*p = %d\n", *p);
    
    // Increment pointer (moves by sizeof(int) bytes)
    p++;
    printf("After p++: *p = %d\n", *p);
    
    // Using array indexing with pointer
    p = arr;
    for (int i = 0; i < 5; i++) {
        printf("arr[%d] = %d, *(p+%d) = %d\n", i, arr[i], i, *(p+i));
    }
    
    return 0;
}
```

### 2.3 Void Pointers and Casting

```c
// void_pointers.c
#include <stdio.h>

int main() {
    int num = 42;
    double fp = 3.14;
    char ch = 'A';
    
    // Generic void pointer
    void *ptr;
    
    // Point to int
    ptr = &num;
    printf("As int: %d\n", *(int*)ptr);
    
    // Point to double
    ptr = &fp;
    printf("As double: %.2f\n", *(double*)ptr);
    
    // Point to char
    ptr = &ch;
    printf("As char: %c\n", *(char*)ptr);
    
    return 0;
}
```

### 2.4 Pointers and Strings

```c
// strings.c
#include <stdio.h>

int main() {
    // Two ways to represent strings
    
    // 1. Array (mutable)
    char arr[] = "Hello";
    arr[0] = 'J';
    printf("Array: %s\n", arr);
    
    // 2. Pointer (literal - immutable!)
    char *ptr = "World";
    // ptr[0] = 'X';  // ERROR! Can't modify literal
    
    // String functions
    char s1[] = "Hello";
    char s2[] = "World";
    
    // Concatenate (need buffer)
    char buffer[100];
    sprintf(buffer, "%s %s", s1, s2);
    printf("Concatenated: %s\n", buffer);
    
    // String length
    int len = 0;
    while (s1[len] != '\0') {
        len++;
    }
    printf("Length of s1: %d\n", len);
    
    return 0;
}
```

### 2.5 Practical Exercise: Implement string functions

```c
// string_exercise.c
#include <stdio.h>

// TODO: Implement these functions

int my_strlen(const char *s) {
    int len = 0;
    while (s[len] != '\0') {
        len++;
    }
    return len;
}

int my_strcmp(const char *s1, const char *s2) {
    while (*s1 && *s2) {
        if (*s1 != *s2) {
            return *s1 - *s2;
        }
        s1++;
        s2++;
    }
    return *s1 - *s2;
}

char *my_strcpy(char *dest, const char *src) {
    char *start = dest;
    while (*src) {
        *dest = *src;
        dest++;
        src++;
    }
    *dest = '\0';
    return start;
}

int main() {
    char s1[] = "Hello";
    char s2[] = "Hello";
    char dest[100];
    
    printf("strlen('Hello') = %d\n", my_strlen(s1));
    printf("strcmp('Hello', 'Hello') = %d\n", my_strcmp(s1, s2));
    printf("strcmp('Hello', 'World') = %d\n", my_strcmp(s1, "World"));
    printf("strcpy(dest, 'Test') = %s\n", my_strcpy(dest, "Test"));
    
    return 0;
}
```

---

## Chapter 3: Structures and Bit Operations

### 3.1 Structures

Structures group different types together:

```c
// structures.c
#include <stdio.h>

// Define a structure
struct Person {
    char name[50];
    int age;
    float height;
};

int main() {
    // Create struct variable
    struct Person p1;
    
    // Assign values
    p1.age = 25;
    p1.height = 5.9f;
    sprintf(p1.name, "Alice");
    
    printf("Name: %s\n", p1.name);
    printf("Age: %d\n", p1.age);
    printf("Height: %.1f\n", p1.height);
    
    // Initialize directly
    struct Person p2 = {"Bob", 30, 6.0f};
    printf("P2: %s, %d\n", p2.name, p2.age);
    
    // Pointer to struct
    struct Person *ptr = &p1;
    printf("Via pointer: %s\n", ptr->name);
    
    return 0;
}
```

### 3.2 Bit Operations

Essential for OS development - hardware registers, flagsc
// bit_operations.c
, etc.

```#include <stdio.h>

int main() {
    unsigned int value = 0b11011010;  // Binary literal (C14+)
    
    // Bitwise AND
    printf("value & 0xFF = 0x%02X\n", value & 0xFF);
    
    // Bitwise OR
    value = value | 0x0F;
    printf("value | 0x0F = 0x%02X\n", value);
    
    // Bitwise XOR
    value = value ^ 0xFF;
    printf("value ^ 0xFF = 0x%02X\n", value);
    
    // Bitwise NOT
    printf("~value = 0x%02X\n", ~value);
    
    // Left shift
    value = 1;
    value = value << 3;  // Multiply by 8
    printf("1 << 3 = %d\n", value);
    
    // Right shift
    value = 8;
    value = value >> 2;  // Divide by 4
    printf("8 >> 2 = %d\n", value);
    
    // Check if bit is set
    unsigned int flags = 0b10101010;
    if (flags & (1 << 3)) {
        printf("Bit 3 is set\n");
    }
    
    // Set a bit
    flags = flags | (1 << 0);  // Set bit 0
    printf("After setting bit 0: 0x%02X\n", flags);
    
    // Clear a bit
    flags = flags & ~(1 << 0);  // Clear bit 0
    printf("After clearing bit 0: 0x%02X\n", flags);
    
    // Toggle a bit
    flags = flags ^ (1 << 0);  // Toggle bit 0
    printf("After toggling bit 0: 0x%02X\n", flags);
    
    return 0;
}
```

### 3.3 Bit Fields in Structures

```c
// bitfields.c
#include <stdio.h>

// Define flags using bit fields
struct Flags {
    unsigned int kernel_mode : 1;   // 1 bit
    unsigned int interrupts_on : 1; // 1 bit
    unsigned int paging_enabled : 1; // 1 bit
    unsigned int reserved : 29;    // 29 bits
};

int main() {
    struct Flags f = {0};
    
    f.kernel_mode = 1;
    f.interrupts_on = 0;
    f.paging_enabled = 1;
    
    printf("kernel_mode = %d\n", f.kernel_mode);
    printf("interrupts_on = %d\n", f.interrupts_on);
    printf("paging_enabled = %d\n", f.paging_enabled);
    
    // Access as regular integer
    unsigned int *raw = (unsigned int*)&f;
    printf("Raw value: 0x%08X\n", *raw);
    
    return 0;
}
```

### 3.4 Practical: VGA Color Attribute

In VGA text mode, each character has a color attribute:

```c
// vga_colors.c
#include <stdio.h>

// VGA Color definitions
#define VGA_COLOR_BLACK         0
#define VGA_COLOR_BLUE          1
#define VGA_COLOR_GREEN         2
#define VGA_COLOR_CYAN          3
#define VGA_COLOR_RED           4
#define VGA_COLOR_MAGENTA       5
#define VGA_COLOR_BROWN         6
#define VGA_COLOR_LIGHT_GREY    7
#define VGA_COLOR_DARK_GREY     8
#define VGA_COLOR_LIGHT_BLUE    9
#define VGA_COLOR_LIGHT_GREEN   10
#define VGA_COLOR_LIGHT_CYAN    11
#define VGA_COLOR_LIGHT_RED     12
#define VGA_COLOR_LIGHT_MAGENTA 13
#define VGA_COLOR_LIGHT_BROWN   14
#define VGA_COLOR_WHITE         15

// Make color attribute: bg << 4 | fg
#define vga_entry_color(fg, bg) ((bg << 4) | (fg))

// Make VGA entry: character | (color << 8)
#define vga_entry(ch, color) ((uint16_t)(ch) | ((uint16_t)(color) << 8))

typedef unsigned short uint16_t;

int main() {
    uint16_t entry = vga_entry('A', vga_entry_color(VGA_COLOR_WHITE, VGA_COLOR_BLUE));
    
    printf("Character 'A' with white on blue: 0x%04X\n", entry);
    printf("Binary: ");
    for (int i = 15; i >= 0; i--) {
        printf("%d", (entry >> i) & 1);
    }
    printf("\n");
    
    // Extract parts
    unsigned char ch = entry & 0xFF;
    unsigned char color = (entry >> 8) & 0xFF;
    unsigned char bg = (color >> 4) & 0x0F;
    unsigned char fg = color & 0x0F;
    
    printf("Char: '%c', Color: 0x%02X (bg: %d, fg: %d)\n", ch, color, bg, fg);
    
    return 0;
}
```

---

## Chapter 4: Inline Assembly

Direct CPU access - essential for OS development.

### 4.1 Basic Inline Assembly

```c
// inline_asm.c
#include <stdio.h>

int main() {
    unsigned int value = 10;
    
    // Simple assembly: add 5 to value
    // "r" means register - compiler chooses one
    __asm__ volatile (
        "addl $5, %0"
        : "+r" (value)   // Output: value modified
    );
    
    printf("10 + 5 = %d\n", value);
    
    // Another example: multiply by 2
    value = 7;
    __asm__ volatile (
        "imull $2, %0"
        : "+r" (value)
    );
    printf("7 * 2 = %d\n", value);
    
    return 0;
}
```

### 4.2 Input and Output Operands

```c
// asm_io.c
#include <stdio.h>

int add_numbers(int a, int b) {
    int result;
    __asm__ volatile (
        "addl %1, %0"      // result = result + b
        : "=r" (result)    // Output: result
        : "r" (a), "0" (result)  // Input: a, initial result = b
    );
    return result;
}

int main() {
    printf("5 + 3 = %d\n", add_numbers(5, 3));
    printf("10 + 20 = %d\n", add_numbers(10, 20));
    return 0;
}
```

### 4.3 Reading and Writing Ports (x86)

```c
// port_io.c
#include <stdio.h>

// Read from port (8-bit)
static inline unsigned char inb(unsigned short port) {
    unsigned char val;
    __asm__ volatile ("inb %1, %0" : "=a"(val) : "Nd"(port));
    return val;
}

// Write to port (8-bit)
static inline void outb(unsigned short port, unsigned char val) {
    __asm__ volatile ("outb %0, %1" : : "a"(val), "Nd"(port));
}

int main() {
    // Read keyboard status port (0x64)
    unsigned char status = inb(0x64);
    printf("Keyboard status: 0x%02X\n", status);
    printf("  Output buffer full: %d\n", (status >> 0) & 1);
    printf("  Input buffer full: %d\n", (status >> 1) & 1);
    printf("  System flag: %d\n", (status >> 4) & 1);
    
    // Write to port 0x60 (keyboard data) - just for demo
    // outb(0x60, 0xFF);
    
    return 0;
}
```

### 4.4 CPU Control Registers

```c
// cpu_registers.c
#include <stdio.h>
#include <stdint.h>

static inline uint64_t read_cr0(void) {
    uint64_t val;
    __asm__ volatile ("mov %%cr0, %0" : "=r"(val));
    return val;
}

static inline void write_cr0(uint64_t val) {
    __asm__ volatile ("mov %0, %%cr0" : : "r"(val));
}

static inline uint64_t read_cr3(void) {
    uint64_t val;
    __asm__ volatile ("mov %%cr3, %0" : "=r"(val));
    return val;
}

static inline uint64_t read_cr4(void) {
    uint64_t val;
    __asm__ volatile ("mov %%cr4, %0" : "=r"(val));
    return val;
}

int main() {
    printf("CR0: 0x%016lX\n", read_cr0());
    printf("CR3: 0x%016lX\n", read_cr3());
    printf("CR4: 0x%016lX\n", read_cr4());
    
    // CR0 bit definitions
    uint64_t cr0 = read_cr0();
    printf("\nCR0 Flags:\n");
    printf("  PE (Protection Enable): %d\n", (cr0 >> 0) & 1);
    printf("  MP (Monitor Coprocessor): %d\n", (cr0 >> 1) & 1);
    printf("  EM (Emulation): %d\n", (cr0 >> 2) & 1);
    printf("  PG (Paging): %d\n", (cr0 >> 31) & 1);
    
    return 0;
}
```

---

## Chapter 5: Memory Management

### 5.1 Understanding Virtual Memory

```c
// virtual_memory.c
#include <stdio.h>
#include <stdlib.h>

int global_var = 100;           // Data segment
int uninit_global;              // BSS segment

int main() {
    // Stack variables
    int stack_var = 10;
    int *heap_ptr = malloc(sizeof(int));  // Heap
    
    printf("Global variable (data):   %p (value: %d)\n", (void*)&global_var, global_var);
    printf("Uninitialized global:     %p (value: %d)\n", (void*)&uninit_global, uninit_global);
    printf("Stack variable:            %p (value: %d)\n", (void*)&stack_var, stack_var);
    printf("Heap allocated:            %p (value: %d)\n", (void*)heap_ptr, *heap_ptr);
    printf("Main function:             %p\n", (void*)main);
    
    // Show pointer difference
    printf("\nStack grows down, heap grows up!\n");
    printf("Stack addr > Heap addr? %s\n", &stack_var > heap_ptr ? "Yes" : "No");
    
    free(heap_ptr);
    return 0;
}
```

### 5.2 Building a Simple Allocator

```c
// simple_allocator.c
#include <stdio.h>
#include <stdint.h>
#include <stddef.h>

#define PAGE_SIZE 4096
#define HEAP_START 0x100000

static char heap[PAGE_SIZE * 16];
static size_t heap_offset = 0;

// Simple malloc implementation
void *kmalloc(size_t size) {
    // Align to 8 bytes
    size = (size + 7) & ~7;
    
    if (heap_offset + size > sizeof(heap)) {
        return NULL;
    }
    
    void *ptr = &heap[heap_offset];
    heap_offset += size;
    
    return ptr;
}

int main() {
    printf("Simple Allocator Test\n");
    printf("Heap size: %zu bytes\n", sizeof(heap));
    printf("Heap start: %p\n", (void*)heap);
    
    // Allocate some memory
    int *a = kmalloc(sizeof(int));
    *a = 42;
    printf("Allocated int at %p: %d\n", (void*)a, *a);
    
    char *b = kmalloc(100);
    sprintf(b, "Hello from kmalloc!");
    printf("Allocated char[100] at %p: %s\n", (void*)b, b);
    
    double *c = kmalloc(sizeof(double));
    *c = 3.14159;
    printf("Allocated double at %p: %f\n", (void*)c, *c);
    
    printf("Heap used: %zu bytes\n", heap_offset);
    
    return 0;
}
```

### 5.3 Page Tables (Conceptual)

```c
// page_tables.c
#include <stdio.h>
#include <stdint.h>

// x86-64 PML4 entry (Page Map Level 4)
typedef struct {
    uint64_t present : 1;
    uint64_t write : 1;
    uint64_t user : 1;
    uint64_t write_through : 1;
    uint64_t cache_disable : 1;
    uint64_t accessed : 1;
    uint64_t ignored : 1;
    uint64_t page_size : 1;
    uint64_t global : 1;
    uint64_t available : 3;
    uint64_t address : 40;  // Physical address (52 bits on some CPUs)
    uint64_t reserved : 11;
    uint64_t nx : 1;        // No-execute
} __attribute__((packed)) pml4e_t;

int main() {
    printf("PML4E Structure Size: %zu bytes\n", sizeof(pml4e_t));
    
    // Create a PML4 entry
    pml4e_t entry = {0};
    entry.present = 1;
    entry.write = 1;
    entry.user = 1;
    entry.address = 0x11000 >> 12;  // Physical address >> 12
    
    printf("\nPML4E Example:\n");
    printf("  Present: %d\n", entry.present);
    printf("  Writable: %d\n", entry.write);
    printf("  User mode: %d\n", entry.user);
    printf("  Physical Address: 0x%lx\n", entry.address << 12);
    
    uint64_t *raw = (uint64_t*)&entry;
    printf("  Raw value: 0x%016lx\n", *raw);
    
    return 0;
}
```

---

## Chapter 6: Hardware I/O

### 6.1 VGA Text Mode

```c
// vga_driver.c
#include <stdio.h>
#include <stdint.h>

#define VGA_WIDTH 80
#define VGA_HEIGHT 25
#define VGA_MEMORY 0xB8000

// Color codes
enum vga_color {
    VGA_BLACK = 0,
    VGA_BLUE = 1,
    VGA_GREEN = 2,
    VGA_CYAN = 3,
    VGA_RED = 4,
    VGA_MAGENTA = 5,
    VGA_BROWN = 6,
    VGA_LIGHT_GREY = 7,
    VGA_DARK_GREY = 8,
    VGA_LIGHT_BLUE = 9,
    VGA_LIGHT_GREEN = 10,
    VGA_LIGHT_CYAN = 11,
    VGA_LIGHT_RED = 12,
    VGA_LIGHT_MAGENTA = 13,
    VGA_LIGHT_BROWN = 14,
    VGA_WHITE = 15,
};

// Make color byte
static inline uint8_t vga_color(uint8_t fg, uint8_t bg) {
    return (bg << 4) | fg;
}

// VGA buffer (memory-mapped I/O)
static volatile uint16_t *vga_buffer = (volatile uint16_t *)VGA_MEMORY;

static int cursor_x = 0;
static int cursor_y = 0;
static uint8_t current_color = vga_color(VGA_WHITE, VGA_BLACK);

void vga_clear(void) {
    for (int y = 0; y < VGA_HEIGHT; y++) {
        for (int x = 0; x < VGA_WIDTH; x++) {
            vga_buffer[y * VGA_WIDTH + x] = 
                (uint16_t)' ' | ((uint16_t)current_color << 8);
        }
    }
    cursor_x = 0;
    cursor_y = 0;
}

void vga_putchar(char c) {
    if (c == '\n') {
        cursor_x = 0;
        cursor_y++;
    } else {
        vga_buffer[cursor_y * VGA_WIDTH + cursor_x] = 
            (uint16_t)c | ((uint16_t)current_color << 8);
        cursor_x++;
    }
    
    if (cursor_x >= VGA_WIDTH) {
        cursor_x = 0;
        cursor_y++;
    }
    
    if (cursor_y >= VGA_HEIGHT) {
        // Scroll
        for (int y = 0; y < VGA_HEIGHT - 1; y++) {
            for (int x = 0; x < VGA_WIDTH; x++) {
                vga_buffer[y * VGA_WIDTH + x] = 
                    vga_buffer[(y + 1) * VGA_WIDTH + x];
            }
        }
        // Clear last line
        for (int x = 0; x < VGA_WIDTH; x++) {
            vga_buffer[(VGA_HEIGHT - 1) * VGA_WIDTH + x] = 
                (uint16_t)' ' | ((uint16_t)current_color << 8);
        }
        cursor_y = VGA_HEIGHT - 1;
    }
}

void vga_write(const char *str) {
    while (*str) {
        vga_putchar(*str);
        str++;
    }
}

void vga_set_color(uint8_t fg, uint8_t bg) {
    current_color = vga_color(fg, bg);
}

int main() {
    // Note: This only works when running on bare metal or in QEMU
    // Won't show anything in regular terminal
    
    printf("VGA Driver Code (see actual VGA screen when running on OS)\n");
    printf("VGA memory mapped at: 0x%X\n", VGA_MEMORY);
    printf("Each character cell is 2 bytes: char + attribute\n");
    
    return 0;
}
```

### 6.2 Keyboard Controller

```c
// keyboard.c
#include <stdio.h>
#include <stdint.h>

#define KEYBOARD_DATA_PORT 0x60
#define KEYBOARD_STATUS_PORT 0x64

// Scancode to ASCII mapping (simplified)
static const char scancode_map[128] = {
    0,   0,   '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=', '\b',
    '\t', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']', '\n',
    0,   'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', ';', '\'', '`',
    0,   '\\', 'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/', 0,
    '*', 0,   ' ', 0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   '7', '8', '9', '-',
    '4', '5', '6', '+', '1', '2', '3', '0', '.'
};

// Read from keyboard
static inline uint8_t keyboard_read(void) {
    uint8_t val;
    __asm__ volatile ("inb %1, %0" : "=a"(val) : "Nd"(KEYBOARD_DATA_PORT));
    return val;
}

// Check keyboard status
static inline uint8_t keyboard_status(void) {
    uint8_t val;
    __asm__ volatile ("inb %1, %0" : "=a"(val) : "Nd"(KEYBOARD_STATUS_PORT));
    return val;
}

int main() {
    printf("Keyboard Controller Test\n");
    printf("Data port: 0x%X\n", KEYBOARD_DATA_PORT);
    printf("Status port: 0x%X\n", KEYBOARD_STATUS_PORT);
    
    printf("\nKey mappings (some common keys):\n");
    for (int i = 0x02; i <= 0x0D; i++) {
        printf("  0x%02X -> '%c'\n", i, scancode_map[i]);
    }
    
    printf("\nStatus register bits:\n");
    printf("  Bit 0: Output buffer full (read this bit before reading data)\n");
    printf("  Bit 1: Input buffer full (don't write while this is set)\n");
    printf("  Bit 4: Keyboard enabled\n");
    printf("  Bit 5: Timeout error\n");
    printf("  Bit 6: Parity error\n");
    printf("  Bit 7: General timeout\n");
    
    return 0;
}
```

---

## Chapter 7: Building an OS Component

### 7.1 Complete VGA Driver for Your OS

Here's a production-quality VGA driver you can use in llmos:

```c
// kernel/src/vga.c - Production VGA Driver
#include "../include/vga.h"

static volatile uint16_t *vga_buffer = (volatile uint16_t *)0xB8000;
static int cursor_x = 0;
static int cursor_y = 0;
static uint8_t current_color = 0x0F; // White on black

void vga_init(void) {
    cursor_x = 0;
    cursor_y = 0;
    current_color = 0x0F;
    vga_clear();
}

void vga_clear(void) {
    uint16_t blank = (uint16_t)' ' | ((uint16_t)current_color << 8);
    
    for (int y = 0; y < VGA_HEIGHT; y++) {
        for (int x = 0; x < VGA_WIDTH; x++) {
            vga_buffer[y * VGA_WIDTH + x] = blank;
        }
    }
    cursor_x = 0;
    cursor_y = 0;
}

void vga_set_color(uint8_t fg, uint8_t bg) {
    current_color = (bg << 4) | (fg & 0x0F);
}

void vga_putchar(char c) {
    if (c == '\n' || c == '\r') {
        cursor_x = 0;
        cursor_y++;
        if (cursor_y >= VGA_HEIGHT) {
            cursor_y = VGA_HEIGHT - 1;
            // Scroll
            for (int y = 0; y < VGA_HEIGHT - 1; y++) {
                for (int x = 0; x < VGA_WIDTH; x++) {
                    vga_buffer[y * VGA_WIDTH + x] = 
                        vga_buffer[(y + 1) * VGA_WIDTH + x];
                }
            }
            // Clear last line
            uint16_t blank = (uint16_t)' ' | ((uint16_t)current_color << 8);
            for (int x = 0; x < VGA_WIDTH; x++) {
                vga_buffer[(VGA_HEIGHT - 1) * VGA_WIDTH + x] = blank;
            }
        }
        return;
    }
    
    vga_buffer[cursor_y * VGA_WIDTH + cursor_x] = 
        (uint16_t)c | ((uint16_t)current_color << 8);
    cursor_x++;
    
    if (cursor_x >= VGA_WIDTH) {
        cursor_x = 0;
        cursor_y++;
        if (cursor_y >= VGA_HEIGHT) {
            cursor_y = VGA_HEIGHT - 1;
        }
    }
}

void vga_write(const char *str) {
    while (*str) {
        vga_putchar(*str);
        str++;
    }
}

void vga_write_at(const char *str, int x, int y) {
    int old_x = cursor_x;
    int old_y = cursor_y;
    cursor_x = x;
    cursor_y = y;
    vga_write(str);
    cursor_x = old_x;
    cursor_y = old_y;
}
```

### 7.2 Complete Keyboard Driver

```c
// kernel/src/keyboard.c - Production Keyboard Driver
#include "../include/keyboard.h"

#define KEYBOARD_BUFFER_SIZE 256

static char keyboard_buffer[KEYBOARD_BUFFER_SIZE];
static int buffer_head = 0;
static int buffer_tail = 0;
static int shift_pressed = 0;

// Scancode to ASCII (without shift)
static const char scancode_map[128] = {
    0,   0,   '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=', '\b',
    '\t', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']', '\n',
    0,   'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', ';', '\'', '`',
    0,   '\\', 'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/', 0,
    '*', 0,   ' ', 0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   '7', '8', '9', '-',
    '4', '5', '6', '+', '1', '2', '3', '0', '.'
};

// Scancode to ASCII (with shift)
static const char scancode_shift[128] = {
    0,   0,   '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '_', '+', '\b',
    '\t', 'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P', '{', '}', '\n',
    0,   'A', 'S', 'D', 'F', 'G', 'H', 'J', 'K', 'L', ':', '"', '~',
    0,   '|', 'Z', 'X', 'C', 'V', 'B', 'N', 'M', '<', '>', '?', 0,
    '*', 0,   ' ', 0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   '7', '8', '9', '-',
    '4', '5', '6', '+', '1', '2', '3', '0', '.'
};

// Inline functions for port I/O
static inline unsigned char inb(unsigned short port) {
    unsigned char val;
    __asm__ volatile ("inb %1, %0" : "=a"(val) : "Nd"(port));
    return val;
}

void keyboard_init(void) {
    buffer_head = 0;
    buffer_tail = 0;
    shift_pressed = 0;
}

// Call this from keyboard interrupt handler
void keyboard_handle_interrupt(void) {
    unsigned char status = inb(0x64);
    if (!(status & 1)) return;
    
    unsigned char scancode = inb(0x60);
    
    // Key release
    if (scancode & 0x80) {
        scancode &= 0x7F;
        if (scancode == 0x2A || scancode == 0x36) {
            shift_pressed = 0;
        }
        return;
    }
    
    // Shift keys
    if (scancode == 0x2A || scancode == 0x36) {
        shift_pressed = 1;
        return;
    }
    
    // Get character
    char c = shift_pressed ? scancode_shift[scancode] : scancode_map[scancode];
    
    // Add to buffer if valid
    if (c && ((buffer_head + 1) % KEYBOARD_BUFFER_SIZE) != buffer_tail) {
        keyboard_buffer[buffer_head] = c;
        buffer_head = (buffer_head + 1) % KEYBOARD_BUFFER_SIZE;
    }
}

int keyboard_has_char(void) {
    return buffer_head != buffer_tail;
}

char keyboard_getchar(void) {
    while (!keyboard_has_char());
    char c = keyboard_buffer[buffer_tail];
    buffer_tail = (buffer_tail + 1) % KEYBOARD_BUFFER_SIZE;
    return c;
}
```

---

## Next Steps

Now that you've completed this tutorial:

1. **Review the llmos code** - Look at each file and understand how these concepts apply
2. **Build the OS** - Run `make` and test in QEMU
3. **Modify and experiment** - Change colors, add features
4. **Add the LLM** - Download TinyLlama and integrate it

## Quick Reference

### Common C Idioms for OS Dev

```c
// Prevent compiler from optimizing
volatile int value;

// Declare function never returns
__attribute__((noreturn)) void panic(void);

// Pack structure (no padding)
struct __attribute__((packed)) Packed {
    char a;
    int b;
};

// Static inline - put in header
static inline void outb(uint16_t port, uint8_t val) {
    __asm__ volatile ("outb %0, %1" : : "a"(val), "Nd"(port));
}
```

### Memory Addresses for VGA/Keyboard

| Address | Description |
|---------|-------------|
| 0xB8000 | VGA text buffer |
| 0xB0000 | Monochrome buffer |
| 0x60 | Keyboard data port |
| 0x64 | Keyboard status port |

### Port I/O

| Port | Description |
|------|-------------|
| 0x60 | Keyboard data / AT controller data |
| 0x64 | Keyboard status / command |
| 0x3F8 | COM1 serial port |
| 0x3F4 | COM1 status |

---

Happy OS Developing!
