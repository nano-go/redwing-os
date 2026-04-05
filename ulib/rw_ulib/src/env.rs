use core::{ffi::c_char, slice};

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use path::Path;
use spin::{Mutex, MutexGuard};

use crate::{
    error::{wrap_with_result, Result},
    fs,
    syscall::api::sys_cd,
    ARGS_PTR,
};

pub(crate) static ENVRON: Mutex<Environemnt> = Mutex::new(Environemnt::new());

pub(crate) unsafe fn init_env_vars(env_vars_ptr: usize) {
    ENVRON
        .lock()
        .set_vars(read_cstr_array(env_vars_ptr as *const u8));
}

/// Reads an array of C-style null-terminated strings from a raw memory pointer.
///
/// # Memory Layout Assumption
///
/// ```text
/// +--------------------------+ <- cstr_array_ptr (initial)
/// | usize (count of strings) |
/// +--------------------------+
/// | "string1\0"              |
/// +--------------------------+
/// | "string2\0"              |
/// +--------------------------+
/// | ...                      |
/// +--------------------------+
/// | "stringN\0"              |
/// +--------------------------+
/// ```
///
/// # Arguments
///
/// * `cstr_array_ptr`: A raw pointer to the beginning of the memory block
///   containing the string array structure.
///
/// # Safety
///
/// This function is highly `unsafe` and requires the caller to uphold the
/// following critical invariants to prevent undefined behavior:
///
/// 1. `cstr_array_ptr` must be a valid, aligned, and dereferenceable pointer to
///    readable memory.
/// 2. The memory region starting at `cstr_array_ptr` must contain:
///     * A valid `usize` value representing the number of strings.
///     * Immediately after the `usize`, `count` number of properly
///       null-terminated C strings.
/// 3. Each C string (including its null terminator) must be entirely contained
///    within a valid, accessible, and readable memory region. Reading past
///    allocated bounds will lead to undefined behavior.
/// 4. The memory backing these strings must outlive the `Vec` and all `&'static
///    [u8]` slices contained within it (i.e., it must truly have a `'static`
///    lifetime).
unsafe fn read_cstr_array(mut cstr_array_ptr: *const u8) -> Vec<&'static [u8]> {
    let mut cstrs = Vec::<&[u8]>::new();

    // Read the total number of C strings from the beginning of the array.
    // `read_volatile` is used to ensure the read is not optimized away,
    // which is important for memory that might be concurrently modified.
    let cstrs_len = (cstr_array_ptr as *const usize).read_volatile();
    cstr_array_ptr = cstr_array_ptr.offset(size_of::<usize>() as isize);

    for _ in 0..cstrs_len {
        unsafe extern "C" {
            /// Provided by libc or compiler_builtins. Calculates the length of
            /// a null-terminated C string.
            fn strlen(s: *const c_char) -> usize;
        }
        // Determine the length of the current C string using strlen.
        let len = strlen(cstr_array_ptr as *const c_char);

        // Create a Rust byte slice from the raw pointer and determined length.
        let slice = slice::from_raw_parts(cstr_array_ptr, len);

        // Advance the pointer past the current string and its null terminator.
        cstr_array_ptr = cstr_array_ptr.offset(len as isize + 1); // Including the NULL byte

        // Convert the byte slice to a UTF-8 Rust string slice.
        // This will panic if the bytes are not valid UTF-8.
        cstrs.push(slice);
    }

    cstrs
}

pub fn change_cur_dir<P: AsRef<Path>>(path: P) -> Result<()> {
    let c_str = fs::cstr_path(path);
    let code = sys_cd(&c_str);
    wrap_with_result(code)?;
    Ok(())
}

/// Returns a vector of process arguments.
pub fn args() -> Vec<&'static str> {
    let args_ptr = unsafe { ARGS_PTR } as *const u8;
    unsafe {
        read_cstr_array(args_ptr)
            .iter()
            .map(|slice| str::from_utf8(&slice).unwrap())
            .collect()
    }
}

pub fn vars() -> Vars {
    Vars {
        env: ENVRON.lock(),
        pos: 0,
    }
}

pub fn var<K>(key: K) -> Option<String>
where
    K: AsRef<str>,
{
    let qkey = key.as_ref();
    for (key, value) in vars() {
        if key == qkey {
            return Some(value.to_string());
        }
    }
    None
}

pub fn set_var<K, V>(key: K, value: V)
where
    K: AsRef<str>,
    V: AsRef<str>,
{
    ENVRON.lock().set_var(key.as_ref(), value.as_ref());
}

#[derive(Default)]
pub struct Environemnt {
    vars: Vec<Vec<u8>>,
}

impl Environemnt {
    #[must_use]
    const fn new() -> Self {
        Self { vars: Vec::new() }
    }

    fn set_vars(&mut self, vars: Vec<&'static [u8]>) {
        self.vars = vars.iter().map(|slice| slice.to_vec()).collect();
    }

    pub fn set_var(&mut self, key: &str, value: &str) {
        Self::check_key_and_value(key, value);

        for var in &mut self.vars {
            if let Some((k, _)) = Self::kv(&var) {
                if k == key {
                    *var = format!("{key}={value}").as_bytes().to_vec();
                    return;
                }
            }
        }

        self.vars.push(format!("{key}={value}").as_bytes().to_vec());
    }

    fn check_key_and_value(key: &str, value: &str) {
        for ch in key.chars() {
            if ch == '=' || ch == '\0' {
                panic!("envron::set_var: the key contains invalid chars.");
            }
        }

        if key.is_empty() {
            panic!("envron::set_var: the key is empty.");
        }

        for ch in value.chars() {
            if ch == '\0' {
                panic!("envron::set_var: the value contains invalid chars.");
            }
        }
    }

    fn kv(var: &[u8]) -> Option<(&str, &str)> {
        let Ok(str) = str::from_utf8(var) else {
            return None;
        };
        let mut kv_iter = str.splitn(2, '=');
        let key = kv_iter.next().unwrap();
        kv_iter.next().map(|value| (key, value))
    }
}

pub struct Vars {
    env: MutexGuard<'static, Environemnt>,
    pos: usize,
}

impl Iterator for Vars {
    type Item = (String, String);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(var) = self.env.vars.get(self.pos) {
            self.pos += 1;
            if let Some((k, v)) = Environemnt::kv(var) {
                return Some((k.to_string(), v.to_string()));
            }
        }
        None
    }
}
