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

## Writing style

Lessons, tours, code comments, and chat replies all follow the same rule: **direct statements, no contrastive-opener tics.**

Banned constructions (LLM-isms the user has called out):
- "It does not X. It does Y." / "It's not X — it's Y." / "Not X, but Y."
- "stop being X and start being Y" / "stops being X, becomes Y"
- "no longer X, now Y" / "not just X — Y"
- "less about X, more about Y"
- "where X gives way to Y" / "the shift from X to Y" (as a rhetorical frame)
- "It's worth noting", "Notice that", "In essence", "At its core", "It's important to understand"

State the positive claim directly. If a contrast is genuinely load-bearing (e.g., comparing two approaches the user is choosing between), write it as a clean comparison — both sides stated, no rhetorical switchback.

Audit every draft before sending. The user reads these tics as condescension — being told what to think before being given the thought.

For lessons, tours, and explanatory prose, also follow the **`plainspoken`** skill (`~/.config/opencode/skills/plainspoken/SKILL.md`): name the actual component, action, and value; strip metaphor, personification, vague verbs, and compressed phrasing that makes the reader infer what literally happens. The contrastive-opener ban above is the narrow chat-reply rule; plainspoken is the broader technical-prose rule.
