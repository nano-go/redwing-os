# Redwing OS

A Unix-like operating system written in Rust for RISC-V architecture.

## Overview

Redwing OS is an educational operating system targeting the RISC-V 64-bit architecture. It features a monolithic kernel with preemptive multitasking, virtual memory, a custom filesystem, and a collection of standard Unix-like user programs.

## Features

- **Architecture**: RISC-V 64-bit (riscv64)
- **Language**: Rust (requires nightly toolchain)
- **Kernel Type**: Monolithic with modular design
- **Memory Management**: Virtual memory with page tables, buddy allocator, and slab allocator
- **Scheduling**: CFS (Completely Fair Scheduler) with support for process priorities
- **Filesystem**: Custom VFS layer with Redwing EFS (Extent-based File System) and RAM filesystem support
- **User Environment**: Unix-like shell and standard utilities

## Project Structure

```
redwing_os/
├── kernel/          # Kernel source code
│   ├── src/arch/riscv64/   # RISC-V specific code (boot, traps, MMU)
│   ├── src/proc/           # Process management, scheduling, signals
│   ├── src/mmu/            # Memory management (VM, buddy, slab allocators)
│   ├── src/fs/             # VFS and filesystem implementations
│   ├── src/drivers/        # Device drivers (UART, VirtIO block)
│   ├── src/syscall/        # System call handlers
│   └── src/sync/           # Synchronization primitives (spinlocks, condition variables)
├── fs/              # Filesystem implementations
│   ├── redwing_vfs/        # Virtual File System layer
│   ├── redwing_efs/        # Extent-based File System
│   └── redwing_ram/        # RAM filesystem
├── ulib/            # User-space libraries
│   ├── rw_ulib/            # Main user library (no_std, syscall interface)
│   └── rw_ulib_types/      # Shared types between kernel and userspace
├── user/            # User programs
│   ├── cat, echo, ls, ps   # Standard Unix utilities
│   ├── sh/                 # Shell with scripting support
│   └── ...
├── common/          # Shared libraries
│   ├── syserr/             # Error codes
│   ├── path/               # Path manipulation
│   └── ...
└── mkfs/            # Filesystem image creation tool
```

## Building

### Prerequisites

- Rust nightly toolchain
- QEMU with RISC-V support (for emulation)
- RISC-V GNU toolchain (for linking)

Adds riscv64 target:

```
rustup target add riscv64gc-unknown-none-elf
```

### Build Instructions

```bash
# Build user programs and create filesystem image
make

# Build kernel only
cd kernel && cargo build --release

# Build with debug symbols
cd kernel && cargo build
```

## Running

The kernel is designed to run on QEMU RISC-V:

```bash
cd kernel & cargo run --release
```

or

```bash
qemu-system-riscv64
		-M virt
		-bios none
		-m 4G
		-smp 4
		-global virtio-mmio.force-legacy=false
		-nographic
		-drive file=../mkfs/fs.img,if=none,format=raw,id=x0
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0
		-kernel
```

## User Programs

The OS includes a basic set of Unix-like utilities:

- **sh** - Command shell with support for pipes, redirections, and scripting
- **cat** - Concatenate and display files
- **echo** - Print arguments to stdout
- **ls** - List directory contents
- **ps** - Display process status
- **kill** - Send signals to processes
- **mkdir** - Create directories
- **rm/rmdir** - Remove files and directories
- **env** - Display environment variables

## Kernel Components

### Memory Management

- **Buddy Allocator**: Physical page frame allocation
- **Slab Allocator**: Efficient kernel object allocation
- **Virtual Memory**: Per-process address spaces with page table management

### Process Management

- Preemptive multitasking with time slicing
- Process groups and sessions
- POSIX-style signals
- Fork/exec process model
- Pipe and I/O redirection support

### Filesystem

- **VFS Layer**: Abstract interface for multiple filesystems
- **Redwing EFS**: Extent-based file system with journaling
- **Proc FS**: Process information filesystem
- **Device Files**: Special files for device access

### Drivers

- **UART**: Serial console I/O
- **VirtIO Block**: Disk storage interface
- **PLIC**: Platform-Level Interrupt Controller support

## Architecture

The kernel is built with `#![no_std]` and uses Rust's type system for safety:

- **Spinlocks**: Basic synchronization primitive
- **Sleep locks**: Mutexes that yield the CPU
- **Condition Variables**: For blocking synchronization
- **IRQ-safe structures**: For interrupt context operations

## Testing

The kernel includes a test framework:

```bash
# Run kernel tests
cd kernel && cargo test
```

## License

Apache License 2.0

## Acknowledgments

This is an educational project for learning operating system concepts and RISC-V architecture.
