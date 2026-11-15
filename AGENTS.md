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

- `src/main.rs` — kernel entry point and panic/allocation handlers.
- `src/arch/x86_64/` — x86 boot, paging, interrupts, context switching, and syscall entry.
- `src/drivers/` — device interfaces plus PCI and virtio block/network implementations.
- `src/fs/` — VFS objects and the ext2 implementation.
- `src/net/` — protocol state, sockets, and the architecture-independent frame-device owner.
- `src/syscall/` — file, socket, process, time, and user-memory syscall handlers; `src/syscall.rs` only dispatches numbers.
- `src/task/` — scheduler mechanics and process lifecycle operations.
- `crates/kernel-core/` — architecture-neutral boot code shared by x86 and ARM.
- `targets/armv7/` — position-independent ARMv7 adapters for QEMU `virt` and Raspberry Pi 2B.
- `tests/` — external behavioral test drivers and user-space fixture programs.
- `scripts/make-disk.sh` — builds an ext2 disk and installs a caller-selected program as `/init.elf`.

## Test naming

Name behavioral tests with this pattern:

```text
test_<action>_<observable_result>
```

Keep names short. Put setup and preconditions in the test body instead of encoding every `given` clause in the name.

Examples:

- `test_boot_starts_userspace`
- `test_fork_resumes_both_processes`
- `test_wait_collects_child_status`
- `test_udp_echoes_payload`
- `test_raspi2b_boots_in_qemu`

## Writing style

Lessons, tours, code comments, and chat replies all follow the same rule: **direct statements, no contrastive-opener tics.**

Banned constructions (LLM-isms the user has called out):
- "It does not X. It does Y." / "It's not X — it's Y." / "Not X, but Y."
- "stop being X and start being Y" / "stops being X, becomes Y"
- "no longer X, now Y" / "not just X — Y"
- "less about X, more about Y"
- "where X gives way to Y" / "the shift from X to Y" (as a rhetorical frame)
- "It's worth noting", "Notice that", "In essence", "At its core", "It's important to understand"
- **False equations / dramatic identity** — equating the lesson, a section, or a component with a label it is not: "This lesson is that mailbox", "the doorbell it rings to ask the GPU". The lesson explains or covers a thing; it is not the thing. Name what the lesson does ("This lesson explains the mailbox") and name a mechanism directly ("the register write that alerts the GPU"), rather than dramatizing it.

State the positive claim directly. If a contrast is genuinely load-bearing (e.g., comparing two approaches the user is choosing between), write it as a clean comparison — both sides stated, no rhetorical switchback.

Audit every draft before sending. The user reads these tics as condescension — being told what to think before being given the thought.

For lessons, tours, and explanatory prose, also follow the **`plainspoken`** skill (`~/.config/opencode/skills/plainspoken/SKILL.md`): name the actual component, action, and value; strip metaphor, personification, vague verbs, and compressed phrasing that makes the reader infer what literally happens. The contrastive-opener ban above is the narrow chat-reply rule; plainspoken is the broader technical-prose rule.

For gabix lessons, tours, and explainers, three project-specific rules apply on top of plainspoken:

- **Both architectures are first-class.** x86 and ARM are two answers to one problem; teach the problem, then both. Neither is the default with the other as a footnote.
- **Ground every behavioral claim in gabix source.** A claim about what the code or hardware does points at the gabix file and line, or the manual section.
- **Code blocks use the course dark theme.** Do not override the `pre` background or text color — `assets/lesson.css` paints `pre` dark with near-white text and styles `.c` comments grey. Tag every comment `<span class="c">`.


## Generated output and review hygiene

Follow `artifacts/README.md`. Keep compiled output in build directories and raw
run output in `target/runs/<date>-<purpose>/` (temporary scratch output may use
an OS temporary directory). Commit concise findings and reproducible tests;
do not commit routine raw logs, screenshots, packet captures, or compiled probes.
Preserve recovery-image metadata in `artifacts/pi2-images.json`; images remain
in the external store. Inspect the staged paths before each commit.
