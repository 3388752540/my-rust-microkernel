ENTRY(_start)

SECTIONS {
    /* bootloader 0.9.x 期望内核加载在 2MB 处 */
    . = 0x200000;

    .text : {
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    .bss : {
        *(.bss .bss.*)
    }
}
