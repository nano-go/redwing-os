#![no_std]
#![no_main]

use rw_ulib::{env, print};

#[no_mangle]
pub fn main() {
    let args = env::args();
    print!("{}", args.join(" "));
}
