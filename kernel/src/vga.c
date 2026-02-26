#include "../include/vga.h"

static volatile uint16_t *vga_buffer = (uint16_t *)0xb8000;
static int cursor_x = 0;
static int cursor_y = 0;
static uint8_t current_color = 0x0f;

void vga_init(void) {
    cursor_x = 0;
    cursor_y = 0;
    current_color = 0x0f;
    vga_clear();
}

void vga_clear(void) {
    for (int y = 0; y < VGA_HEIGHT; y++) {
        for (int x = 0; x < VGA_WIDTH; x++) {
            vga_buffer[y * VGA_WIDTH + x] = (uint16_t)(' ' | (current_color << 8));
        }
    }
    cursor_x = 0;
    cursor_y = 0;
}

void vga_set_color(uint8_t fg, uint8_t bg) {
    current_color = (bg << 4) | (fg & 0x0f);
}

void vga_putchar(char c) {
    if (c == '\n' || c == '\r') {
        cursor_x = 0;
        cursor_y++;
        if (cursor_y >= VGA_HEIGHT) {
            cursor_y = VGA_HEIGHT - 1;
            for (int y = 0; y < VGA_HEIGHT - 1; y++) {
                for (int x = 0; x < VGA_WIDTH; x++) {
                    vga_buffer[y * VGA_WIDTH + x] = vga_buffer[(y + 1) * VGA_WIDTH + x];
                }
            }
            for (int x = 0; x < VGA_WIDTH; x++) {
                vga_buffer[(VGA_HEIGHT - 1) * VGA_WIDTH + x] = (uint16_t)(' ' | (current_color << 8));
            }
        }
        return;
    }
    
    vga_buffer[cursor_y * VGA_WIDTH + cursor_x] = (uint16_t)(c | (current_color << 8));
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
