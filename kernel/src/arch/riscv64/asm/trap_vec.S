.macro SAVE_REGS base
	sd ra, 0(\base)
  sd sp, 8(\base)
  sd gp, 16(\base)
  sd tp, 24(\base)
  sd t0, 32(\base)
  sd t1, 40(\base)
  sd t2, 48(\base)
  sd t3, 56(\base)
  sd t4, 64(\base)
  sd t5, 72(\base)
  sd t6, 80(\base)
  sd a0, 88(\base)
  sd a1, 96(\base)
  sd a2, 104(\base)
  sd a3, 112(\base)
  sd a4, 120(\base)
  sd a5, 128(\base)
  sd a6, 136(\base)
  sd a7, 144(\base)
  sd s0, 152(\base)
  sd s1, 160(\base)
  sd s2, 168(\base)
  sd s3, 176(\base)
  sd s4, 184(\base)
  sd s5, 192(\base)
  sd s6, 200(\base)
  sd s7, 208(\base)
  sd s8, 216(\base)
  sd s9, 224(\base)
  sd s10, 232(\base)
  sd s11, 240(\base)
.endm

.macro LOAD_REGS base
 	ld ra, 0(\base)
  ld sp, 8(\base)
  ld gp, 16(\base)
	# ld tp, 24(\base)
  ld t0, 32(\base)
  ld t1, 40(\base)
  ld t2, 48(\base)
  ld t3, 56(\base)
  ld t4, 64(\base)
  ld t5, 72(\base)
  ld t6, 80(\base)
	# ld a0, 88(\base)
  ld a1, 96(\base)
  ld a2, 104(\base)
  ld a3, 112(\base)
  ld a4, 120(\base)
  ld a5, 128(\base)
  ld a6, 136(\base)
  ld a7, 144(\base)
  ld s0, 152(\base)
  ld s1, 160(\base)
  ld s2, 168(\base)
  ld s3, 176(\base)
  ld s4, 184(\base)
  ld s5, 192(\base)
  ld s6, 200(\base)
  ld s7, 208(\base)
  ld s8, 216(\base)
  ld s9, 224(\base)
  ld s10, 232(\base)
  ld s11, 240(\base)
.endm


.balign 4
.global _kernel_trap_vec
_kernel_trap_vec:
	# Reserved for trapframe.
	addi sp, sp, -256

	SAVE_REGS sp

	call kernel_trap_handler

	# Restore the registers from trapframe but a0
	mv a0, sp
	LOAD_REGS a0
	addi sp, sp, 256

	# Restore a0 
	ld a0, 88(a0)

	sret

.extern TASK_TRAPFRAME_BASE
.extern user_trap_handler 

.balign 4
.global _user_trap_vec
_user_trap_vec:
	csrw sscratch, a0            # sscratch = a0
	ld a0, (TASK_TRAPFRAME_BASE) # a0 = TASK_TRAPFRAME_BASE
	
	SAVE_REGS a0               # Save registers to trapframe of the current task

	# Save a0(sscratch) to the trapframe->a0
	csrr t0, sscratch          # t0 = sscratch(saved orgin a0)
	sd t0, 88(a0)              # trapframe->a0 = t0

	# Load kernel stack from trapframe
	ld sp, 248(a0)             # sp = trapframe->kernel_stack

	# Make sure tp holds the hart ID.
	ld tp, 256(a0)             # tp = trapframe->hartid

	call user_trap_handler


.balign 4
.global _user_trap_ret
_user_trap_ret:

# sfence.vma zero, zero
# csrw satp, a0
# sfence.vma zero, zero

	# a0 is the argument of _user_trap_ret that points a trapframe.
	# Restore registers from the trapframe excluding 'a0' register.
	LOAD_REGS a0

	# Restore a0 
	ld a0, 88(a0)

	sret
