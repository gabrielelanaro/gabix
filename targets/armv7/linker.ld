ENTRY(_start)

SECTIONS
{
    . = KERNEL_BASE;

    .text : {
        KEEP(*(.text.boot))
        *(.text*)
    }

    .rodata : { *(.rodata*) }
    .data : { *(.data*) }
    . = ALIGN(16);
    .bss (NOLOAD) : {
        __bss_start = .;
        *(.bss*)
        *(COMMON)
        __bss_end = .;
    }

    /* Only the Wi-Fi development build contributes this RAM-only section. */
    .probe_ram 0x00e00000 (NOLOAD) : { *(.probe_ram) }

    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment*) }
}
