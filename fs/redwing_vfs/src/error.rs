use core::{
    error::Error,
    fmt::{Arguments, Debug, Display},
    str::Utf8Error,
};

use alloc::{alloc::AllocError, borrow::Cow, string::ToString};

pub type Result<T, E = FsError> = core::result::Result<T, E>;

#[macro_export]
macro_rules! fs_err {
    ($kind:expr) => {
        $crate::error::FsError { kind: $kind, msg: None }
    };

    ($kind:expr, $lit:literal $(, $args:expr)* $(,)? ) => {
        $crate::error::FsError::with_fmt_args($kind, format_args!($lit $(, $args )*))
    }
}

#[derive(Debug)]
pub struct FsError {
    pub msg: Option<Cow<'static, str>>,
    pub kind: FsErrorKind,
}

impl Error for FsError {}

impl Display for FsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(ref msg) = self.msg {
            write!(f, "{}: {}", self.kind, msg)
        } else {
            write!(f, "{}", self.kind)
        }
    }
}

impl From<FsErrorKind> for FsError {
    fn from(kind: FsErrorKind) -> Self {
        Self { msg: None, kind }
    }
}

impl From<AllocError> for FsError {
    fn from(_: AllocError) -> Self {
        fs_err!(FsErrorKind::OutOfMemory, "due to alloc")
    }
}

impl From<Utf8Error> for FsError {
    fn from(value: Utf8Error) -> Self {
        fs_err!(FsErrorKind::InvalidUt8Str, "{}", value)
    }
}

#[cfg(feature = "to_syserr")]
impl From<FsError> for syserr::SysError {
    fn from(s: FsError) -> syserr::SysError {
        let kind = match s.kind {
            FsErrorKind::OutOfMemory => syserr::SysErrorKind::OutOfMemory,
            FsErrorKind::IOError => syserr::SysErrorKind::IOError,
            FsErrorKind::AlreadyExists => syserr::SysErrorKind::AlreadyExists,
            FsErrorKind::FileTooLarge => syserr::SysErrorKind::FileTooLarge,
            FsErrorKind::FileNameTooLong => syserr::SysErrorKind::FileNameTooLong,
            FsErrorKind::NoSuchFileOrDirectory => syserr::SysErrorKind::NoSuchFileOrDirectory,
            FsErrorKind::NoSuchDev => syserr::SysErrorKind::NoSuchDev,
            FsErrorKind::IsADirectory => syserr::SysErrorKind::IsADirectory,
            FsErrorKind::NotADirectory => syserr::SysErrorKind::NotADirectory,
            FsErrorKind::NotEmpty => syserr::SysErrorKind::NotEmpty,
            FsErrorKind::PermissionDenied => syserr::SysErrorKind::PermissionDenied,
            FsErrorKind::NoSpaceLeft => syserr::SysErrorKind::NoSpaceLeft,
            FsErrorKind::ExDev => syserr::SysErrorKind::ExDev,
            FsErrorKind::FileSystemCorruption => syserr::SysErrorKind::BadFs,
            FsErrorKind::InvalidArgument => syserr::SysErrorKind::InvalidArgument,
            FsErrorKind::Unsupported => syserr::SysErrorKind::Unsupported,
            FsErrorKind::InvalidData => syserr::SysErrorKind::InvalidData,
            FsErrorKind::InvalidUt8Str => syserr::SysErrorKind::InvalidUt8Str,
        };
        syserr::SysError { kind, msg: s.msg }
    }
}

impl FsError {
    #[must_use]
    pub fn with_fmt_args<'a>(kind: FsErrorKind, args: Arguments<'a>) -> Self {
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
    pub fn kind(&self) -> FsErrorKind {
        self.kind
    }

    #[must_use]
    pub fn msg(&self) -> Option<&str> {
        self.msg.as_ref().map(|cow| cow.as_ref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FsErrorKind {
    OutOfMemory,
    IOError,
    AlreadyExists,
    FileTooLarge,
    FileNameTooLong,
    NoSuchFileOrDirectory,
    NoSuchDev,
    IsADirectory,
    NotADirectory,
    NotEmpty,
    PermissionDenied,
    NoSpaceLeft,
    ExDev,
    FileSystemCorruption,
    InvalidArgument,
    Unsupported,
    InvalidData,
    InvalidUt8Str,
}

impl Display for FsErrorKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let str = match self {
            Self::OutOfMemory => "out of memory",
            Self::IOError => "io error",
            Self::AlreadyExists => "already exists",
            Self::FileTooLarge => "file too large",
            Self::FileNameTooLong => "file name too long",
            Self::NoSuchFileOrDirectory => "no such file or directory",
            Self::NoSuchDev => "no such dev",
            Self::IsADirectory => "is a directory",
            Self::NotADirectory => "not a directory",
            Self::NotEmpty => "not empty",
            Self::PermissionDenied => "permission denied",
            Self::NoSpaceLeft => "no space left on device",
            Self::ExDev => "invalid cross-device link",
            Self::FileSystemCorruption => "file system corruption",
            Self::InvalidArgument => "invalid argument",
            Self::Unsupported => "Unsupported",
            Self::InvalidData => "invalid data",
            Self::InvalidUt8Str => "invalid ut8 string",
        };
        write!(f, "{}", str)
    }
}
