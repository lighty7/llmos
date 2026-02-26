OUTPUT_FORMAT("binary")
ENTRY(_start)

SECTIONS {
    . = 0x7c00;
    
    .text : {
        *(.text)
        *(.text.*)
    }
    
    .rodata : {
        *(.rodata)
    }
    
    .data : {
        *(.data)
    }
    
    . = ALIGN(512);
    
    .sig : {
        LONG(0xAA55)
    }
}
