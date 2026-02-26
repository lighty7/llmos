#include "../include/kernel.h"
#include "../include/vga.h"
#include "../include/keyboard.h"
#include "../include/memory.h"
#include "../include/fs.h"
#include "../include/syscall.h"
#include <stdarg.h>

void kernel_main(void) {
    vga_init();
    memory_init();
    keyboard_init();
    fs_init();
    syscall_init();
    
    vga_clear();
    vga_set_color(VGA_COLOR_LIGHT_GREEN, VGA_COLOR_BLACK);
    vga_write("=================================================\r\n");
    vga_write("           llmos v0.1 - LLM Integrated OS       \r\n");
    vga_write("=================================================\r\n");
    vga_write("\r\n");
    vga_write("Type 'help' for available commands.\r\n");
    vga_write("\r\n");
    vga_set_color(VGA_COLOR_WHITE, VGA_COLOR_BLACK);
    
    char input_buffer[256];
    int pos = 0;
    
    while (1) {
        vga_set_color(VGA_COLOR_LIGHT_CYAN, VGA_COLOR_BLACK);
        vga_write("llmos> ");
        vga_set_color(VGA_COLOR_WHITE, VGA_COLOR_BLACK);
        
        pos = 0;
        input_buffer[0] = 0;
        
        while (1) {
            if (keyboard_has_char()) {
                char c = keyboard_getchar();
                
                if (c == '\n' || c == '\r') {
                    vga_write("\r\n");
                    input_buffer[pos] = 0;
                    break;
                } else if (c == '\b') {
                    if (pos > 0) {
                        pos--;
                        vga_write("\b \b");
                    }
                } else if (c >= 32 && c <= 126) {
                    if (pos < 255) {
                        input_buffer[pos++] = c;
                        char s[2] = {c, 0};
                        vga_write(s);
                    }
                }
            }
        }
        
        if (pos == 0) continue;
        
        if (strncmp(input_buffer, "help", 4) == 0) {
            vga_set_color(VGA_COLOR_YELLOW, VGA_COLOR_BLACK);
            vga_write("Available commands:\r\n");
            vga_write("  help           - Show this help\r\n");
            vga_write("  clear          - Clear screen\r\n");
            vga_write("  ls             - List files\r\n");
            vga_write("  cat <file>     - Read file\r\n");
            vga_write("  echo <text>    - Create/write to file\r\n");
            vga_write("  rm <file>      - Delete file\r\n");
            vga_write("  exit           - Shutdown\r\n");
            vga_write("  reboot         - Reboot system\r\n");
            vga_set_color(VGA_COLOR_WHITE, VGA_COLOR_BLACK);
        }
        else if (strncmp(input_buffer, "clear", 5) == 0) {
            vga_clear();
        }
        else if (strncmp(input_buffer, "ls", 2) == 0) {
            fs_list();
        }
        else if (strncmp(input_buffer, "exit", 4) == 0) {
            vga_write("System halted.\r\n");
            while (1) __asm__ volatile ("hlt");
        }
        else if (strncmp(input_buffer, "reboot", 6) == 0) {
            __asm__ volatile ("outb %%al, $0x64" : : "a"(0xFE));
            while (1) __asm__ volatile ("hlt");
        }
        else if (strncmp(input_buffer, "echo ", 5) == 0) {
            char *filename = input_buffer + 5;
            char *content = strchr(filename, ' ');
            if (content) {
                *content = 0;
                content++;
                file_t *f = fs_create(filename);
                if (f) {
                    fs_write(f, content, strlen(content));
                    vga_write("File created/updated.\r\n");
                } else {
                    vga_write("Error creating file.\r\n");
                }
            } else {
                vga_write("Usage: echo <filename> <content>\r\n");
            }
        }
        else if (strncmp(input_buffer, "cat ", 4) == 0) {
            char *filename = input_buffer + 4;
            file_t *f = fs_open(filename);
            if (f && f->used) {
                vga_write((char*)f->data);
                vga_write("\r\n");
            } else {
                vga_write("File not found.\r\n");
            }
        }
        else if (strncmp(input_buffer, "rm ", 3) == 0) {
            char *filename = input_buffer + 3;
            if (fs_delete(filename)) {
                vga_write("File deleted.\r\n");
            } else {
                vga_write("File not found.\r\n");
            }
        }
        else {
            vga_write("Unknown command. Type 'help' for commands.\r\n");
        }
    }
}

int strncmp(const char *s1, const char *s2, int n) {
    for (int i = 0; i < n; i++) {
        if (s1[i] != s2[i]) return s1[i] - s2[i];
        if (s1[i] == 0) return 0;
    }
    return 0;
}

int strlen(const char *s) {
    int len = 0;
    while (s[len]) len++;
    return len;
}

char *strchr(char *s, int c) {
    while (*s) {
        if (*s == c) return s;
        s++;
    }
    return 0;
}

void printf(const char *format, ...) {
    char buffer[256];
    va_list args;
    va_start(args, format);
    
    int i = 0;
    const char *p = format;
    while (*p && i < 255) {
        if (*p == '%' && *(p+1) == 's') {
            char *s = va_arg(args, char*);
            while (*s && i < 255) buffer[i++] = *s++;
            p += 2;
        } else if (*p == '%' && *(p+1) == 'd') {
            int d = va_arg(args, int);
            char num[32];
            int j = 0;
            if (d == 0) {
                buffer[i++] = '0';
            } else {
                if (d < 0) {
                    buffer[i++] = '-';
                    d = -d;
                }
                while (d > 0) {
                    num[j++] = '0' + (d % 10);
                    d /= 10;
                }
                while (j > 0) buffer[i++] = num[--j];
            }
            p += 2;
        } else {
            buffer[i++] = *p++;
        }
    }
    buffer[i] = 0;
    va_end(args);
    vga_write(buffer);
}

void panic(const char *msg) {
    vga_set_color(VGA_COLOR_RED, VGA_COLOR_BLACK);
    vga_write("PANIC: ");
    vga_write(msg);
    vga_write("\r\n");
    while (1) __asm__ volatile ("hlt");
}
