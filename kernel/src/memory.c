#include "../include/memory.h"

#define MEMORY_START 0x100000
#define MEMORY_END 0x10000000

static char memory_bitmap[32768];
static uint64_t total_pages = 0;
static uint64_t used_pages = 0;

void memory_init(void) {
    total_pages = (MEMORY_END - MEMORY_START) / PAGE_SIZE;
    used_pages = 0;
    for (int i = 0; i < 32768; i++) {
        memory_bitmap[i] = 0;
    }
}

void *kmalloc(size_t size) {
    if (size == 0) return 0;
    
    size_t pages_needed = (size + PAGE_SIZE - 1) / PAGE_SIZE;
    
    for (size_t i = 0; i < total_pages - pages_needed; i++) {
        int can_alloc = 1;
        for (size_t j = 0; j < pages_needed; j++) {
            if (memory_bitmap[(i + j) / 8] & (1 << ((i + j) % 8))) {
                can_alloc = 0;
                break;
            }
        }
        
        if (can_alloc) {
            for (size_t j = 0; j < pages_needed; j++) {
                memory_bitmap[(i + j) / 8] |= (1 << ((i + j) % 8));
            }
            used_pages += pages_needed;
            return (void *)(MEMORY_START + i * PAGE_SIZE);
        }
    }
    
    return 0;
}

void kfree(void *ptr) {
    if (ptr < (void *)MEMORY_START || ptr >= (void *)MEMORY_END) return;
    
    uint64_t page_num = ((uint64_t)ptr - MEMORY_START) / PAGE_SIZE;
    
    if (memory_bitmap[page_num / 8] & (1 << (page_num % 8))) {
        memory_bitmap[page_num / 8] &= ~(1 << (page_num % 8));
        used_pages--;
    }
}

memory_info_t get_memory_info(void) {
    memory_info_t info;
    info.total_pages = total_pages;
    info.used_pages = used_pages;
    info.free_pages = total_pages - used_pages;
    return info;
}
