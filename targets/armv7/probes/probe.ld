ENTRY(probe)
SECTIONS {
    . = 0x00e00000;
    .text : { KEEP(*(.text.probe)) *(.text*) }
    .rodata : { *(.rodata*) }
    .data : { *(.data*) *(.got*) }
    .bss (NOLOAD) : { *(.bss*) *(COMMON) }
    ASSERT(. <= 0x00e40000, "probe exceeds reserved RAM")
    ASSERT(probe == 0x00e00000, "entry must be first and in ARM state")
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment*) }
}
