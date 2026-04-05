#![no_std]
#![no_main]

use rw_ulib::{env, println};

#[no_mangle]
pub fn main() {
    for (key, value) in env::vars() {
        println!("{key}={value}")
    }
}
