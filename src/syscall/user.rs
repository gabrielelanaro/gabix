//! Address-bound checks and path decoding for syscall arguments.

// Exclusive upper bound of the low canonical half in four-level x86 paging.
// Kernel mappings also exist below this bound; it is not a permission boundary.
const LOW_HALF_END: u64 = 0x0000_8000_0000_0000;

/// Check the numeric interval [ptr, ptr + len) for overflow and the low-half
/// boundary. Zero is accepted. Page presence, USER_ACCESSIBLE, writability,
/// alignment and Rust slice validity are not checked by this predicate.
pub(super) fn is_low_half_range(ptr: u64, len: u64) -> bool {
    if ptr >= LOW_HALF_END {
        return false;
    }
    match ptr.checked_add(len) {
        Some(end) => end <= LOW_HALF_END,
        None => false,
    }
}

/// Read a nonempty, NUL-terminated UTF-8 path within a 256-byte window.
/// The current implementation requires that entire window to be readable,
/// even if a terminator appears earlier. Bounds alone do not establish this;
/// an unmapped or protected user pointer can fault inside the kernel.
pub(super) fn read_user_path(ptr: u64) -> Option<alloc::string::String> {
    const PATH_SCAN_BYTES: usize = 256;

    if ptr == 0 || !is_low_half_range(ptr, PATH_SCAN_BYTES as u64) {
        return None;
    }

    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, PATH_SCAN_BYTES) };
    let end = bytes.iter().position(|&byte| byte == 0)?;
    if end == 0 {
        return None;
    }

    let path = core::str::from_utf8(&bytes[..end]).ok()?;
    Some(alloc::string::String::from(path))
}
