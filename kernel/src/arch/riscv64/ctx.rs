use const_default::ConstDefault;

extern "C" {
    /// This is defined in `switch.S` file.
    ///
    /// This will save current callee-registers into the old context and load
    /// these from the new conext.
    ///
    /// This is used for switching to a task from another task.
    pub fn switch(old: &Context, new: &Context);
}

/// Saved register for kernel switch.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Context {
    pub ra: u64,
    pub sp: u64,

    pub s0: u64,
    pub s1: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
}

impl Context {
    #[must_use]
    #[inline]
    pub fn new(entry: u64, sp: u64) -> Self {
        Self {
            ra: entry,
            sp,
            ..Self::default()
        }
    }
}

#[derive(ConstDefault, Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct Trapframe {
    pub ra: u64,  // offset 0
    pub sp: u64,  // offset 8
    pub gp: u64,  // offset 16
    pub tp: u64,  // offset 24
    pub t0: u64,  // offset 32
    pub t1: u64,  // offset 40
    pub t2: u64,  // offset 48
    pub t3: u64,  // offset 56
    pub t4: u64,  // offset 64
    pub t5: u64,  // offset 72
    pub t6: u64,  // offset 80
    pub a0: u64,  // offset 88
    pub a1: u64,  // offset 96
    pub a2: u64,  // offset 104
    pub a3: u64,  // offset 112
    pub a4: u64,  // offset 120
    pub a5: u64,  // offset 128
    pub a6: u64,  // offset 136
    pub a7: u64,  // offset 144
    pub s0: u64,  // offset 152
    pub s1: u64,  // offset 160
    pub s2: u64,  // offset 168
    pub s3: u64,  // offset 176
    pub s4: u64,  // offset 184
    pub s5: u64,  // offset 192
    pub s6: u64,  // offset 200
    pub s7: u64,  // offset 208
    pub s8: u64,  // offset 216
    pub s9: u64,  // offset 224
    pub s10: u64, // offset 232
    pub s11: u64, // offset 240

    pub kernel_stack: u64, // offset 248
    pub hartid: u64,       // offset 256
    pub sstatus: u64,      // offset 264
    pub sepc: u64,         // offset 272
}

impl Trapframe {
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self::DEFAULT
    }

    #[inline]
    pub const fn set_return_val(&mut self, val: u64) {
        self.a0 = val;
    }

    #[must_use]
    #[inline]
    pub const fn arg0(&mut self) -> u64 {
        self.a0
    }

    #[inline]
    pub const fn set_arg0(&mut self, val: u64) {
        self.a0 = val;
    }

    #[must_use]
    #[inline]
    pub const fn arg1(&mut self) -> u64 {
        self.a1
    }

    #[inline]
    pub const fn set_arg1(&mut self, val: u64) {
        self.a1 = val;
    }

    #[must_use]
    #[inline]
    pub const fn sp(&self) -> u64 {
        self.sp
    }

    #[inline]
    pub const fn set_sp(&mut self, sp: u64) {
        self.sp = sp;
    }

    pub const fn set_ret_pc(&mut self, ret_pc: u64) {
        self.sepc = ret_pc;
    }
}
