#![no_std]
#![no_main]

use rw_ulib::{env, println, process};
use rw_ulib_types::signal::Signal;

extern crate alloc;

#[no_mangle]
pub fn main() {
    let args = env::args();
    if args.is_empty() {
        println!("kill: not enough arguments");
        process::exit(1);
    }

    for arg in args {
        if let Ok(tid) = arg.parse::<u64>() {
            kill(tid);
        } else {
            println!("kill: illegal tid: {arg}");
            process::exit(1);
        }
    }
}

fn kill(tid: u64) {
    if let Err(err) = process::kill(tid as i64, Signal::SIGILL) {
        println!("kill: {err}");
        process::exit(1);
    }
}
