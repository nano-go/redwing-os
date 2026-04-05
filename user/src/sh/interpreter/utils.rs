use alloc::{
    borrow::Cow,
    string::{String, ToString},
    vec::Vec,
};
use path::{Component, Path};

#[must_use]
pub fn canonical_path<P: AsRef<Path>>(path: P) -> String {
    let path = path.as_ref();
    let mut stack = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => return "./".to_string(),
            Component::ParentDir => {
                stack.pop();
            }
            Component::RootDir => stack.push(Cow::Borrowed("")),
            Component::Normal(name) => stack.push(String::from_utf8_lossy(name)),
        }
    }
    let path = stack.join("/");
    if path.is_empty() {
        "/".to_string()
    } else {
        path
    }
}

#[must_use]
pub fn is_identifier(name: &str) -> bool {
    if name.is_empty() || name.chars().next().unwrap().is_ascii_digit() {
        return false;
    }

    for ch in name.chars() {
        if !ch.is_ascii_alphabetic() && !ch.is_ascii_digit() && ch != '_' {
            return false;
        }
    }

    true
}
