use core::ops::Deref;

use crate::{
    error::{FsError, FsErrorKind, Result},
    fs_err,
};

const FILE_NAME_LEN: usize = 256;

pub fn check_contains_invalid_char(name: &str) -> Result<()> {
    let invalid = name
        .chars()
        .any(|ch| ch == '/' || ch == '\0' || ch.is_ascii_control());
    if invalid {
        return Err(fs_err!(
            FsErrorKind::InvalidArgument,
            "the file name contains invalid character"
        ));
    }
    Ok(())
}

/// Represents a valid file name(excluding `.` and `..`).
#[derive(Debug, Clone, Copy)]
pub struct ValidFileName<'a>(&'a str);

impl<'a> Deref for ValidFileName<'a> {
    type Target = str;

    fn deref(&self) -> &'a Self::Target {
        self.0
    }
}

impl<'a> TryFrom<&'a [u8]> for ValidFileName<'a> {
    type Error = FsError;

    fn try_from(name: &'a [u8]) -> Result<Self, Self::Error> {
        Self::try_from(str::from_utf8(name)?)
    }
}

impl<'a> TryFrom<&'a str> for ValidFileName<'a> {
    type Error = FsError;

    fn try_from(name: &'a str) -> Result<Self, Self::Error> {
        if name.len() > FILE_NAME_LEN {
            return Err(FsErrorKind::FileNameTooLong.into());
        }

        if name == "." || name == ".." {
            return Err(fs_err!(
                FsErrorKind::InvalidArgument,
                "the file name is not valid"
            ));
        }

        check_contains_invalid_char(name)?;
        Ok(Self(name))
    }
}

/// Represents a valid lookup name(including `.` and `..`).
#[derive(Clone, Copy)]
pub struct ValidLookupName<'a>(&'a str);

impl<'a> ValidLookupName<'a> {
    #[must_use]
    pub unsafe fn new_unchecked(str: &'a str) -> Self {
        Self(str)
    }
}

impl<'a> From<ValidFileName<'a>> for ValidLookupName<'a> {
    fn from(value: ValidFileName<'a>) -> Self {
        Self(value.0)
    }
}

impl<'a> TryFrom<&'a [u8]> for ValidLookupName<'a> {
    type Error = FsError;

    fn try_from(name: &'a [u8]) -> Result<Self, Self::Error> {
        Self::try_from(str::from_utf8(name)?)
    }
}

impl<'a> TryFrom<&'a str> for ValidLookupName<'a> {
    type Error = FsError;

    fn try_from(name: &'a str) -> Result<Self, Self::Error> {
        if name.len() > FILE_NAME_LEN {
            return Err(FsErrorKind::FileNameTooLong.into());
        }

        check_contains_invalid_char(name)?;
        Ok(Self(name))
    }
}

impl<'a> Deref for ValidLookupName<'a> {
    type Target = str;

    fn deref(&self) -> &'a Self::Target {
        self.0
    }
}
