#!/usr/bin/python3

from tabulate import tabulate

def format_hex(addr):
    hex_str = f"{addr:08X}"
    # Add underscores every 4 characters.
    groups = [hex_str[max(i - 4, 0):i] for i in range(len(hex_str), 0, -4)]
    return "0x" + "_".join(reversed(groups))

def format_size(size):
    units = [("GB", 1 << 30), ("MB", 1 << 20), ("KB", 1 << 10), ("B", 1)]
    for name, factor in units:
        if size >= factor:
            return f"{size // factor}{name}"
    return "0B"

def expand_table(table):
    expanded = []
    header = table[0]
    expanded.append(header)
    for start, end, size, desc in table[1:]:
        if size is None:
            size = end - start
        elif end is None:
            end = start + size
        expanded.append((
            format_hex(start),
            format_hex(end),
            format_size(size),
            desc
        ))
    return expanded



def print_ascii_table(expanded_table):
    # Use tabulate to print the table in an ASCII format
    headers = expanded_table[0]
    rows = expanded_table[1:]
    table = tabulate(rows, headers=headers, tablefmt="grid", maxcolwidths=[22, 22, 8, 40])
    print(table)

# Input
table = [
    ("Start addr", "End addr", "Area Size", "Description"),
    (0x10000000, None, 4096, "UART registers"),
    (0x10001000, None, 4096, "Virtio mmio registers"),
    (0x80000000, 0xC0000000, None, "Kernel code and data, which includes both text and rodata sections for the OS kernel"),
    (0xC0000000, None, 0x10000000, "RISC-V plic"),
    (0xD0000000, None, 128*1024*1024, "Direct mapping of all physical memory"),
    (0x200000000, None, 32*1024, "Task Kernel Stack"),
    (0x200009000, None, 4*1024, "Task TrapFrame"),
    (0x200080000, None, 512*1024*1024, "User ELF(code, data, rodata...)"),
    (0x300000000, None, 1024*1024*1024, "User stack and heap"),
]

if __name__ == "__main__":
    expanded = expand_table(table)
    print_ascii_table(expanded)
