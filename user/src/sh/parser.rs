use core::{
    fmt::{self, Arguments},
    iter::Peekable,
    str::Chars,
};

use alloc::{borrow::Cow, boxed::Box, string::ToString, vec::Vec};

use crate::{
    ast::{CmdExec, Pipe, Redirect, ShellAst, VarString},
    utils::EscapeStr,
};

pub const EOF: char = char::MIN;

pub const SPECIAL_CHARS: &str = "\0|&\'\"<>;()";

pub type Result<T, E = SyntaxError> = core::result::Result<T, E>;

pub fn parse<'a>(line: &'a str) -> Result<ShellAst<'a>> {
    ShellParser::new(line).parse()
}

#[derive(Debug, Clone)]
pub struct SyntaxError {
    msg: Cow<'static, str>,
}

macro_rules! syntax_error {
    ($( $args:expr ),*) => {
        SyntaxError::from_args( format_args!( $( $args ,)* ))
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl SyntaxError {
    #[must_use]
    pub fn from_args<'a>(args: Arguments<'a>) -> Self {
        let msg = if let Some(str) = args.as_str() {
            Cow::Borrowed(str)
        } else {
            Cow::Owned(args.to_string())
        };
        Self { msg }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token<'a> {
    String(EscapeStr<'a>),
    Variable(EscapeStr<'a>),
    BitOr,
    BitAnd,
    GtGt,
    Gt,
    Lt,
    LeftParen,
    RightParen,
    Semi,
    Eof,
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::String(str) => write!(f, "{str}"),
            Token::Variable(str) => write!(f, "var({str})"),
            Token::BitOr => write!(f, "|"),
            Token::BitAnd => write!(f, "&"),
            Token::GtGt => write!(f, ">>"),
            Token::Gt => write!(f, ">"),
            Token::Lt => write!(f, "<"),
            Token::LeftParen => write!(f, "("),
            Token::RightParen => write!(f, ")"),
            Token::Semi => write!(f, ";"),
            Token::Eof => write!(f, "<EOF>"),
        }
    }
}

struct Tokenizer<'a> {
    buf: &'a str,
    chars: Peekable<Chars<'a>>,
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    #[must_use]
    #[inline]
    pub fn new(str: &'a str) -> Self {
        Self {
            buf: str,
            chars: str.chars().peekable(),
            pos: 0,
        }
    }

    #[inline]
    pub fn consume(&mut self) {
        if self.chars.next().is_some() {
            self.pos += 1;
        }
    }

    #[must_use]
    #[inline]
    pub fn peek(&mut self) -> char {
        self.chars.peek().cloned().unwrap_or(EOF)
    }

    #[must_use]
    #[inline]
    pub fn pos(&mut self) -> usize {
        self.pos
    }

    #[must_use]
    #[inline]
    pub fn got(&mut self, expected: char) -> bool {
        if self.peek() == expected {
            self.consume();
            true
        } else {
            false
        }
    }

    pub fn match_char(&mut self, expected: char) -> Result<()> {
        if self.peek() != expected {
            Err(syntax_error!("unexpected char '{}'", self.peek()))
        } else {
            self.consume();
            Ok(())
        }
    }

    pub fn skip_whitespace(&mut self) {
        while self.peek().is_whitespace() {
            self.consume();
        }
    }

    fn parse_token(&mut self) -> Result<Token<'a>> {
        self.skip_whitespace();
        let ch = self.peek();
        self.consume();
        match ch {
            EOF => Ok(Token::Eof),
            '|' => Ok(Token::BitOr),
            '&' => Ok(Token::BitAnd),
            ';' => Ok(Token::Semi),
            '(' => Ok(Token::LeftParen),
            ')' => Ok(Token::RightParen),
            '>' => {
                if self.got('>') {
                    Ok(Token::GtGt)
                } else {
                    Ok(Token::Gt)
                }
            }
            '<' => Ok(Token::Lt),
            '\'' => Ok(Token::String(self.parse_string(Some('\''), false)?)),
            '\"' => Ok(Token::String(self.parse_string(Some('\"'), true)?)),
            _ => {
                let str = self.parse_string(None, false)?;
                if str.raw_str().starts_with("$") {
                    // Remove the leading '$' character.
                    let var_name = EscapeStr::new(&str.raw_str()[1..], false);
                    Ok(Token::Variable(var_name))
                } else {
                    Ok(Token::String(str))
                }
            }
        }
    }

    fn parse_string(&mut self, terminal_ch: Option<char>, escaped: bool) -> Result<EscapeStr<'a>> {
        let mut start_pos = self.pos();
        if terminal_ch.is_none() {
            // The current character is not ' or ", move to the previous char.
            start_pos -= 1;
        }

        // The position of the buffer at the end of token string(exclusive).
        let end_pos;
        if let Some(terminal_ch) = terminal_ch {
            self.consume_until(terminal_ch, escaped);
            end_pos = self.pos();
            // Consume the terminal chars. use match_char to report error in EOF case.
            self.match_char(terminal_ch)?;
        } else {
            self.consume_until_nonstr();
            end_pos = self.pos();
        }

        Ok(EscapeStr::new(&self.buf[start_pos..end_pos], escaped))
    }

    fn consume_until(&mut self, terminal: char, escaped: bool) {
        loop {
            let ch = self.peek();

            if ch == terminal || ch == EOF {
                return;
            }

            self.consume();

            if escaped && ch == '\\' {
                let ch = self.peek();
                if ch == terminal {
                    self.consume();
                }
            }
        }
    }

    fn consume_until_nonstr(&mut self) {
        fn is_valid_char(ch: char) -> bool {
            !ch.is_whitespace() && !SPECIAL_CHARS.contains(ch)
        }

        while is_valid_char(self.peek()) {
            self.consume();
        }
    }
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = Result<Token<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.parse_token() {
            Ok(Token::Eof) => None,
            token => Some(token),
        }
    }
}

pub struct ShellParser<'a> {
    tokenizer: Peekable<Tokenizer<'a>>,
}

impl<'a> ShellParser<'a> {
    #[must_use]
    pub fn new(str: &'a str) -> Self {
        Self {
            tokenizer: Tokenizer::new(str).peekable(),
        }
    }

    fn peek(&mut self) -> Result<Token<'a>> {
        let peek = self.tokenizer.peek().cloned();
        match peek {
            None => Ok(Token::Eof),
            Some(token) => token,
        }
    }

    fn consume(&mut self) -> Result<()> {
        self.tokenizer.next();
        self.peek()?;
        Ok(())
    }

    fn got(&mut self, expected: Token) -> Result<bool> {
        let token = self.peek()?;
        if token == expected {
            self.consume()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn expected(&mut self, expected: Token) -> Result<()> {
        let token = self.peek()?;
        if token == expected {
            self.consume()
        } else {
            Err(syntax_error!("expected {expected}, but {token}"))
        }
    }

    fn expected_var_or_string(&mut self) -> Result<VarString<'a>> {
        let token = self.peek()?;
        if let Token::String(str) = token {
            self.consume()?;
            Ok(VarString { is_var: false, str })
        } else if let Token::Variable(str) = token {
            self.consume()?;
            Ok(VarString { is_var: true, str })
        } else {
            Err(syntax_error!("expected a string/variable, but {token}"))
        }
    }

    fn try_match_var_or_string(&mut self) -> Result<Option<VarString<'a>>> {
        let token = self.peek()?;
        if let Token::String(str) = token {
            self.consume()?;
            Ok(Some(VarString { is_var: false, str }))
        } else if let Token::Variable(str) = token {
            self.consume()?;
            Ok(Some(VarString { is_var: true, str }))
        } else {
            Ok(None)
        }
    }

    pub fn parse(&mut self) -> Result<ShellAst<'a>> {
        let expr = self.parse_expr()?;
        self.expected(Token::Eof)?;
        Ok(expr)
    }

    #[inline]
    fn parse_expr(&mut self) -> Result<ShellAst<'a>> {
        self.parse_list()
    }

    /// Syntax likes `ls; ls; ls;...`
    fn parse_list(&mut self) -> Result<ShellAst<'a>> {
        let mut list = Vec::new();
        while {
            if let Token::Eof | Token::RightParen = self.peek()? {
                // Supports trailing ';'.
                false
            } else {
                let expr = self.parse_pipe()?;
                list.push(expr);
                self.got(Token::Semi)?
            }
        } {}
        Ok(ShellAst::List(list))
    }

    /// Syntax likes `echo hello | cat`
    fn parse_pipe(&mut self) -> Result<ShellAst<'a>> {
        let left = self.parse_redirect()?;
        if let Token::BitOr = self.peek()? {
            self.consume()?;
            let right = self.parse_redirect()?;
            Ok(ShellAst::Pipe(Pipe {
                left: Box::new(left),
                right: Box::new(right),
            }))
        } else {
            Ok(left)
        }
    }

    /// Syntax likes `cat < main.rs`, `ls >> ls.txt`
    fn parse_redirect(&mut self) -> Result<ShellAst<'a>> {
        let cmd = self.parse_term()?;

        let append: bool;
        let is_output: bool;

        match self.peek()? {
            Token::Gt => {
                is_output = true;
                append = false;
            }
            Token::GtGt => {
                is_output = true;
                append = true;
            }
            Token::Lt => {
                is_output = false;
                append = false;
            }
            _ => return Ok(cmd),
        }

        // comsume '>','>>' or '<'
        self.consume()?;
        let file_name = self.expected_var_or_string()?;
        let redir = ShellAst::Redirect(Redirect {
            cmd: Box::new(cmd),
            is_output,
            file_name,
            append,
        });

        // Support synyax like 'ls > tmp.txt &'
        self.parse_suffix(redir)
    }

    /// Syntax likes `rm -rf ./log`, `(...)`
    fn parse_term(&mut self) -> Result<ShellAst<'a>> {
        let command = match self.peek()? {
            Token::String(name) => self.parse_exec(name, false)?,
            Token::Variable(name) => self.parse_exec(name, true)?,
            Token::LeftParen => {
                self.consume()?;
                let expr = self.parse_expr()?;
                self.expected(Token::RightParen)?;
                expr
            }
            Token::Eof => ShellAst::Empty,
            token => {
                return Err(syntax_error!("unexpected token {token}"));
            }
        };
        self.parse_suffix(command)
    }

    /// Syntax likes `rm -rf ./log`
    fn parse_exec(&mut self, name: EscapeStr<'a>, is_var: bool) -> Result<ShellAst<'a>> {
        self.consume()?;
        let mut args = Vec::new();
        args.push(VarString { is_var, str: name });
        while let Some(arg) = self.try_match_var_or_string()? {
            args.push(arg);
        }
        Ok(ShellAst::Exec(CmdExec { args }))
    }

    /// Syntax likes `ls &`
    fn parse_suffix(&mut self, expr: ShellAst<'a>) -> Result<ShellAst<'a>> {
        match self.peek()? {
            Token::BitAnd => {
                self.consume()?;
                Ok(ShellAst::Background(Box::new(expr)))
            }
            _ => Ok(expr),
        }
    }
}
