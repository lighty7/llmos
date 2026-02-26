#ifndef FS_H
#define FS_H

#include <stdint.h>

#define FS_MAX_FILES 32
#define FS_MAX_FILENAME 64
#define FS_FILE_SIZE 4096
#define FS_SECTOR_SIZE 512

typedef struct {
    char name[FS_MAX_FILENAME];
    uint32_t size;
    uint8_t data[FS_FILE_SIZE];
    int used;
} file_t;

typedef struct {
    file_t files[FS_MAX_FILES];
    int file_count;
} filesystem_t;

void fs_init(void);
file_t *fs_create(const char *filename);
file_t *fs_open(const char *filename);
int fs_write(file_t *file, const void *data, uint32_t size);
int fs_read(file_t *file, void *data, uint32_t size);
int fs_delete(const char *filename);
void fs_list(void);

#endif
