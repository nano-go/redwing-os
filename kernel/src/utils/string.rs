/// Formats arguments into a `heapless::String` on the stack with a specified
/// maximum length to avoid extra heap allocations.
///
/// This macro provides a convenient way to perform string formatting similar to
/// `format!` but without requiring heap allocations. It leverages
/// `heapless::String` which uses a fixed-size buffer on the stack.
///
/// # Arguments
///
/// * `$len:expr` - An expression that evaluates to a `usize` constant,
///   representing the **maximum capacity** (in bytes) of the `heapless::String`
///   buffer. The formatted string will be truncated if it exceeds this length.
/// * `$( $args:tt )*` - The format string and arguments, identical to those
///   used by `format!` or `println!`.
///
/// # Returns
///
/// A `heapless::String<$len>` containing the formatted string.
///
/// # Panics
///
/// This macro will **panic** if the formatted string, including its arguments,
/// exceeds the specified `$len` capacity. This is because `heapless::String`'s
/// `write_fmt` method, when used directly with `unwrap()`, will panic on
/// overflow. Ensure `$len` is large enough for your expected output.
///
/// # Examples
///
/// ``` no_run
/// let name = "World";
/// let num = 42;
///
/// // Create a string with a max capacity of 32 bytes
/// let hello_str = format_on_stack!(32, "Hello, {}! The answer is {}.", name, num);
/// assert_eq!(hello_str.as_str(), "Hello, World! The answer is 42.");
/// ```
pub macro format_on_stack($buf_len:expr, $( $args:tt )*) {
    {
        let mut __heapless_str_formt = heapless::String::<$buf_len>::new();
        (&mut __heapless_str_formt as &mut dyn core::fmt::Write)
            .write_fmt(format_args!($( $args )*))
            .unwrap();
        __heapless_str_formt
    }
}
