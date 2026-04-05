use core::arch::global_asm;

global_asm!(include_str!("./entry.S"));
global_asm!(include_str!("./trap_vec.S"));
global_asm!(include_str!("./switch.S"));
