# dup and FD-table semantics — initial exposure

User asked "what is dup?" after a probe question about `fork()` and `dup()` sharing
`struct file` instances. Explained: `dup` (and `dup2`) create a new FD number pointing at
the same underlying File. Two FDs, one File, shared cursor. `dup2(old, new)` lets you pick
the target FD number — this is the primitive the shell uses to implement `>`, `2>&1`, and
`|`.

User has been exposed but has not yet demonstrated use. Watch for whether they reach for
"dup-equivalent in mini-os" naturally when we get to pipelines (roadmap 4.6), or whether
the concept needs a refresher.

**Implications:** When we design mini-os's FD table, the implementation must support
multiple FD numbers → one `File` (for `dup`, `fork` inheritance). A naive "FD = index into
Vec<File>" works; "FD = index into Vec<&File>" or `Vec<Arc<File>>`" is the right shape.
