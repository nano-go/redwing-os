OUTPUT_ARCH( "riscv" )
ENTRY( _entry )

SECTIONS {
	. = 0x80000000;


	.text : {
		*(.text.entry)
		*(.text .text.*)
		. = ALIGN(0x1000);
		PROVIDE(text_end = .);
	}

	.rodata : {
		. = ALIGN(0x1000);
		PROVIDE(rodata_begin = .);
	  *(.rodata .rodata.*)
		. = ALIGN(0x1000);
		PROVIDE(rodata_end = .);
	}

	.data : {
		. = ALIGN(0x1000);
		PROVIDE(data_begin = .);
	  *(.data .data.*)
		. = ALIGN(0x1000);
		PROVIDE(data_end = .);
	}

	.bss : {
		. = ALIGN(0x1000);
		PROVIDE(bss_begin = .);
	  *(.bss .bss.*)
		. = ALIGN(0x1000);
		PROVIDE(bss_end = .);
	}

	PROVIDE(kernel_end = .);
}
