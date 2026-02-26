#include "../include/syscall.h"
#include "../include/vga.h"
#include "../include/fs.h"

void syscall_init(void) {
}

uint64_t syscall_handle(uint64_t nr, uint64_t arg1, uint64_t arg2, uint64_t arg3) {
    switch (nr) {
        case SYSCALL_WRITE: {
            const char *str = (const char *)arg1;
            uint32_t len = (uint32_t)arg2;
            for (uint32_t i = 0; i < len && str[i]; i++) {
                vga_putchar(str[i]);
            }
            return len;
        }
        case SYSCALL_READ: {
            return 0;
        }
        case SYSCALL_CREATE: {
            const char *filename = (const char *)arg1;
            file_t *f = fs_create(filename);
            return (uint64_t)f;
        }
        case SYSCALL_OPEN: {
            const char *filename = (const char *)arg1;
            file_t *f = fs_open(filename);
            return (uint64_t)f;
        }
        case SYSCALL_CLOSE: {
            return 0;
        }
        case SYSCALL_DELETE: {
            const char *filename = (const char *)arg1;
            return fs_delete(filename);
        }
        case SYSCALL_LIST: {
            fs_list();
            return 0;
        }
        case SYSCALL_EXIT: {
            vga_write("System halted.\r\n");
            while (1) __asm__ volatile ("hlt");
        }
        case SYSCALL_CLEAR: {
            vga_clear();
            return 0;
        }
        default:
            return 0;
    }
    return 0;
}
