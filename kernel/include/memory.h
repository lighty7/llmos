#ifndef MEMORY_H
#define MEMORY_H

#include <stdint.h>
#include <stddef.h>

#define PAGE_SIZE 4096
#define KERNEL_VIRT_BASE 0xFFFFFFFF80000000

typedef struct {
    uint64_t total_pages;
    uint64_t used_pages;
    uint64_t free_pages;
} memory_info_t;

void memory_init(void);
void *kmalloc(size_t size);
void kfree(void *ptr);
memory_info_t get_memory_info(void);

#endif
