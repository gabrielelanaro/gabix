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
//! `Arc<IrqMutex<...>>` patterns are required because (a) two `open()`s of the
//! same inode share its data, (b) two FDs from `dup()` would share one File,
//! and (c) we have no borrow checker across the kernel/user boundary — syscalls
//! hand `Arc<File>` clones out so the scheduler and the syscall handler can
//! both touch the same File without aliasing.

use crate::irq_mutex::IrqMutex;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

// ============================================================
// InodeKind — what kind of bytes an inode holds
// ============================================================

/// What kind of bytes an `Inode` holds.
///
/// Linux packs this into the upper bits of `i_mode` (the same field that
/// holds permission bits) alongside six other kinds: symlink, block device,
/// character device, named pipe, socket. We use a clean Rust enum — easier
/// to pattern-match, easier to read. The enum grows as we add the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeKind {
    /// A regular file. Bytes are the file's contents.
    File,
    /// A directory. Bytes are a sequence of directory entries (dirents).
    Dir,
}

// ============================================================
// Inode — the file itself
// ============================================================

/// A file's identity: a stable inode number, a kind discriminant, and the
/// bytes themselves, mutable through interior mutability.
///
/// An inode has no name. Names are dentries' job — directory entries that
/// map a name to an inode number, which is what `ino` is for.
pub struct Inode {
    /// Stable identifier for this inode. Stored in directory entries, returned
    /// to userspace by `stat()`. Survives inode cache eviction in Linux's
    /// design (and would survive reboot on a disk-backed filesystem). Allocated
    /// monotonically from `NEXT_INO`.
    pub ino: u64,

    /// File or directory. Tells the kernel how to interpret `data` — as file
    /// contents or as bytes formatted as directory entries.
    pub kind: InodeKind,

    /// The file's bytes. `Mutex` for interior mutability so two `File`
    /// instances pointing at the same `Inode` can both write without aliasing
    /// violations. For directories, the bytes are dirent records.
    pub data: IrqMutex<Vec<u8>>,
}

/// Monotonic counter for allocating fresh inode numbers. Starts at 1 — Linux
/// reserves inode 0 as "no inode" (the NULL-equivalent), and we follow that
/// convention. `fetch_add(1)` returns the value before the add, so the first
/// allocated inode gets number 1.
static NEXT_INO: AtomicU64 = AtomicU64::new(1);

impl Inode {
    /// Allocate a fresh regular-file inode with the given initial bytes,
    /// register it, and return an `Arc` to it.
    pub fn new_file(initial: &[u8]) -> Arc<Inode> {
        Self::alloc(InodeKind::File, Vec::from(initial))
    }

    /// Allocate a fresh empty directory inode, register it, and return an
    /// `Arc` to it. The directory's bytes start empty — directory entries are
    /// added later by appending dirent records to `data`.
    pub fn new_dir() -> Arc<Inode> {
        Self::alloc(InodeKind::Dir, Vec::new())
    }

    fn alloc(kind: InodeKind, data: Vec<u8>) -> Arc<Inode> {
        let ino = NEXT_INO.fetch_add(1, Ordering::SeqCst);
        let inode = Arc::new(Inode {
            ino,
            kind,
            data: IrqMutex::new(data),
        });
        REGISTRY.lock().insert(ino, inode.clone());
        inode
    }

    /// Look up an inode by number in the global registry. Returns a fresh
    /// `Arc` clone (cheap refcount bump) if found, `None` if not registered.
    pub fn lookup(ino: u64) -> Option<Arc<Inode>> {
        REGISTRY.lock().get(&ino).cloned()
    }
}

// ============================================================
// REGISTRY — global inode-number-to-inode map (the inode cache)
// ============================================================

/// The kernel's inode cache: maps inode numbers to live `Arc<Inode>` entries.
///
/// This is the mini-os analog of Linux's inode cache (historically an
/// `inode_hashtable`, in modern kernels an xarray-indexed structure). When a
/// directory entry's `ino` field is resolved during path walk, this is where
/// the actual inode struct is fetched from.
lazy_static::lazy_static! {
    pub static ref REGISTRY: IrqMutex<BTreeMap<u64, Arc<Inode>>> =
        IrqMutex::new(BTreeMap::new());
}

// ============================================================
// Directory entry (dirent) byte format
// ============================================================
//
// A directory's `data` bytes are a sequence of variable-length records:
//
//   +-----------+-----------------+----------------+
//   | name_len  | name [N bytes]  | ino [u64 LE]   |
//   +-----------+-----------------+----------------+
//      1 byte        name_len           8 bytes
//
// Total entry size is 1 + name_len + 8. Length-prefixing (instead of null
// termination) lets the parser skip an entry in O(1): read name_len, advance
// by entry_size. Linux's `struct linux_dirent64` uses the same trick via its
// `d_reclen` field.

/// Encode a single directory entry as `[name_len: u8][name: N bytes][ino: u64 LE]`.
///
/// Names longer than 255 bytes panic — Linux's NAME_MAX is 255 anyway.
pub fn encode_dirent(name: &str, ino: u64) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    assert!(
        name_bytes.len() <= u8::MAX as usize,
        "dirent name too long: {} bytes (max {})",
        name_bytes.len(),
        u8::MAX
    );
    let mut out = Vec::with_capacity(1 + name_bytes.len() + 8);
    out.push(name_bytes.len() as u8);
    out.extend_from_slice(name_bytes);
    out.extend_from_slice(&ino.to_le_bytes());
    out
}

/// Walk `dir_bytes` looking for an entry whose name matches `name`. Returns
/// the matching inode number on hit, `None` on miss, on empty input, or if
/// `dir_bytes` is truncated mid-entry.
pub fn lookup_in_dir(dir_bytes: &[u8], name: &str) -> Option<u64> {
    let name_bytes = name.as_bytes();
    let mut i = 0;
    while i < dir_bytes.len() {
        let name_len = dir_bytes[i] as usize;
        let entry_size = 1 + name_len + 8;
        if i + entry_size > dir_bytes.len() {
            return None; // truncated final entry — corrupted directory
        }
        let entry_name = &dir_bytes[i + 1..i + 1 + name_len];
        if entry_name == name_bytes {
            let ino_start = i + 1 + name_len;
            let ino_bytes: [u8; 8] = dir_bytes[ino_start..ino_start + 8].try_into().ok()?;
            return Some(u64::from_le_bytes(ino_bytes));
        }
        i += entry_size;
    }
    None
}

// ============================================================
// Path resolution — walk the directory tree by absolute path
// ============================================================

/// Resolve an absolute path to the inode it points at.
///
/// On-demand walk via the mounted filesystem trait. For each path
/// component, ask the trait to look it up in the current directory.
/// At the leaf, check the REGISTRY cache; on miss, build a fresh
/// `Arc<Inode>` from the trait's data.
///
/// Algorithm:
///   1. Path must start with `/`. Relative paths not supported.
///   2. Start at ext2 inode 2 (root, by convention).
///   3. For each component:
///        a. Check `inode_kind(current) == Dir`, else ENOTDIR.
///        b. `lookup(current, component)` returns child's inode number.
///   4. At leaf: check REGISTRY cache. Hit → return cached.
///   5. Miss: build a fresh `Arc<Inode>`. For directories, call
///      `list_dir` and re-encode in fs.rs dirent format. For files,
///      call `read_file` and store the bytes. Insert into REGISTRY,
///      return.
///
/// Subsequent calls for the same path hit the cache — no disk reads.
pub fn resolve_path(path: &str) -> Option<Arc<Inode>> {
    if !path.starts_with('/') {
        return None;
    }

    // Clone the mounted filesystem handle, then release the registry lock
    // before any disk operation that may sleep waiting for an interrupt.
    let fs = MOUNTED_FS.lock().as_ref()?.clone();

    let mut current_ino: u64 = 2; // ext2 root, by convention

    for component in path.split('/').filter(|c| !c.is_empty()) {
        // Current must be a directory.
        let kind = fs.inode_kind(current_ino)?;
        if kind != InodeKind::Dir {
            return None; // ENOTDIR
        }
        current_ino = fs.lookup(current_ino, component)?;
    }

    // Cache check.
    if let Some(cached) = REGISTRY.lock().get(&current_ino) {
        return Some(cached.clone());
    }

    // Cache miss: build fresh.
    let kind = fs.inode_kind(current_ino)?;
    let data: Vec<u8> = match kind {
        InodeKind::Dir => {
            let entries = fs.list_dir(current_ino)?;
            let mut bytes = Vec::new();
            for (name, child_ino) in entries {
                bytes.extend(encode_dirent(&name, child_ino));
            }
            bytes
        }
        InodeKind::File => fs.read_file(current_ino)?,
    };

    let inode = Arc::new(Inode {
        ino: current_ino,
        kind,
        data: IrqMutex::new(data),
    });
    REGISTRY.lock().insert(current_ino, inode.clone());
    Some(inode)
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
    pub cursor: IrqMutex<u64>,
}

impl File {
    /// Create a new open instance with cursor at position 0.
    pub fn new(inode: Arc<Inode>) -> Self {
        Self {
            inode,
            cursor: IrqMutex::new(0),
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
    table: Vec<Option<OpenObject>>,
}

#[derive(Clone)]
pub enum OpenObject {
    File(Arc<File>),
    Socket(Arc<crate::net::socket::UdpSocket>),
}

impl FdTable {
    /// Create an empty FD table.
    pub fn new() -> Self {
        Self { table: Vec::new() }
    }

    /// Copy the table entries while sharing the same open `File`s.
    /// Each cloned `Arc<File>` points at the same cursor and inode, which
    /// is the behavior `fork` needs for inherited file descriptors.
    pub fn clone_shallow(&self) -> Self {
        Self {
            table: self.table.clone(),
        }
    }

    /// Count currently-open descriptors. Used for lifecycle log lines.
    pub fn open_count(&self) -> usize {
        self.table.iter().filter(|slot| slot.is_some()).count()
    }

    /// Insert `file` at the lowest free FD number, return that number.
    ///
    /// "Lowest free" matches POSIX `open()` semantics — after closing FD 3,
    /// the next `open()` reuses 3 rather than appending at the end.
    pub fn open(&mut self, file: Arc<File>) -> u64 {
        self.open_object(OpenObject::File(file))
    }

    pub fn open_socket(&mut self, socket: Arc<crate::net::socket::UdpSocket>) -> u64 {
        self.open_object(OpenObject::Socket(socket))
    }

    fn open_object(&mut self, object: OpenObject) -> u64 {
        // Scan for the first None slot.
        for (i, slot) in self.table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(object);
                return i as u64;
            }
        }
        // No free slot — append.
        let fd = self.table.len() as u64;
        self.table.push(Some(object));
        fd
    }

    /// Look up the `File` for `fd`, if any. Returns a cloned `Arc` (cheap
    /// refcount bump) so the caller can use it without holding the FdTable.
    pub fn get(&self, fd: u64) -> Option<Arc<File>> {
        match self.table.get(fd as usize)?.as_ref()? {
            OpenObject::File(file) => Some(file.clone()),
            OpenObject::Socket(_) => None,
        }
    }

    pub fn get_socket(&self, fd: u64) -> Option<Arc<crate::net::socket::UdpSocket>> {
        match self.table.get(fd as usize)?.as_ref()? {
            OpenObject::Socket(socket) => Some(socket.clone()),
            OpenObject::File(_) => None,
        }
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
// The filesystem itself — root directory and its initial contents
// ============================================================

/// Reserved inode number for the root directory. ext2 uses inode 2.
/// (Lesson 1-3 used inode 1 for the old in-memory ROOT_INODE; that
/// was replaced by ext2 mount in Lesson 14.)
pub const ROOT_INO: u64 = 2;

// ============================================================
// Filesystem trait — VFS becomes filesystem-agnostic (Lesson 14)
// ============================================================

/// A filesystem is anything that can answer these four questions.
///
/// VFS calls trait methods; specific filesystems (ext2 now, future
/// ones later) implement them. This is the same pattern Linux uses
/// with `struct super_operations` + `struct inode_operations`
/// + `struct file_operations` (we collapse them into one for
/// simplicity).
///
/// Inode numbers (`u64`) are opaque to VFS — each filesystem
/// assigns them per its own convention. ext2 uses 2 for root.
pub trait Filesystem {
    /// What kind of thing is inode N? Returns `None` if it doesn't exist.
    fn inode_kind(&self, ino: u64) -> Option<InodeKind>;

    /// Find a child by name in a directory. Returns the child's
    /// inode number, or `None` if not found.
    fn lookup(&self, dir_ino: u64, name: &str) -> Option<u64>;

    /// List a directory's entries as (name, inode_number) pairs.
    /// Used by VFS to cache translated dirents.
    fn list_dir(&self, dir_ino: u64) -> Option<Vec<(String, u64)>>;

    /// Read a regular file's bytes. Returns `None` on I/O error
    /// or if `ino` isn't a regular file.
    fn read_file(&self, ino: u64) -> Option<Vec<u8>>;
}

/// The currently-mounted filesystem. One at a time (Linux supports
/// many simultaneous mounts; we don't need that yet).
lazy_static::lazy_static! {
    pub static ref MOUNTED_FS: IrqMutex<Option<Arc<dyn Filesystem + Send + Sync>>> =
        IrqMutex::new(None);
}

/// Install `fs` as the mounted filesystem. Subsequent `resolve_path`
/// calls dispatch to it via the trait.
pub fn mount(fs: Box<dyn Filesystem + Send + Sync>) {
    *MOUNTED_FS.lock() = Some(Arc::from(fs));
}
