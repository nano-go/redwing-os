#![no_std]
#![feature(allocator_api)]

extern crate alloc;

use core::{
    error::Error,
    fmt::{Arguments, Debug, Display},
    num::TryFromIntError,
    str::Utf8Error,
};

use alloc::{alloc::AllocError, borrow::Cow, string::ToString};
use num_enum::{IntoPrimitive, TryFromPrimitive};

pub type Result<T> = core::result::Result<T, SysError>;

#[macro_export]
macro_rules! sys_err {
    ($kind:expr) => {
        $crate::SysError { kind: $kind, msg: None }
    };

    ($kind:expr, $lit:literal $(, $args:expr)* $(,)? ) => {
        $crate::SysError::with_fmt_args($kind, format_args!($lit $(, $args )*))
    }
}

#[derive(Debug)]
pub struct SysError {
    pub msg: Option<Cow<'static, str>>,
    pub kind: SysErrorKind,
}

impl Error for SysError {}

impl Display for SysError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(ref msg) = self.msg {
            write!(f, "{}: {}", self.kind, msg)
        } else {
            write!(f, "{}", self.kind)
        }
    }
}

impl From<SysErrorKind> for SysError {
    fn from(kind: SysErrorKind) -> Self {
        Self { msg: None, kind }
    }
}

impl From<AllocError> for SysError {
    fn from(_: AllocError) -> Self {
        sys_err!(SysErrorKind::OutOfMemory, "due to alloc")
    }
}

impl From<Utf8Error> for SysError {
    fn from(value: Utf8Error) -> Self {
        sys_err!(SysErrorKind::InvalidUt8Str, "{}", value)
    }
}

impl From<TryFromIntError> for SysError {
    fn from(_value: TryFromIntError) -> Self {
        sys_err!(SysErrorKind::InvalidArgument)
    }
}

impl SysError {
    #[must_use]
    pub fn with_fmt_args<'a>(kind: SysErrorKind, args: Arguments<'a>) -> Self {
        let msg = if let Some(str) = args.as_str() {
            Cow::Borrowed(str)
        } else {
            Cow::Owned(args.to_string())
        };
        Self {
            msg: Some(msg),
            kind,
        }
    }

    #[must_use]
    pub fn kind(&self) -> SysErrorKind {
        self.kind
    }

    #[must_use]
    pub fn msg(&self) -> Option<&str> {
        self.msg.as_ref().map(|cow| cow.as_ref())
    }

    #[must_use]
    pub fn errno(&self) -> u32 {
        self.kind.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoPrimitive, TryFromPrimitive)]
#[repr(u32)]
pub enum SysErrorKind {
    NotPermitted = 1,

    OutOfMemory,
    NoSys,
    NoSuchProcess,
    Interrupted,

    IOError,
    AlreadyExists,
    BadFileDescriptor,
    FileTooLarge,
    FileNameTooLong,
    NoSuchFileOrDirectory,
    IsADirectory,
    NotADirectory,
    NotEmpty,
    PermissionDenied,
    TooManyOpenFiles,
    NoSpaceLeft,
    NoSuchDev,
    NoExec,
    Pipe,
    ExDev,
    BadFs,
    NoTty,

    InvalidArgument,
    Unsupported,
    InvalidData,

    ExecFormat,

    Fault,

    InvalidUt8Str,

    TooManyTasks,
}

impl Display for SysErrorKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let str = match self {
            Self::NotPermitted => "operation not permitted",
            Self::OutOfMemory => "out of memory",
            Self::NoSys => "unknown system call number",
            Self::NoSuchProcess => "no such process",
            Self::Interrupted => "interrupted system call",

            Self::IOError => "io error",
            Self::AlreadyExists => "already exists",
            Self::BadFileDescriptor => "bad file descriptor",
            Self::FileTooLarge => "file too large",
            Self::FileNameTooLong => "file name too long",
            Self::NoSuchFileOrDirectory => "no such file or directory",
            Self::IsADirectory => "is a directory",
            Self::NotADirectory => "not a directory",
            Self::NotEmpty => "not empty",
            Self::PermissionDenied => "Permission denied",
            Self::TooManyOpenFiles => "too many open files",
            Self::NoSpaceLeft => "no space left on device",
            Self::NoSuchDev => "no such device",
            Self::NoExec => "not an executable file",
            Self::Pipe => "pipe error",
            Self::ExDev => "invalid cross-device link",
            Self::BadFs => "bad file system",

            Self::InvalidArgument => "invalid argument",
            Self::Unsupported => "Unsupported",
            Self::InvalidData => "invalid data",
            Self::ExecFormat => "executable format error",

            Self::NoTty => "inappropriate ioctl for device",

            Self::Fault => "address fault",

            Self::InvalidUt8Str => "invalid ut8 string",

            Self::TooManyTasks => "too many tasks",
        };
        write!(f, "{}", str)
    }
}
