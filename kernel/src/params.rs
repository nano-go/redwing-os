use core::time::Duration;

use crate::mmu::PGSIZE;

#[no_mangle]
pub static MAX_NCPU: usize = 8;

pub const MAX_TASKS: usize = 128;
pub const TASK_KERNEL_STACK_SIZE: usize = PGSIZE * 8;
pub const TASK_USER_STACK_SIZE: usize = PGSIZE * 32;

pub const KERNEL_HEAP_SIZE: usize = 16 * 1024 * 1024;
pub const TIMER_FREQ_HZ: usize = 100;

pub const TICK_TIME: u64 = 1000 / TIMER_FREQ_HZ as u64;
pub const TICK_TIME_DUR: Duration = Duration::from_millis(TICK_TIME);

#[no_mangle]
pub static KERNEL_STACK_SIZE_PER_CPU: usize = PGSIZE * 8;

#[rustfmt::skip]
pub const WELCOME_MSG: &str = r#" 
    __  __     ____           ____           __         _            
   / / / /__  / / /___       / __ \___  ____/ /      __(_)___  ____ _
  / /_/ / _ \/ / / __ \     / /_/ / _ \/ __  / | /| / / / __ \/ __ `/
 / __  /  __/ / / /_/ /    / _, _/  __/ /_/ /| |/ |/ / / / / / /_/ / 
/_/ /_/\___/_/_/\____/    /_/ |_|\___/\__,_/ |__/|__/_/_/ /_/\__, /  
                                                            /____/    
"#;
