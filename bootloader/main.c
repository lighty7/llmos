// llmos bootloader - C main function only

void boot_main(void);

void entry(void) {
    // Display message
    volatile unsigned short *vga = (volatile unsigned short *)0xb8000;
    
    const char *msg = "llmos v0.1 - Loading kernel...\r\n";
    int i = 0;
    
    while (msg[i]) {
        vga[i] = (unsigned short)msg[i] | 0x0F00;
        i++;
    }
    
    // Delay
    volatile long j;
    for (j = 0; j < 50000000L; j++);
    
    // Jump to kernel at 1MB
    void (*kernel_main)(void) = (void (*)(void))0x100000;
    kernel_main();
    
    while(1) __asm__ volatile ("hlt");
}
