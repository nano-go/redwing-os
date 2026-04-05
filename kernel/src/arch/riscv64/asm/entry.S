.attribute arch, "rv64gc"

.section .text.entry 
.global _entry
_entry:
	# Initialize the stack pointer for each CPU.

	# The KERNEL_STACK is defined in 'params.rs', pointer to the 
  # top of the kernel stack.
	la sp, KERNEL_STACK # sp = KERNEL_STACK

	# Read the mhartid(Machine Hardware Thread Id) to the a0 register.
	# The ID is used to indicate the position of its stack.
	csrr a0, mhartid

	# The KSTACK_SIZE_PER_CPU is defined 'params.rs'.
	ld a1, (KERNEL_STACK_SIZE_PER_CPU) # a1 = KSTACK_SIZE_PER_CPU

	# a0 = (ID + 1) * KSTACK_SIZE_PER_CPU
	addi a0, a0, 1
	mul a0, a0, a1

	add sp, sp, a0

	call arch_init
