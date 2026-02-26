#ifndef KEYBOARD_H
#define KEYBOARD_H

#include <stdint.h>

#define KEYBOARD_BUFFER_SIZE 256

void keyboard_init(void);
char keyboard_getchar(void);
int keyboard_has_char(void);
void keyboard_read_buffer(char *buf, int max_len);

#endif
