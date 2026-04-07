ENTRY(_start)
SECTIONS
{
    . = 0x400000;
    .text : { *(.text) }
    . = ALIGN(4096);
    .data : { *(.data) }
    .bss  : { *(.bss)  *(COMMON) }
    /DISCARD/ : { *(.comment) *(.note*) }
}