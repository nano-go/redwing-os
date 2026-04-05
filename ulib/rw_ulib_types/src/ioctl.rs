use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(Debug, Clone, Copy, IntoPrimitive, TryFromPrimitive)]
#[repr(u64)]
pub enum Request {
    TIOCGPGRP = 0x540F,
    TIOCSPGRP = 0x5410,
}
