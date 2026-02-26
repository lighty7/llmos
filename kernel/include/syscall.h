#ifndef SYSCALL_H
#define SYSCALL_H

#include <stdint.h>

#define SYSCALL_WRITE 1
#define SYSCALL_READ 2
#define SYSCALL_OPEN 3
#define SYSCALL_CLOSE 4
#define SYSCALL_CREATE 5
#define SYSCALL_DELETE 6
#define SYSCALL_LIST 7
#define SYSCALL_EXIT 8
#define SYSCALL_GETMEM 9
#define SYSCALL_CLEAR 10

typedef struct {
    uint64_t rax;
    uint64_t rdi;
    uint64_t rsi;
    uint64_t rdx;
    uint64_t r10;
    uint64_t r8;
    uint64_t r9;
} syscall_args_t;

void syscall_init(void);
uint64_t syscall_handle(uint64_t nr, uint64_t arg1, uint64_t arg2, uint64_t arg3);

#endif
