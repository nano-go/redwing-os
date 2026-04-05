use redwing_vfs::error::FsError;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("fs error: {0}")]
    FsError(#[from] FsError),

    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("{0}")]
    Custom(String),
}

#[macro_export]
macro_rules! custom_err {
    ($($arg:tt)*) => {
        crate::error::Error::Custom(format!($( $arg )*))
    };
}
