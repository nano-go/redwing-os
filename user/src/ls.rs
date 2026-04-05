#![no_std]
#![no_main]

use core::fmt;

use path::Path;
use rw_ulib::{env, error::Result, fs, println, process};
use rw_ulib_types::fcntl::{Dirent, FileType, Stat};

use alloc::{format, string::ToString, vec::Vec};

extern crate alloc;

macro_rules! blue_colored {
    ($str:expr) => {
        format!("\x1b[1;34m{}\x1b[0m", $str)
    };
}

macro_rules! yellow_colored {
    ($str:expr) => {
        format!("\x1b[1;33m{}\x1b[0m", $str)
    };
}

#[no_mangle]
pub fn main() {
    let args = env::args();

    if args.is_empty() {
        handle_error(ls("./"), "./");
        return;
    }

    for arg in args {
        handle_error(ls(arg), arg);
    }
}

fn ls(path: &str) -> Result<()> {
    if fs::is_dir(path)? {
        let files = fs::read_dir(path)?
            .map(|dirent| -> Result<(Dirent, Stat)> {
                let dirent = dirent?;
                let path = format!("{path}/{}", dirent.name());
                let stat = fs::metadata(path.as_str())?;
                Ok((dirent, stat))
            })
            .collect::<Result<Vec<_>>>()?;

        let total_size = files.iter().map(|(_, stat)| stat.size).sum::<u64>();
        println!("total {}", human_size(total_size));

        for (dirent, stat) in &files {
            print_dirent(dirent.name(), stat);
        }
    } else {
        let path = Path::new(path);
        if let Some(name) = path.name() {
            print_dirent(str::from_utf8(name).unwrap(), &fs::metadata(path)?);
        } else {
            println!("ls: invalid path: {path}");
            process::exit(1);
        }
    }
    Ok(())
}

fn print_dirent(name: &str, metedata: &Stat) {
    let (typ, name) = match metedata.typ {
        FileType::RegularFile => ("-", name.to_string()),
        FileType::Directory => ("d", blue_colored!(name)),
        FileType::Device => ("D", yellow_colored!(name)),
        FileType::Symlink => ("s", name.to_string()),
    };

    println!(
        "{typ} i{:0>3} {:>4} {}",
        metedata.ino,
        format!("{}", human_size(metedata.size)),
        name
    );
}

fn handle_error<T>(result: Result<T>, path: &str) -> T {
    match result {
        Ok(val) => val,
        Err(err) => {
            println!("ls: can not access '{path}': {err}");
            process::exit(1);
        }
    }
}

pub struct HumanSize(pub u64);

impl fmt::Display for HumanSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: [&str; 7] = ["", "K", "M", "G", "T", "P", "E"];

        let mut bytes = self.0;
        let mut unit = 0;
        while bytes >= 1024 && unit < UNITS.len() - 1 {
            bytes /= 1024;
            unit += 1;
        }

        if unit == 0 {
            write!(f, "{}", bytes)
        } else {
            // compute fraction part without float.
            let div = 1024_u64.pow(unit as u32);
            let mut whole = self.0 / div;
            let frac = (self.0 % div) * 10 / div;

            if whole >= 10 || frac == 0 {
                if frac >= 5 {
                    whole += 1;
                }
                write!(f, "{}{}", whole, UNITS[unit])
            } else {
                write!(f, "{}.{}{}", whole, frac, UNITS[unit])
            }
        }
    }
}

pub fn human_size(bytes: u64) -> HumanSize {
    HumanSize(bytes)
}
