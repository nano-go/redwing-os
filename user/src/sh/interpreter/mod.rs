use core::fmt::Display;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use path::Path;
use rw_ulib::{
    env,
    fs::{self, File},
    io::{ioctl::tcsetpgrp, stdin, stdout},
    println,
    process::{self, get_pgid, set_pgid, Tid},
};
use rw_ulib_types::fcntl::OpenFlags;
use syserr::SysErrorKind;

use crate::{
    ast::{CmdExec, Pipe, Redirect, ShellAst, VarString},
    parser,
};

mod internal_cmd;
mod utils;

pub type Result<T, E = ExecError> = core::result::Result<T, E>;

#[derive(Debug)]
pub struct ExecError {
    msg: String,
}

impl Display for ExecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

pub macro exec_err {
    ($( $args:tt )*) => {
        ExecError { msg: alloc::format!($( $args )*) }
    }
}

pub struct Environment {
    pub status: u32,
}

impl Environment {
    pub fn new() -> Self {
        Self { status: 0 }
    }
}

fn fork() -> Result<Option<Tid>> {
    process::fork().map_err(|err| exec_err!("sh error: fail to fork a child task: {err}"))
}

fn fork_with<F>(in_background: bool, do_child: F) -> Result<u32>
where
    F: FnOnce() -> Result<()>,
{
    let tid = fork()?;
    if let Some(tid) = tid {
        let mut status = 0;
        if in_background {
            waittid(tid, &mut status)?;
        } else {
            let _ = set_pgid(tid as i64, tid as i64);
            let _ = tcsetpgrp(stdin().as_raw_fd(), tid);
            waittid(tid, &mut status)?;
            let _ = tcsetpgrp(stdin().as_raw_fd(), get_pgid(0).unwrap());
        }
        Ok(status)
    } else {
        if !in_background {
            let _ = set_pgid(0, 0);
        }
        if let Err(err) = do_child() {
            println!("{err}");
        }
        process::exit(127);
    }
}

fn fork2<F>(do_child: F) -> Result<u32>
where
    F: FnOnce(isize) -> Result<()>,
{
    let tid0 = fork()?;
    if let Some(tid0) = tid0 {
        let tid1 = fork()?;
        if let Some(tid1) = tid1 {
            do_child(0)?;
            let mut status0 = 0;
            let mut status1 = 0;
            waittid(tid0, &mut status0)?;
            waittid(tid1, &mut status1)?;
            if status0 != 0 {
                Ok(status0)
            } else {
                Ok(status1)
            }
        } else {
            if let Err(err) = do_child(1) {
                println!("{err}");
            }
            process::exit(127);
        }
    } else {
        if let Err(err) = do_child(2) {
            println!("{err}");
        }
        process::exit(127);
    }
}

fn waittid(tid: Tid, status: &mut u32) -> Result<()> {
    process::waittid(tid, status).map_err(|err| exec_err!("waittid error: {err}"))
}

fn open_file<P: AsRef<Path>>(path: P, flags: OpenFlags) -> Result<File> {
    let path = path.as_ref();
    File::with_flags(path, flags).map_err(|err| exec_err!("{err} '{path}'"))
}

#[must_use]
fn var_to_string(var: VarString) -> Option<String> {
    if var.is_var {
        env::var(var.str.to_escaped_string())
    } else {
        Some(var.str.to_string())
    }
}

#[must_use]
fn var_to_string_or_empty(var: VarString) -> String {
    var_to_string(var).unwrap_or_else(|| String::new())
}

pub fn exec_ast<'a>(env: &mut Environment, ast: ShellAst<'a>) -> Result<()> {
    match ast {
        ShellAst::Empty => {
            println!();
            Ok(())
        }
        ShellAst::List(list) => {
            for cmd in list {
                exec_ast(env, cmd)?;
            }
            Ok(())
        }
        ShellAst::Background(cmd) => exec_in_background(env, *cmd),
        ShellAst::Exec(cmd) => exec_command(env, cmd),
        ShellAst::Redirect(redir) => exec_redir(env, redir),
        ShellAst::Pipe(pipe) => exec_pipe(env, pipe),
    }
}

pub fn exec_ast_handle_error<'a>(env: &mut Environment, ast: ShellAst<'a>) -> bool {
    if let Err(err) = exec_ast(env, ast) {
        println!("{err}");
        false
    } else {
        true
    }
}

fn exec_command(env: &mut Environment, cmd: CmdExec) -> Result<()> {
    let mut args = cmd
        .args
        .into_iter()
        .filter_map(|arg| var_to_string(arg))
        .collect::<Vec<_>>();

    if args.is_empty() {
        return Ok(());
    }

    let name = args.remove(0);

    if let Some(internal_cmd) = internal_cmd::get_cmd(&name) {
        if let Err(err) = internal_cmd(env, args) {
            env.status = 1;
            println!("{err}");
        }
        return Ok(());
    }

    let status = fork_with(false, || execp(env, name, args))?;

    env.status = status;
    Ok(())
}

pub fn execp<P>(env: &mut Environment, path: P, args: Vec<String>) -> Result<()>
where
    P: AsRef<Path>,
{
    let exec_path = path.as_ref();

    if exec_path.is_absolute() || exec_path.as_bytes().starts_with(b"./") {
        return exec(env, path, args).map_err(|(err, _)| err);
    }

    let path_var = env::var("PATH").unwrap_or_else(|| "/bin".to_string());
    let dirs = path_var.split(':').chain(["./"].into_iter());

    for dir in dirs {
        let p = format!("{dir}/{exec_path}");
        let (err, next) = exec(env, p, args.clone()).unwrap_err();
        if !next {
            return Err(err);
        }
    }

    let name = str::from_utf8(exec_path.name().unwrap_or(b"")).unwrap();
    Err(exec_err!("no command {name} found"))
}

pub fn exec<P>(env: &mut Environment, path: P, args: Vec<String>) -> Result<(), (ExecError, bool)>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let name = str::from_utf8(path.name().unwrap_or(b"")).unwrap();

    let error = process::exec(&path, args).unwrap_err();
    match error {
        rw_ulib::error::Error::System(SysErrorKind::NoSuchFileOrDirectory) => {
            Err((exec_err!("no command {name} found"), true))
        }

        rw_ulib::error::Error::System(SysErrorKind::ExecFormat | SysErrorKind::NoExec) => {
            exec_shell_file(env, path).map_err(|err| (err, false))
        }

        err => Err((exec_err!("sh: {err} ({name})"), false)),
    }
}

pub fn exec_shell_file<P>(env: &mut Environment, path: P) -> Result<()>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let content = fs::read_str(path).map_err(|err| exec_err!("sh: read file error: {err}"))?;
    let ast = parser::parse(&content).map_err(|err| exec_err!("{path}: syntax error: {err}"))?;
    exec_ast(env, ast)?;
    process::exit(0);
}

fn exec_in_background(env: &mut Environment, cmd: ShellAst) -> Result<()> {
    fork_with(true, || {
        let tid = fork()?;
        if tid.is_none() {
            exec_ast_handle_error(env, cmd);
        }
        Ok(())
    })?;
    Ok(())
}

fn exec_redir(env: &mut Environment, redir: Redirect) -> Result<()> {
    let status = fork_with(false, || {
        let mut flags;
        if redir.is_output {
            flags = OpenFlags::CREAT | OpenFlags::WRONLY;
            if redir.append {
                flags |= OpenFlags::APPEND;
            }
        } else {
            flags = OpenFlags::RDONLY;
        }
        let file = open_file(var_to_string_or_empty(redir.file_name), flags)?;
        let result = if redir.is_output {
            stdout().replace_with(&file)
        } else {
            stdin().replace_with(&file)
        };
        result.map_err(|err| exec_err!("dup2 {err}"))?;
        drop(file);
        exec_ast(env, *redir.cmd)
    })?;
    env.status = status;
    Ok(())
}

fn exec_pipe(env: &mut Environment, pipe: Pipe) -> Result<()> {
    let [r_pipe, w_pipe] = rw_ulib::fs::pipe().map_err(|err| exec_err!("sh pipe: {err}"))?;
    let status = fork2(|branch| {
        if branch == 0 {
            // The parent task.
            drop(w_pipe);
            drop(r_pipe);
            Ok(())
        } else if branch == 1 {
            // The child task 0
            stdout().replace_with(&w_pipe).unwrap();
            drop(w_pipe);
            drop(r_pipe);
            exec_ast(env, *pipe.left)
        } else {
            // The child task 1
            stdin().replace_with(&r_pipe).unwrap();
            drop(w_pipe);
            drop(r_pipe);
            exec_ast(env, *pipe.right)
        }
    })?;
    env.status = status;
    Ok(())
}
