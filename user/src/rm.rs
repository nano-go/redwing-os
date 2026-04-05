#![no_std]
#![no_main]

use alloc::{format, vec::Vec};
use rw_ulib::{env, fs, println, process};

extern crate alloc;

pub struct RmArgs {
    is_recursive: bool,
    is_mute: bool, // don't need force.
    defaults: Vec<&'static str>,
}

impl RmArgs {
    pub fn parse() -> RmArgs {
        let args = env::args();

        let mut is_recursive = false;
        let mut is_mute = false;
        let mut defaults = Vec::new();

        for arg in args {
            match arg {
                "-r" | "--recursive" => {
                    is_recursive = true;
                }

                "-f" | "--force" => {
                    is_mute = true;
                }

                "-rf" => {
                    is_recursive = true;
                    is_mute = true;
                }

                arg if arg.starts_with("-") || arg.starts_with("--") => {
                    println!("unknown flag {arg}");
                    process::exit(1);
                }

                arg => defaults.push(arg),
            }
        }
        Self {
            is_recursive,
            is_mute,
            defaults,
        }
    }
}

#[no_mangle]
pub fn main() {
    let mut args = RmArgs::parse();

    for path in core::mem::take(&mut args.defaults) {
        if let Err(err) = rm(path, &args) {
            println!("rm: can not remove '{path}': {err}");
        }
    }
}

pub fn rm(file: &str, args: &RmArgs) -> rw_ulib::error::Result<()> {
    let stat = fs::metadata(file)?;
    if stat.is_dirctory() {
        if !args.is_recursive {
            println!("rm: can not remove '{file}': is a directory");
            process::exit(1);
        }
        for dirent in fs::read_dir(file)? {
            let dirent = dirent?;
            let path = format!("{file}/{}", dirent.name());
            if let Err(err) = rm(&path, args) {
                println!("rm: can not remove '{path}': {err}");
                process::exit(1);
            }
        }
        fs::remove_dir(file)?;
    } else {
        fs::remove_file(file)?;
    }
    if !args.is_mute && args.is_recursive {
        println!("removed '{file}'")
    }
    Ok(())
}
