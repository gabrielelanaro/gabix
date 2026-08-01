ENTRY(_start)

SECTIONS
{
    . = 0x41000000;
    .user_header : { KEEP(*(.user_header)) }

    . = 0x41000020;
    .text : {
        KEEP(*(.text._start))
        *(.text*)
    }
    .rodata : { *(.rodata*) }
    .data : { *(.data*) }
    .bss (NOLOAD) : { *(.bss*) *(COMMON) }

    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment*) }
}
