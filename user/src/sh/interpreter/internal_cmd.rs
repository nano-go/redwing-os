use alloc::{
    collections::btree_map::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use lazy_static::lazy_static;
use rw_ulib::env;

use super::{
    exec_err,
    utils::{canonical_path, is_identifier},
    Environment, Result,
};

pub type InternalCmd = fn(env: &mut Environment, args: Vec<String>) -> Result<()>;

lazy_static! {
    static ref INTERNAL_CMD_TABLE: BTreeMap<&'static str, InternalCmd> = {
        let mut table = BTreeMap::new();
        table.insert("cd", exec_cd as InternalCmd);
        table.insert("export", exec_export as InternalCmd);
        table
    };
}

pub fn get_cmd(name: &str) -> Option<InternalCmd> {
    INTERNAL_CMD_TABLE.get(name).cloned()
}

fn exec_cd(_env: &mut super::Environment, args: Vec<String>) -> Result<()> {
    let dir_path = args.first().map(String::as_str).unwrap_or("./");
    let result = env::change_cur_dir(dir_path);
    if let Err(err) = result {
        return Err(exec_err!("cd: {err} '{dir_path}'"));
    }

    let mut pwd = env::var("PWD").unwrap_or_else(|| "/".to_string());

    pwd.push('/');
    pwd.push_str(&*dir_path);
    pwd = canonical_path(&pwd);

    env::set_var("PWD", pwd);
    Ok(())
}

fn exec_export(_env: &mut Environment, args: Vec<String>) -> Result<()> {
    for arg in args {
        let mut kv_iter = arg.splitn(2, '=');
        let key = kv_iter.next().unwrap();
        if !is_identifier(key) {
            return Err(exec_err!("export: invalid key"));
        }
        let value = kv_iter.next().unwrap_or("");
        if value.contains("\0") {
            return Err(exec_err!("export: invalid value"));
        }
        env::set_var(key, value);
    }
    Ok(())
}
