OUTPUT_ARCH( "riscv" )
ENTRY( _start )


SECTIONS
{
 . = 0x200080000;

  .text : {
		*(.text.entry)
    *(.text) *(.text.*)
    *(.text .text.*)
    . = ALIGN(0x1000);
  }

  .rodata : {
    . = ALIGN(16);
    *(.srodata .srodata.*) /* do not need to distinguish this from .rodata */
    . = ALIGN(16);
    *(.rodata .rodata.*)
    . = ALIGN(0x1000);
  }

  .eh_frame : {
    *(.eh_frame .eh_frame.*)
    . = ALIGN(0x1000);
  }

  .data : {
    . = ALIGN(16);
    *(.sdata .sdata.*) /* do not need to distinguish this from .data */
    . = ALIGN(16);
    *(.data .data.*)
    . = ALIGN(0x1000);
  }

  .bss : {
    . = ALIGN(16);
    *(.sbss .sbss.*) /* do not need to distinguish this from .bss */
    . = ALIGN(16);
    *(.bss .bss.*)
  }

  PROVIDE(end = .);
}
