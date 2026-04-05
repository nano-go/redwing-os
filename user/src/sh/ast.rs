use alloc::{boxed::Box, vec::Vec};

use crate::utils::EscapeStr;

pub enum ShellAst<'a> {
    Empty,
    List(Vec<ShellAst<'a>>),
    Exec(CmdExec<'a>),
    Background(Box<ShellAst<'a>>),
    Redirect(Redirect<'a>),
    Pipe(Pipe<'a>),
}

pub struct VarString<'a> {
    // Starts with '$'
    pub is_var: bool,
    pub str: EscapeStr<'a>,
}

pub struct CmdExec<'a> {
    pub args: Vec<VarString<'a>>,
}

pub struct Redirect<'a> {
    /// Redirect output if `true` or input.
    pub is_output: bool,
    pub cmd: Box<ShellAst<'a>>,
    pub file_name: VarString<'a>,

    /// Do open file with APPEND flag? Only available if output.
    pub append: bool,
}

pub struct Pipe<'a> {
    pub left: Box<ShellAst<'a>>,
    pub right: Box<ShellAst<'a>>,
}
