//! Virtual File System layer for mini-os.
//!
//! Slice 4.4 (Lesson 2): the three core abstractions — Inode, File, FdTable —
//! plus a hardcoded inode for end-to-end testing of `open`/`read`/`write`/
//! `close` before path resolution lands in Lesson 3.
//!
//! The split mirrors Lesson 1's vocabulary:
//!   - Inode   = the file's identity (data + metadata). Has no name.
//!   - File    = an open instance (cursor + access mode). Private per `open()`.
//!   - FdTable = per-process table of FDs (integer keys to Files).
//!
//! `Arc<Mutex<...>>` patterns are required because (a) two `open()`s of the
//! same inode share its data, (b) two FDs from `dup()` would share one File,
//! and (c) we have no borrow checker across the kernel/user boundary — syscalls
//! hand `Arc<File>` clones out so the scheduler and the syscall handler can
//! both touch the same File without aliasing.

use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================
// Inode — the file itself
// ============================================================

/// A file's identity: its bytes, mutable through interior mutability.
///
/// In a real VFS this would also hold metadata (size, owner uid, group gid,
/// permissions, timestamps, link count, allocated blocks). For Lesson 2 we
/// keep only the data — the rest gets added in Lesson 3 when we grow the
/// inode layer to support multiple files and metadata.
///
/// An inode has no name. Names are dentries' job.
pub struct Inode {
    /// The file's bytes. `Mutex` for interior mutability so two `File`
    /// instances pointing at the same `Inode` can both write without aliasing
    /// violations.
    pub data: Mutex<Vec<u8>>,
}

impl Inode {
    /// Create a new inode owning a copy of `initial`.
    pub fn new(initial: &[u8]) -> Self {
        Self {
            data: Mutex::new(Vec::from(initial)),
        }
    }
}

// ============================================================
// File — an open instance
// ============================================================

/// An open instance of an inode. Holds the cursor and a reference to the
/// shared inode.
///
/// Two `open()` calls produce two distinct `File` instances pointing at the
/// same `Inode` — independent cursors, shared data. `dup()` and `fork()`
/// (Lesson 5+) will produce shared `File` instances — same cursor, same data.
pub struct File {
    /// The underlying inode. Shared across all `File` instances that opened
    /// this same file.
    pub inode: Arc<Inode>,

    /// The read/write offset. `Mutex` so multiple syscalls on the same File
    /// can update it without re-borrowing the File — which matters because
    /// the scheduler hands `Arc<File>` clones out, not `&mut File`.
    pub cursor: Mutex<u64>,
}

impl File {
    /// Create a new open instance with cursor at position 0.
    pub fn new(inode: Arc<Inode>) -> Self {
        Self {
            inode,
            cursor: Mutex::new(0),
        }
    }
}

// ============================================================
// FdTable — per-process table of file descriptors
// ============================================================

/// A per-process table mapping small integer FDs to `Arc<File>` entries.
///
/// Linux uses a more elaborate structure (bitmap + growable array) for sparse
/// tables at scale. We use a flat `Vec`; simplicity wins for early mini-os
/// where the typical process has fewer than 10 FDs open.
///
/// FD numbers always start at 0 and are reused on close. Conventions like
/// "0=stdin, 1=stdout, 2=stderr" live at the syscall-handler level, not here
/// — this table is purely an integer→File map.
pub struct FdTable {
    table: Vec<Option<Arc<File>>>,
}

impl FdTable {
    /// Create an empty FD table.
    pub fn new() -> Self {
        Self { table: Vec::new() }
    }

    /// Insert `file` at the lowest free FD number, return that number.
    ///
    /// "Lowest free" matches POSIX `open()` semantics — after closing FD 3,
    /// the next `open()` reuses 3 rather than appending at the end.
    pub fn open(&mut self, file: Arc<File>) -> u64 {
        // Scan for the first None slot.
        for (i, slot) in self.table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(file);
                return i as u64;
            }
        }
        // No free slot — append.
        let fd = self.table.len() as u64;
        self.table.push(Some(file));
        fd
    }

    /// Look up the `File` for `fd`, if any. Returns a cloned `Arc` (cheap
    /// refcount bump) so the caller can use it without holding the FdTable.
    pub fn get(&self, fd: u64) -> Option<Arc<File>> {
        self.table.get(fd as usize).and_then(|slot| slot.clone())
    }

    /// Remove the entry for `fd`. Returns `true` if there was an entry to
    /// remove, `false` if `fd` was already free or out of range.
    ///
    /// Dropping the `Arc<File>` here decrements its refcount; if this was the
    /// last reference, the File (and its `Arc<Inode>`, if also last) gets
    /// freed. This is how `close(fd)` releases kernel memory.
    pub fn close(&mut self, fd: u64) -> bool {
        if let Some(slot) = self.table.get_mut(fd as usize) {
            if slot.is_some() {
                *slot = None;
                return true;
            }
        }
        false
    }
}

// ============================================================
// Hardcoded inode for Lesson 2 testing
// ============================================================

lazy_static::lazy_static! {
    /// The single inode that exists in Lesson 2's "filesystem". `sys_open`
    /// ignores its path argument and returns a `File` pointing here.
    ///
    /// Lesson 3 replaces this with a real path-indexed table of inodes.
    pub static ref HARDCODED_INODE: Arc<Inode> = {
        Arc::new(Inode::new(b"hello vfs\n"))
    };
}
