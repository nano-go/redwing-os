#![no_std]
#![no_main]

use rw_ulib::{println, process, signal::sigaction};
use rw_ulib_types::signal::{Signal, SignalAction, SignalFlags};

#[no_mangle]
pub fn main() {
    fn custom_action(_signal: u32) {
        println!("receive SIGINT signal");
        process::exit(0);
    }

    let action = SignalAction {
        sig_handler: custom_action,
        mask: SignalFlags::empty(),
    };
    sigaction(Signal::SIGINT, &action, None).unwrap();

    println!("wait for Ctrl-C");
    loop {
        process::yield_now().unwrap();
    }
}
