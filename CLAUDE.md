# mini-os

A bare-metal Linux-like OS written in Rust, built to learn Linux internals by re-implementing them.

## Project files

### `LEARNING.md`

This file is the project's spine. It serves three purposes:

1. **Plan** — the 4-month roadmap (skeleton → memory → scheduling → syscalls/VFS), with checkboxes for each milestone.
2. **Reference** — toolchain setup, books, and Linux source files to read alongside each milestone.
3. **Progress log** — append-only session log at the bottom of the file.

**Working with `LEARNING.md`:**

- **At the start of a session:** read the "Roadmap" and "Progress Log" to orient on where the project is.
- **At the end of a session:** add a new entry under "Progress Log" (most recent at top) describing what was built, which checkboxes flipped (`[ ]` → `[x]`), any blockers, and where to pick up next time. Use the template comment in the file.
- **When completing an item:** flip its checkbox in the roadmap. Don't reorganize roadmap structure mid-milestone — that's churn.
- **Milestones (M0–M5):** when a milestone is reached, mention it in the log entry and consider `git tag`-ing the commit.
- **If plans change:** update the roadmap directly. The file should reflect current intent, not history — history lives in git and the progress log.

### Other files

(Nothing else yet. As the kernel takes shape, update this section with the layout: `kernel/`, `boot/`, etc.)
