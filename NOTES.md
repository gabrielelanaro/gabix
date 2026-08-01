# Teaching Notes

## Teaching process principles (meta-level — how to drive a lesson)

These are the *process* rules from the `/teach` skill plus what's been earned
across Lessons 1–3. The sections below cover *style*; this one covers *method*.

### Lesson artifact
- **HTML lesson is the unit of teaching, not chat.** Socratic dialogue is
  for *design* — converging on the decisions before the lesson is written.
  The lesson file is what the user revisits; chat scrolls away.
- **Each lesson has a companion tour.** After the lesson's design lands in
  code, write `tours/tour-<NNNN>-<dash-name>.html` showing the actual Rust
  that landed for each build step, with the serial-output verification.
  Lesson = the *design* (why this shape, what alternatives); tour = the
  *code* (what it looks like in Rust, what the test printed). User asked
  for this so they can revisit "how did this translate into code" without
  re-reading the lesson. Build the tour incrementally as steps
  complete; don't wait until the lesson is finished.
- **Every code snippet in a tour carries a filename caption.** A
  `<div class="snippet-meta">` block above each `<pre>` with the file path
  (`<span class="snippet-file">src/fs.rs</span>`) and a brief what-it-is
  label (`<span class="snippet-what">— InodeKind enum</span>`). The user
  said "otherwise I lose track" — within a single step, snippets from
  different files (or different parts of the same file) get jumbled
  without per-snippet attribution. Apply to conceptual diagrams and
  output blocks too — diagram gets only `<snippet-what>`, output gets
  only `<snippet-what>` ("Boot serial output").
- **Author the lesson only after the design has converged.** Don't preemptively
  write a lesson while there are still open design questions to Socratically
  walk through. (Learned Lesson 3: started Socratic, user asked "did you
  prepare lesson 3 already?" — they wanted the artifact.)
- **Build from `./assets/`.** The shared stylesheet is a component. Before
  authoring a new lesson, check what's already reusable. Match the existing
  lesson template's structure (header, lead, sections, build steps, probe,
  footer with nav).
- **Open the lesson file for the user** with `open <path>` after writing.

### Sequence within a lesson
- **Read learning-records first** to find the zone of proximal development.
- **Tie every lesson to MISSION.md.** The mission grounds scope and order.
- **Knowledge before skills.** Teach the concept first (with diagrams and
  worked examples), then have the user practice via build steps.
- **Worked examples beat prose.** Hexdumps, byte layouts, descent diagrams
  land harder than paragraphs. When the user pushes back on a concept
  ("show me O(1) skip"), the right response is a concrete artifact, not
  more words.
- **Probe question at the end.** Retrieval practice — make them apply what
  they read to a fresh scenario (e.g., Lesson 3's hard-link probe).

### Knowledge integrity
- **Cite primary sources.** Linux source via elixir.bootlin.com, man pages
  via man7.org. Never trust parametric knowledge for specifics — link.
- **Quizzes (when added): same-length answers, no clues from formatting.**
  No length tells, no bolding the right answer.
- **Update GLOSSARY.md only when a term is earned.** A term graduates from
  "pending" to a section when the user demonstrates understanding, not when
  they first hear it.

### After the lesson
- **Create a learning-record.** ADR-style, captures the non-obvious insight
  or fork-in-the-road decision. Numbered: `0007-<dash-name>.md`, etc.
- **Update MISSION.md progress checkboxes.** `[ ]` → `[x]` as criteria land.
- **Update the reference HTML (vfs-glossary.html)** if new terms graduated.
- **Commit per build step, not per lesson.** Already covered in workflow
  preferences below — repeated here because it's also a teaching principle
  (granular commits = granular rollback = safer to let the user drive).

### Prose and voice (lessons, explainers, index teasers)
- **Write for a reader who does not yet know the material.** Lead with the
  concrete problem the mechanism solves — what the system needs to do that it
  cannot — then bring the mechanism on stage as the answer. Never open with the
  mechanism.
- **Never compress a mechanism into a noun phrase the reader must decode.**
  "Privilege on x86 is the ring carried in the code-segment selector" is
  notes-to-self, not teaching. Spell out the component, the action, and the
  value, or cut the sentence.
- **Defer all jargon** (register names, protocol acronyms, chip names, ISA
  terms) to the step where that thing is the topic, and define it there in
  plain words on first use. An index or module-teaser line stays plain: name
  the problem and the topic, nothing more. A teaser that tries to be a
  mini-lesson becomes jargon soup.
- **Apply the `plainspoken` skill** to every lesson, tour, and explainer,
  before publishing. Re-read each behavior sentence and check it names a
  component, an action, and a value.
- **Banned words (user request): "shape", "boundary", "whole"** — the user
  reads them as vague or as filler. Use the concrete noun, or cut the word
  (overview, map, split, line, interface; and "the whole X" → "the X").
- The user's words on a jargon-dense paragraph: *"this is such a jargon soup
  … this seems to be your internal monologue."* If a paragraph reads like a
  summary to yourself, rewrite it. (2026-08-01, after the module-index draft.)

## User's learning preferences (from earlier sessions)
- **Introduce new domains in their own right.** When a phase enters genuinely
  new territory (networking after disk, etc.), teach the domain from first
  principles — what problem it solves, how it works, why it's shaped the way
  it is. Parallels to prior work are seasoning, never the spine. The user
  called out the first draft of Lesson 19 for framing networking as "same as
  disk but different headers" — read it as dismissive, assuming knowledge he
  doesn't have. A comparison table is fine as a *late* section ("what you
  already own"), after the domain has been taught on its own terms.
- Expand every acronym on first use (VFS = Virtual File System, FD = File Descriptor, etc.)
- Short Socratic chunks: one paragraph max, then a probing question
- Explain terminology with intuition, not just definitions — push back hard on jargon dumps
- Prefers to write code himself when fresh; flexible when tired
- Loves visual HTML explainers for major concepts (we have 14+ in docs/)
- Follows the "Linux to read alongside" pattern from LEARNING.md

## User's workflow preferences (recorded during Lesson 2)

### Commit granularity
**Prefer separate commits per step, not bundled slices.** When implementing
a multi-step plan (e.g., Lesson 2 had steps 2.1 through 2.6), commit each
step as its own commit with a focused message. Bundling loses the
rollback granularity and obscures the build progression. If you (Claude)
realize mid-lesson that you've been holding uncommitted changes across
multiple steps, surface that and offer to commit-as-you-go from then on,
don't wait for the user to ask at the end.

### Communication style
- **No LLM-isms.** Specifically: no "It does not X. It does Y." contrastive
  openers, no "It's worth noting", no "Notice that", no "The honest version
  is", no "Get the intuition for... and it clicks into place", no
  "The benefit is enormous". Direct statements only.
- **Keep audit/status answers short.** User explicitly said long multi-page
  answers are unreadable; prefer a tight checklist with the next decision.
- **Be concrete about timelines.** "We'll get there" or "later" is too
  vague. Give specific triggers (e.g., "when the page-fault handler lands"
  or "possibly never if the goal stays educational").
- **When confused, query — don't guess.** If user pushes back saying you
  don't understand their confusion, ask probing questions until you do.
  Don't pattern-match to a generic explanation.
- **Finish work before answering questions.** If a question arrives mid-task,
  complete the current step (including verification) before pivoting to the
  question. Don't interleave.
- **Don't refer to step numbers without saying what the step does.**
  "Ready for 3.5?" is meaningless — the user doesn't have the lesson's
  numbered list in front of them and the numbers aren't anchorable in
  their head. Always pair a step number with a one-line description of
  the work: "Ready for step 3.5 (update USER_PROGRAM to call
  `open("/hello.txt")` instead of `open(NULL)` so the end-to-end test
  prints 'hello vfs' again)?" The number is fine as a label *plus* the
  description; the number alone is not.

### Code quality
- **Correctness paramount.** Don't take shortcuts that "probably won't
  deadlock." If the lock isn't irqsafe, fix it. If the validation is
  missing, add it. Small now > retrofit later.
- **Time yourself for calibration.** User wants real timing data to
  calibrate future estimates. Note start/end on timed tasks.

### Estimate calibration (data from Lesson 2)
| Task | Estimated | Actual |
|---|---|---|
| Step 2.1 (fs.rs types) | "part of 45-60 min lesson" | 90s |
| Step 2.4 (sys_read + ptr validation) | "30 min including test" | 170s |
| Step 2.5 (sys_write + user programs) | "step in lesson" | 208s |
| Step 2.6 (USER_PROGRAM + yield_now fix) | "step in lesson" | 209s |
| **Lesson 2 total** | **45-60 min** | **~14 min** |

Pattern: estimates for small, well-scoped tasks are 3-5x too high. Bias
toward "do it now" for anything under 10 minutes. Separate "design
thinking" (the long part) from "writing the code" (usually minutes).

## Project context (where the user is)
- mini-os is a bare-metal Rust kernel for learning Linux internals by re-implementing them
- Currently at end of M4 process management: fork/exec/wait done (slice 4.5), kernel relinked at high addresses (slice 4.6)
- 4.4 VFS layer is the next roadmap item — that's why we're here
- Code conventions: `#![no_std]`, panic-on-failure for now, lots of `println!` for debugging, processes have their own AddressSpace, syscalls via `syscall`/`sysretq`
- The user has strong intuition for paging, per-process address spaces, and syscalls — but no prior VFS exposure

## What to skip (the user already knows this)
- How syscalls work end-to-end (sys_fork, sys_exec, sys_wait all built)
- Per-process address spaces via PML4 (Page Map Level 4) cloning
- The "Linux to read alongside" reading-list pattern from LEARNING.md

## Tech debt / cleanup to revisit

## Spaced repetition follow-ups pending

The user wanted Round 02 of the review scheduled for 2026-07-06 (3 days
after Review 01 on 2026-07-03). cmux doesn't support durable cron jobs, so
this is hand-triggered: when the user is ready (anytime around or after
July 6), generate the lesson from the spec below.

### Round 02 spec (file: `lessons/review-02-<descriptive-name>.html`)

Same five concepts as Review 01, fresh scenarios. The point is to force
retrieval of the same ideas from different angles — not variations on the
same scenarios, which collapses to recognition.

1. **FD vs File vs Inode sharing** — instead of fork+cursor, use `dup2()` or
   shell pipeline `|`. E.g., a pipe between two processes: parent writes to
   fd=1, child inherits fd=0 reading from the same File; what does the child
   see?
2. **Path resolution inode count** — use a deeper path or trickier case
   (e.g., `/a/b/../c.txt` — does `..` count as a component? how many inodes
   visited?).
3. **Dirent byte format** — flip the question: give a byte offset, ask what
   field is being read. Or give the ino value as little-endian bytes, ask
   what number it represents.
4. **Inode numbers vs pointers** — flip the angle: what does the kernel do
   when it sees `ino=17` on disk for the first time after boot? (Allocate a
   fresh `Inode` in RAM, store the mapping `17 → Arc<Inode>` in the
   registry.) Tests the registry/cache mechanism, not just the persistence
   decision.
5. **Storage stack ordering** — same concept, different interaction. Could
   be: trace the bytes flowing back UP after a read completes, in order.
   Different from Round 01 which was the request flowing DOWN.

Format: same as Review 01 — sticky score bar, click-to-reveal cases, summary
breakdown with per-case ✓/↻ and deep links to source lessons. Mix multiple-
choice with one interactive case. Expand every acronym on first use. No
LLM-isms.

Open with `open lessons/review-02-*.html` after writing. Remind the user
this is the spaced-repetition round — low scores mean revisit, not regress.

If Round 02 lands well, Round 03 should cover the sidebar learning-records
that both reviews skipped: memory layering (frame allocator vs heap vs page
cache), allocator routing through `#[global_allocator]`, and user-pointer
validation in syscalls (records 0003, 0004, 0005).

### Build an `IrqSaveMutex<T>` wrapper (found during Lesson 2 step 2.2)
The current pattern is: every public SCHEDULER accessor manually wraps in
`x86_64::instructions::interrupts::without_interrupts(|| { ... })` before calling
`SCHEDULER.lock()`. This is correct but error-prone — adding a new accessor means
remembering to wrap, and forgetting produces a kernel-deadlock bug under timer
interrupts.

The right long-term shape is a wrapper type:

```rust
pub struct IrqSaveMutex<T>(spin::Mutex<T>);

impl<T> IrqSaveMutex<T> {
    pub const fn new(value: T) -> Self { Self(spin::Mutex::new(value)) }
    pub fn lock(&self) -> IrqSaveGuard<'_, T> { /* save RFLAGS, cli, lock */ }
}

pub struct IrqSaveGuard<'a, T> {
    inner: spin::MutexGuard<'a, T>,
    were_enabled: bool,
}

impl<'a, T> Drop for IrqSaveGuard<'a, T> {
    fn drop(&mut self) {
        // release inner, then restore RFLAGS
    }
}
```

Then `SCHEDULER` becomes `IrqSaveMutex<Scheduler>` and every `.lock()` is
automatically interrupt-safe. This is what Linux's `spinlock_t` is.

**When to do this:** after Lesson 2 is fully working. A small refactor slice on
its own (no behavior change visible to user programs, just safer internals).
Probably 1-2 hours of work including auditing all SCHEDULER access sites.

**Why we deferred:** the manual wrap works for Lesson 2's three syscall-facing
accessors. The wrapper type is the right shape but pulls focus from "first
end-to-end open/read/write/close" — the lesson's actual win.

### Audit internal SCHEDULER accesses (found during Lesson 2 step 2.2)
`yield_now`, `spawn_fork_child`, `mark_current_zombie`, etc. lock SCHEDULER
without `without_interrupts`. They currently work because their callers manage
interrupts explicitly (some do `cli`/`sti`, some run from timer context where
interrupts are already off). But this is fragile — the safety lives in the
callers' heads, not the code.

When the IrqSaveMutex wrapper lands, this gets fixed for free.

