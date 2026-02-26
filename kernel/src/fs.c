#include "../include/fs.h"
#include "../include/vga.h"

static filesystem_t filesystem;

static int fs_strcmp(const char *s1, const char *s2);
static void fs_strncpy(char *dest, const char *src, int n);
static void fs_int_to_str(int value, char *buffer);

void fs_init(void) {
    for (int i = 0; i < FS_MAX_FILES; i++) {
        filesystem.files[i].used = 0;
        filesystem.files[i].name[0] = 0;
        filesystem.files[i].size = 0;
    }
    filesystem.file_count = 0;
}

file_t *fs_create(const char *filename) {
    if (filesystem.file_count >= FS_MAX_FILES) return 0;
    
    for (int i = 0; i < FS_MAX_FILES; i++) {
        if (filesystem.files[i].used && fs_strcmp(filesystem.files[i].name, filename) == 0) {
            filesystem.files[i].size = 0;
            return &filesystem.files[i];
        }
    }
    
    for (int i = 0; i < FS_MAX_FILES; i++) {
        if (!filesystem.files[i].used) {
            filesystem.files[i].used = 1;
            fs_strncpy(filesystem.files[i].name, filename, FS_MAX_FILENAME - 1);
            filesystem.files[i].name[FS_MAX_FILENAME - 1] = 0;
            filesystem.files[i].size = 0;
            filesystem.file_count++;
            return &filesystem.files[i];
        }
    }
    
    return 0;
}

file_t *fs_open(const char *filename) {
    for (int i = 0; i < FS_MAX_FILES; i++) {
        if (filesystem.files[i].used && fs_strcmp(filesystem.files[i].name, filename) == 0) {
            return &filesystem.files[i];
        }
    }
    return 0;
}

int fs_write(file_t *file, const void *data, uint32_t size) {
    if (!file || !file->used) return 0;
    if (size > FS_FILE_SIZE) size = FS_FILE_SIZE;
    
    for (uint32_t i = 0; i < size; i++) {
        file->data[i] = ((const char *)data)[i];
    }
    file->size = size;
    return size;
}

int fs_read(file_t *file, void *data, uint32_t size) {
    if (!file || !file->used) return 0;
    if (size > file->size) size = file->size;
    
    for (uint32_t i = 0; i < size; i++) {
        ((char *)data)[i] = file->data[i];
    }
    return size;
}

int fs_delete(const char *filename) {
    for (int i = 0; i < FS_MAX_FILES; i++) {
        if (filesystem.files[i].used && fs_strcmp(filesystem.files[i].name, filename) == 0) {
            filesystem.files[i].used = 0;
            filesystem.files[i].name[0] = 0;
            filesystem.files[i].size = 0;
            filesystem.file_count--;
            return 1;
        }
    }
    return 0;
}

void fs_list(void) {
    vga_set_color(VGA_COLOR_YELLOW, VGA_COLOR_BLACK);
    vga_write("Files:\r\n");
    vga_set_color(VGA_COLOR_WHITE, VGA_COLOR_BLACK);
    
    if (filesystem.file_count == 0) {
        vga_write("  (no files)\r\n");
        return;
    }
    
    for (int i = 0; i < FS_MAX_FILES; i++) {
        if (filesystem.files[i].used) {
            vga_write("  ");
            vga_write(filesystem.files[i].name);
            vga_write(" (");
            char size_str[16];
            fs_int_to_str(filesystem.files[i].size, size_str);
            vga_write(size_str);
            vga_write(" bytes)\r\n");
        }
    }
}

static int fs_strcmp(const char *s1, const char *s2) {
    while (*s1 && *s2) {
        if (*s1 != *s2) return *s1 - *s2;
        s1++;
        s2++;
    }
    return *s1 - *s2;
}

static void fs_strncpy(char *dest, const char *src, int n) {
    int i = 0;
    while (src[i] && i < n) {
        dest[i] = src[i];
        i++;
    }
    while (i < n) {
        dest[i] = 0;
        i++;
    }
}

static void fs_int_to_str(int value, char *buffer) {
    if (value == 0) {
        buffer[0] = '0';
        buffer[1] = 0;
        return;
    }
    
    char temp[32];
    int i = 0;
    while (value > 0) {
        temp[i++] = '0' + (value % 10);
        value /= 10;
    }
    
    int j = 0;
    while (i > 0) {
        buffer[j++] = temp[--i];
    }
    buffer[j] = 0;
}
