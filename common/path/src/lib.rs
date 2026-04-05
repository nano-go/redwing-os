#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::fmt;

/// Represents a file system path as a byte slice.
///
/// This struct provides utilities for parsing and manipulating paths
/// without requiring UTF-8 validity.
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Path {
    inner: [u8],
}

impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        &self
    }
}

impl AsRef<Path> for [u8] {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl AsRef<Path> for str {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

#[cfg(feature = "alloc")]
impl AsRef<Path> for alloc::string::String {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl fmt::Debug for &Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Path({:?})",
            core::str::from_utf8(self.as_bytes()).unwrap_or("<invalid UTF-8>")
        )
    }
}

impl fmt::Display for &Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            core::str::from_utf8(self.as_bytes()).unwrap_or("<invalid UTF-8>")
        )
    }
}

impl Path {
    /// Creates a new `Path` from a byte slice.
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    ///
    /// let path = Path::new("/usr/local/bin");
    /// ```
    #[must_use]
    pub fn new<T: AsRef<[u8]> + ?Sized>(path: &T) -> &Self {
        unsafe { &*(path.as_ref() as *const [u8] as *const Path) }
    }

    /// Returns the parent directory of the path, if any.
    ///
    /// The parent is determined by removing the last component.
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    ///
    /// let path = Path::new("/usr/local/bin");
    /// assert_eq!(path.parent(), Some(Path::new("/usr/local")));
    ///
    /// let path = Path::new("/home");
    /// assert_eq!(path.parent(), Some(Path::new("/")));
    ///
    /// let path = Path::new("/");
    /// assert_eq!(path.parent(), None);
    ///
    /// let guard_parent = Path::new("./");
    /// assert_eq!(guard_parent.parent(), None);
    /// ```
    #[must_use]
    pub fn parent(&self) -> Option<&Path> {
        let mut components = self.components();
        components.next_back().and_then(|comp| match comp {
            Component::Normal(_) | Component::ParentDir => Some(components.as_path()),
            Component::RootDir | Component::CurDir => None,
        })
    }

    /// Returns the file name or last component of the path, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    ///
    /// let path_with_file_name = Path::new(b"/usr/local/bin");
    /// assert_eq!(path_with_file_name.name(), Some(b"bin".as_ref()));
    ///
    /// let path_with_dir_name = Path::new(b"/usr/local/");
    /// assert_eq!(path_with_dir_name.name(), Some(b"local".as_ref()));
    /// ```
    #[must_use]
    pub fn name(&self) -> Option<&[u8]> {
        let mut components = self.components();
        components.next_back().and_then(|comp| match comp {
            Component::Normal(name) => Some(name),
            _ => None,
        })
    }

    /// Returns an iterator over the path's components viewed as byte slice.
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    ///
    /// let path = Path::new(b"/home/work/");
    /// let mut iter = path.iter();
    ///
    /// assert_eq!(iter.next(), Some("/".as_bytes()));
    /// assert_eq!(iter.next(), Some("home".as_bytes()));
    /// assert_eq!(iter.next(), Some("work".as_bytes()));
    /// assert_eq!(iter.next(), None);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = &[u8]> {
        self.components().map(|comp| comp.as_bytes())
    }

    /// Returns an iterator over the path's components.
    ///
    /// # Parsing Path
    ///
    /// * Repeat `"/"` are ignored, e.g. `a/b` and `a//b` have same components.
    ///
    /// * The `"."` will be ignored except it is at the beginning of the path.
    ///   e.g. `a/./b` and `a/b` have same components, but `./a/b` starts with
    ///   an additional `CurDir`.
    ///
    /// * The `".."` will be parsed as [`Component::ParentDir`].
    ///
    /// * Trailling `"/"` will be ignored except the `"/"` is the full of path.
    ///   e.g. `a/b` and  `a/b/` have same components, but `/a/b` starts with an
    ///   additional `RootDir`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    /// # use path::Component;
    ///
    /// let path = Path::new("/home//work/");
    /// let mut components = path.components();
    /// assert_eq!(components.next(), Some(Component::RootDir));
    /// assert_eq!(components.next(), Some(Component::Normal(b"home")));
    /// assert_eq!(components.next(), Some(Component::Normal(b"work")));
    /// assert_eq!(components.next(), None);
    ///
    /// let path = Path::new(".");
    /// let mut components = path.components();
    /// assert_eq!(components.next(), Some(Component::CurDir));
    /// assert_eq!(components.next(), None);
    ///
    /// let path = Path::new("./test/./");
    /// let mut components = path.components();
    /// assert_eq!(components.next(), Some(Component::CurDir));
    /// assert_eq!(components.next(), Some(Component::Normal(b"test")));
    /// assert_eq!(components.next(), None);
    /// ```
    #[must_use]
    pub fn components(&self) -> Components {
        Components::new(self.as_bytes())
    }

    /// Returns `true` if the path is absolute (starts with a `/`).
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    ///
    /// let path = Path::new("/usr/local/bin");
    /// assert!(path.is_absolute());
    /// ```
    #[must_use]
    pub fn is_absolute(&self) -> bool {
        self.inner.first() == Some(&b'/')
    }

    /// Returns `true` if the path is relative (does not start with `/`).
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    ///
    /// let path = Path::new("local/bin");
    /// assert!(path.is_relative());
    /// ```
    #[must_use]
    pub fn is_relative(&self) -> bool {
        !self.is_absolute()
    }

    /// Checks if the path is a file (i.e., does not end with a `/`).
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    ///
    /// let path = Path::new("/usr/local/bin");
    /// assert!(path.is_file());
    /// ```
    #[must_use]
    pub fn is_file(&self) -> bool {
        !self.is_dir()
    }

    /// Checks if the path is a directory (i.e., ends with a `/`).
    ///
    /// # Examples
    ///
    /// ```
    /// # use path::Path;
    ///
    /// let path = Path::new("/usr/local/");
    /// assert!(path.is_dir());
    /// ```
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.inner.ends_with(b"/")
            || self.inner.ends_with(b"/.")
            || &self.inner == b"."
            || self.inner.ends_with(b"/..")
            || &self.inner == b".."
    }

    /// Checks if the path is root (i.e, '/', '//'...)
    #[must_use]
    pub fn is_root(&self) -> bool {
        match self.components().next_back() {
            Some(Component::RootDir) => true,
            _ => false,
        }
    }

    /// Returns the path as a byte slice.
    #[must_use]
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.inner
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component<'a> {
    RootDir,
    CurDir,
    ParentDir,
    Normal(&'a [u8]),
}

impl<'a> Component<'a> {
    pub fn as_bytes(self) -> &'a [u8] {
        match self {
            Component::RootDir => b"/",
            Component::CurDir => b".",
            Component::ParentDir => b"..",
            Component::Normal(comp) => comp,
        }
    }
}

pub struct Components<'a> {
    buf: &'a [u8],
    front: usize,
    back: usize,
}

impl<'a> Components<'a> {
    #[must_use]
    pub fn new(path: &'a [u8]) -> Self {
        Self {
            buf: path,
            front: 0,
            back: path.len(),
        }
    }

    #[must_use]
    pub fn as_path(&self) -> &'a Path {
        if self.back == 0 {
            return Path::new(b"");
        }
        let mut e = self.back - 1;
        while e != 0 && self.buf[e] == b'/' {
            e -= 1;
        }
        self.buf[self.front..=e].as_ref()
    }
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }

        let start = self.front;
        let mut end = self.front;

        while end < self.back && self.buf[end] != b'/' {
            end += 1;
        }

        self.front = end;
        if self.front < self.back && self.buf[self.front] == b'/' {
            self.front += 1;
        }

        match &self.buf[start..end] {
            b"" => {
                if start == 0 {
                    Some(Component::RootDir)
                } else {
                    self.next()
                }
            }
            b"." => {
                if start == 0 {
                    Some(Component::CurDir)
                } else {
                    self.next()
                }
            }
            b".." => Some(Component::ParentDir),
            s => Some(Component::Normal(s)),
        }
    }
}

impl<'a> DoubleEndedIterator for Components<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }

        // skip '/'.
        let mut end = self.back;
        while end > self.front && self.buf[end - 1] == b'/' {
            end -= 1;
        }
        self.back = end;

        let mut start = self.back;
        while start > self.front && self.buf[start - 1] != b'/' {
            start -= 1;
        }

        let component = &self.buf[start..self.back];
        self.back = start;

        match component {
            b"" => {
                if start == 0 {
                    Some(Component::RootDir)
                } else {
                    self.next_back()
                }
            }
            b"." => {
                if start == 0 {
                    Some(Component::CurDir)
                } else {
                    self.next_back()
                }
            }
            b".." => Some(Component::ParentDir),
            s => Some(Component::Normal(s)),
        }
    }
}
