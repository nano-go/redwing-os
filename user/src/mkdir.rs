#![no_std]
#![no_main]

use rw_ulib::{env, fs};

extern crate alloc;

#[no_mangle]
pub fn main() {
    let args = env::args();
    let mut iter = args.iter();

    let recursive = args.first().is_some_and(|arg| *arg == "-p");
    if recursive {
        iter.next();
    }

    for arg in iter {
        if recursive {
            fs::create_dir_all(*arg).unwrap();
        } else {
            fs::create_dir(*arg).unwrap();
        }
    }
}
