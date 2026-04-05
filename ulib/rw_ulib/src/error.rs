use core::fmt;
use syserr::SysErrorKind;

pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Error {
    /* From Kernel */
    System(SysErrorKind),

    /* From User */
    ReadExact,
    WriteExact,

    Unknown,
}

impl core::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let str = match self {
            Self::System(err) => {
                return write!(f, "{err}");
            }
            Self::ReadExact => "read exact",
            Self::WriteExact => "write exact",

            Self::Unknown => "unknown",
        };
        write!(f, "{}", str)
    }
}

pub fn wrap_with_result(code: isize) -> Result<usize, Error> {
    if code >= 0 {
        Ok(code as usize)
    } else {
        let errno = u32::try_from(-code).map_err(|_| Error::Unknown)?;
        if let Ok(syserr) = SysErrorKind::try_from(errno) {
            Err(Error::System(syserr))
        } else {
            Err(Error::Unknown)
        }
    }
}
