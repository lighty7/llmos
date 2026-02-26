#include "../include/keyboard.h"

static inline unsigned char inb(unsigned short port) {
    unsigned char val;
    __asm__ volatile ("inb %1, %0" : "=a"(val) : "Nd"(port));
    return val;
}

static inline void outb(unsigned short port, unsigned char val) {
    __asm__ volatile ("outb %0, %1" : : "a"(val), "Nd"(port));
}

static char keyboard_buffer[KEYBOARD_BUFFER_SIZE];
static int buffer_head = 0;
static int buffer_tail = 0;

static const char scancode_map[128] = {
    0, 0, '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=', '\b',
    '\t', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']', '\n',
    0, 'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', ';', '\'', '`',
    0, '\\', 'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/', 0,
    '*', 0, ' ', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, '7', '8', '9', '-', '4', '5', '6', '+', '1', '2', '3', '0', '.'
};

static const char scancode_shift[128] = {
    0, 0, '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '_', '+', '\b',
    '\t', 'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P', '{', '}', '\n',
    0, 'A', 'S', 'D', 'F', 'G', 'H', 'J', 'K', 'L', ':', '"', '~',
    0, '|', 'Z', 'X', 'C', 'V', 'B', 'N', 'M', '<', '>', '?', 0,
    '*', 0, ' ', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, '7', '8', '9', '-', '4', '5', '6', '+', '1', '2', '3', '0', '.'
};

static int shift_pressed = 0;

void keyboard_init(void) {
    buffer_head = 0;
    buffer_tail = 0;
    shift_pressed = 0;
}

void keyboard_handle_interrupt(void) {
    unsigned char status = inb(0x64);
    if (!(status & 1)) return;
    
    unsigned char scancode = inb(0x60);
    
    if (scancode & 0x80) {
        scancode &= 0x7f;
        if (scancode == 0x2a || scancode == 0x36) {
            shift_pressed = 0;
        }
    } else {
        if (scancode == 0x2a || scancode == 0x36) {
            shift_pressed = 1;
            return;
        }
        
        char c = shift_pressed ? scancode_shift[scancode] : scancode_map[scancode];
        
        if (c && ((buffer_head + 1) % KEYBOARD_BUFFER_SIZE) != buffer_tail) {
            keyboard_buffer[buffer_head] = c;
            buffer_head = (buffer_head + 1) % KEYBOARD_BUFFER_SIZE;
        }
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

void keyboard_read_buffer(char *buf, int max_len) {
    int i = 0;
    while (i < max_len - 1) {
        if (keyboard_has_char()) {
            char c = keyboard_getchar();
            if (c == '\n' || c == '\r') break;
            if (c == '\b') {
                if (i > 0) i--;
            } else {
                buf[i++] = c;
            }
        }
    }
    buf[i] = 0;
}
