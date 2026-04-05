#![no_std]
#![no_main]

use alloc::{
    collections::btree_map::BTreeMap,
    format,
    string::{String, ToString},
    vec::Vec,
};

use rw_ulib::{error::Result, fs, println};

extern crate alloc;

pub struct TaskStatus {
    tid: u64,
    tgid: u64,
    sid: u64,
    name: String,
    state: String,
}

#[no_mangle]
pub fn main() {
    if let Err(err) = ps() {
        println!("error: {err}");
    }
}

fn ps() -> Result<()> {
    println!(
        "{:<5} {:<5} {:<5} {:<12} {:<8}\n",
        "TID", "TGID", "SID", "NAME", "STATE"
    );
    for tid in list_all_tasks()? {
        let status = parse_status(tid)?;
        println!(
            "{:<5} {:<5} {:<5} {:<12} {:<8}",
            status.tid, status.tgid, status.sid, status.name, status.state
        );
    }
    Ok(())
}

fn list_all_tasks() -> Result<Vec<u64>> {
    Ok(fs::read_dir("/proc")?
        .filter_map(|dirent| dirent.ok())
        .filter_map(|dirent| dirent.name().parse::<u64>().ok())
        .collect())
}

fn parse_status(tid: u64) -> Result<TaskStatus> {
    let status = fs::read_str(format!("/proc/{tid}/status"))?;

    let mut table = BTreeMap::new();

    for line in status.lines() {
        let mut kv_iter = line.splitn(2, ":");
        let key = kv_iter.next();
        let value = kv_iter.next();
        match (key, value) {
            (Some(key), Some(value)) => table.insert(key.trim(), value.trim()),
            _ => continue,
        };
    }

    fn parse_to_u64(val: &str) -> u64 {
        val.parse::<u64>().ok().unwrap_or(0)
    }

    Ok(TaskStatus {
        tid: parse_to_u64(table["Tid"]),
        tgid: parse_to_u64(table["Tgid"]),
        sid: parse_to_u64(table["Sid"]),
        name: table["Name"].to_string(),
        state: table["State"].to_string(),
    })
}
