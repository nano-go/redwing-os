#![no_std]
#![no_main]

use rw_ulib::{env, error::Result, fs, io::stdin, print, println, process};

extern crate alloc;

#[no_mangle]
pub fn main() {
    let args = env::args();

    if args.is_empty() {
        handle_error(cat_from_stdin(), "<standard input>");
        return;
    }

    for arg in args {
        handle_error(cat(arg), arg);
    }
}

fn cat(path: &str) -> Result<()> {
    let bytes = fs::read(path)?;
    match str::from_utf8(&bytes) {
        Ok(content) => print!("{content}"),
        Err(_) => {
            println!("cat: invalid utf-8");
            process::exit(1);
        }
    }
    Ok(())
}

fn cat_from_stdin() -> Result<()> {
    loop {
        let line = stdin().read_line()?;
        if line.is_empty() {
            break;
        }
        print!("{line}");
    }
    Ok(())
}

fn handle_error<T>(result: Result<T>, path: &str) -> T {
    match result {
        Ok(val) => val,
        Err(err) => {
            println!("error: {} '{}'", err, path);
            process::exit(1);
        }
    }
}
