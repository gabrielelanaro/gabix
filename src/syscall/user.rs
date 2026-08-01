//! Validation and decoding of values supplied by user space.

const USER_ADDR_LIMIT: u64 = 0x0000_8000_0000_0000;

pub(super) fn is_user_range(ptr: u64, len: u64) -> bool {
    if ptr >= USER_ADDR_LIMIT {
        return false;
    }
    match ptr.checked_add(len) {
        Some(end) => end <= USER_ADDR_LIMIT,
        None => false,
    }
}

pub(super) fn read_user_path(ptr: u64) -> Option<alloc::string::String> {
    const MAX_PATH: usize = 256;

    if ptr == 0 || !is_user_range(ptr, MAX_PATH as u64) {
        return None;
    }

    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, MAX_PATH) };
    let end = bytes.iter().position(|&byte| byte == 0)?;
    if end == 0 {
        return None;
    }

    let path = core::str::from_utf8(&bytes[..end]).ok()?;
    Some(alloc::string::String::from(path))
}
