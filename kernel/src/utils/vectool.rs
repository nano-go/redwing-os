use alloc::collections::VecDeque;

/// Drains elements from the front of a [`VecDeque`] into a destination slice.
///
/// Copies up to `dst.len()` elements from `src` into `dst`, preserving order,
/// and removes the copied elements from `src`.
///
/// # Arguments
///
/// * `src` - The source `VecDeque` to drain from.
/// * `dst` - The destination slice to copy into.
///
/// # Returns
///
/// The number of elements copied.
pub fn drain_vecdeque_to_slice<T: Copy>(src: &mut VecDeque<T>, dst: &mut [T]) -> usize {
    let to_copy = usize::min(src.len(), dst.len());
    let (slice_a, slice_b) = src.as_slices();

    // Copy from the first contiguous slice
    let copy_a = usize::min(slice_a.len(), to_copy);
    dst[..copy_a].copy_from_slice(&slice_a[..copy_a]);

    // Copy from the second slice if needed
    let copy_b = to_copy - copy_a;
    if copy_b > 0 {
        dst[copy_a..to_copy].copy_from_slice(&slice_b[..copy_b]);
    }

    // Drain the used entries efficiently
    src.drain(..to_copy);

    to_copy
}
