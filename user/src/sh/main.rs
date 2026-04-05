#![no_std]
#![no_main]
#![feature(decl_macro)]

use alloc::{string::ToString, vec::Vec};
use interpreter::Environment;

use rw_ulib::{env, fs, io::stdin, print, println, process};

extern crate alloc;

mod ast;
mod interpreter;
mod parser;
mod utils;

pub struct ShellArgs {
    is_interactive: bool,
    is_mute: bool,
    defaults: Vec<&'static str>,
}

impl ShellArgs {
    pub fn parse() -> ShellArgs {
        let args = env::args();
        let mut is_interactive = args.is_empty();
        let mut is_mute = false;
        let mut defaults = Vec::new();
        for arg in args {
            match arg {
                "-i" | "--interactive" => {
                    is_interactive = true;
                }

                "-m" | "--mute" => {
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
            is_interactive: is_interactive || defaults.is_empty(),
            is_mute,
            defaults,
        }
    }
}

#[no_mangle]
pub fn main() {
    let args = ShellArgs::parse();

    let mut env = Environment::new();

    if args.is_interactive {
        repl(args, env);
    } else {
        let content = fs::read_str(args.defaults[0]);
        match content {
            Err(err) => {
                println!("sh: {err}");
                process::exit(1);
            }
            Ok(content) => run(&mut env, &content),
        }
    }
}

fn repl(args: ShellArgs, mut env: Environment) {
    let stdin = stdin();
    loop {
        if !args.is_mute {
            print!(
                "\n@ {} C: {}\n$ ",
                env::var("PWD").unwrap_or_else(|| "/".to_string()),
                env.status
            );
        }
        let line = stdin.read_line().unwrap();
        if line.is_empty() {
            // Read EOF.
            return;
        }
        run(&mut env, &line);
    }
}

fn run(env: &mut Environment, content: &str) {
    let ast = match parser::parse(content) {
        Ok(ast) => ast,
        Err(err) => {
            println!("sh: {err}");
            return;
        }
    };

    interpreter::exec_ast_handle_error(env, ast);
}
