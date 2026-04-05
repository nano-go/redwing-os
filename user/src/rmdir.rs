#![no_std]
#![no_main]

use rw_ulib::{env, fs, println};

#[no_mangle]
pub fn main() {
    let args = env::args();

    for arg in args {
        if let Err(err) = fs::remove_dir(arg) {
            println!("{err}");
        }
    }
}
