#![no_std]
#![no_main]

use rw_ulib::{println, process::fork};

#[no_mangle]
pub fn main() {
    for i in 0..4 {
        let tid = fork().unwrap();
        if tid.is_none() {
            loop {
                println!("user loop {i}");
            }
        }
    }
}
