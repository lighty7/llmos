#ifndef KERNEL_H
#define KERNEL_H

#include <stdint.h>
#include <stddef.h>
#include <stdarg.h>

void kernel_main(void);

void panic(const char *msg);
void printf(const char *format, ...);

int strncmp(const char *s1, const char *s2, int n);
int strlen(const char *s);
char *strchr(char *s, int c);

#endif
