# Teaching Notes

## Wi-Fi lesson preferences — 2026-11-15

Start with the actual Pi, USB dongle, router, and Mac. Use a stable isometric scene and one operation per step. Explain firmware versus Rust before register details. Keep code available through optional sections and a companion tour. The user requested maximum understandability; lesson delivery does not establish mastery.

## Clarity plus implementation detail — 2026-11-13

The user likes the revised lesson’s clarity and wants explicit connections to assembly and ARM features. Preserve the concrete scenario and readable main explanation. Add optional code sections at the relevant point, explain operands and bit fields, link the actual functions, and finish with one instruction traced through the example. Clarity should retain the implementation details needed to read the source.

## Meet the learner at the mechanism — 2026-11-12

User explicitly found the register simulator hard to follow and visually unappealing. Their gap: how the timer causes another task to run, with the scenario previously implicit. Start by naming the tasks, what each is doing, the one-core constraint, and what we want to happen. Explain timer request → CPU handler entry → scheduler choice before introducing saved registers or ARM modes. Use a stable component diagram and progressive disclosure. Ask for a short explanation to check understanding; do not infer mastery from delivering a lesson.

## Interactive learning preference — 2026-11-12

Use a step-through CPU/register/stack model for mechanisms such as preemption. Show saved values, changed values, and the source operation together. Include predictions and controlled faults with immediate feedback; label all simplifications.

## Lesson style correction — 2026-11-12

The user found lesson 3.05 difficult to parse and sensational. Explain concrete behavior before asking for predictions. Define task, scheduler, context switch, and dispatcher before using them. Give one worked example before each interactive activity. Use descriptive headings and literal button labels. Keep assembly details optional after the basic mechanism. Avoid mission/challenge framing and compressed technical summaries.

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
Universal prose rules — open with the problem; a setup sentence states its point (never withhold for a reveal); define terms at first use; banned words `shape`/`boundary`/`whole` — live in the **`plainspoken`** skill (`~/.agents/skills/plainspoken/SKILL.md`). They were merged in on 2026-08-01 so they apply in every project, not just gabix.

Gabix-specific rules — both architectures first-class; ground every claim in gabix source; code blocks on the course dark theme — are in **`AGENTS.md`** under Writing style.

Origin: the user called a module-index paragraph *"jargon soup … your internal monologue"* (2026-08-01), rejected a lesson lead that withheld its point as a reveal, and asked for the prose rules to live in plainspoken so they apply everywhere.

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

### Raspberry Pi 2 official kernel source

The known-good official image reported this kernel over UART:

```text
6.12.93+rpt-rpi-v7
```

Raspberry Pi publishes the matching packages in its archive:

- `linux-image-6.12.93+rpt-rpi-v7_6.12.93-1+rpt1_armhf.deb`
- `linux-source-6.12_6.12.93-1+rpt1_all.deb`

Archive: https://archive.raspberrypi.org/debian/pool/main/l/linux/

The upstream Raspberry Pi source tree is https://github.com/raspberrypi/linux.
Raspberry Pi's kernel build documentation identifies `bcm2709_defconfig` for
the 32-bit Raspberry Pi 2 and builds the kernel as `zImage`:

```sh
git clone --branch rpi-6.12.y https://github.com/raspberrypi/linux
cd linux
make ARCH=arm CROSS_COMPILE=arm-linux-gnueabihf- bcm2709_defconfig
make -j$(nproc) ARCH=arm CROSS_COMPILE=arm-linux-gnueabihf- zImage modules dtbs
```

For an exact reproduction of the official image, use the versioned
`6.12.93-1+rpt1` source package and its build metadata. The current Git branch
may contain a newer kernel revision.

Documentation: https://www.raspberrypi.com/documentation/computers/linux_kernel.html

## Raspberry Pi 2 physical UART smoke boot — the journey

### 2026-08-09 — Gabix prints on the real board

We reached the first reliable physical Gabix boot. The clean UART capture
contains:

```text
gabix uart smoke booted
gabix uart smoke alive
```

The Pi printed one boot marker and thousands of heartbeat markers. The
heartbeat proves that the ARM entry point ran, GPIO14 was configured for UART0,
the PL011 was accepting writes, and the kernel remained alive in its loop.

### The starting point

The Raspberry Pi 2 already had a working UART path. The old NOOBS/Raspbian
recovery card booted Linux and produced a serial console at 115200 baud. The
first operational problem was capture hygiene: several terminal monitors and
screen sessions could compete for the same serial device. We added
`scripts/monitor-pi2-uart.sh`, which creates a unique log directory, reports the
log path, rejects a second owner, and keeps exactly one screen session attached
to `/dev/cu.usbserial-0001`.

The official Bookworm image then became the storage baseline. Its UART log
reported Linux `6.12.93+rpt-rpi-v7`, Raspberry Pi 2 Model B, `ttyAMA0` at
115200, and a correctly detected SD card. That separated board, wiring, UART,
firmware, and Linux boot from the Gabix experiment.

### The storage problems

The SD card changed between `/dev/disk6` and other device numbers as macOS
mounted and detached it. Raw writes also failed with `Resource busy` while the
boot partition was mounted. Every raw operation therefore needed an explicit
`diskutil unmountDisk` step and a read-only device check immediately before the
write.

The first reader produced intermittent block mismatches and disappearing-card
failures. A retry writer using 16 MiB blocks recorded each attempt and backoff.
The newer reader was stable. Its contiguous `dd` write sustained about
5.1 MB/s and finished the 2.5 GB image in about eight minutes, compared with
roughly twenty-two minutes for the per-block retry workflow.

The `dd` exit status alone was insufficient evidence. The first fast write
returned status 0 but its full read-back hash differed from the image. The
successful run used targeted read-back of the mounted FAT boot files. The
hashes of `KERNEL7.IMG`, `CONFIG.TXT`, `bootcode.bin`, `start.elf`, and
`fixup.dat` all matched the prepared image before the card was moved to the Pi.

### The binary comparison

The official Linux `KERNEL7.IMG` was 7,817,088 bytes and began with the ARM
Linux zImage startup/header data. The Gabix smoke image was a 260-byte raw ARM
binary. Its first instruction was `cpsid if`; it then parked three cores,
configured GPIO14/15, programmed PL011, printed a message, and repeated the
heartbeat. Raspberry Pi firmware can load a raw `kernel7.img`; the Pi 2
default filename and the physical load address are `kernel7.img` and `0x8000`.

QEMU provided a fast binary feedback loop. The smoke image linked for QEMU at
`0x10000` printed both markers under the `raspi2b` machine. That test proved the
ARM instructions and entry path. QEMU did not expose the physical UART clock
contract, so it could not reveal the baud-rate mistake.

### The actual failure

The first derived image replaced `KERNEL7.IMG` while retaining the official
`CONFIG.TXT`. The official configuration did not contain
`init_uart_clock=3000000`. Gabix's [uart_smoke.S](targets/armv7/src/uart_smoke.S)
hardcoded PL011 `IBRD=1` and `FBRD=40`, which produce approximately 115200 baud
with a 3 MHz UART clock.

Current Raspberry Pi firmware uses a 48 MHz initial PL011 clock. The same
divisors therefore produced approximately 1,846,154 baud. The Mac monitor was
listening at 115200, so the log filled with repeating bytes such as `ec`, `6c`,
`d8`, and `ff`. The stream was the smoke kernel running too quickly for the
monitor to decode. Linux worked because its PL011 driver reads the actual clock
and calculates the divisor at runtime.

This changed the interpretation of the symptom. The kernel was loading and
running; its UART contract was wrong.

### The correction and proof

The corrected image was derived from the official image with the Gabix kernel
and the project boot configuration, including:

```ini
arm_64bit=0
kernel=kernel7.img
kernel_address=0x8000
enable_uart=1
init_uart_clock=3000000
```

The self-verifying builder is
`scripts/make-pi2-uart-smoke-from-official.py`. It replaces files through their
FAT32 chains, updates directory sizes, and reads each replacement back before
reporting success. The final image was written to the confirmed `/dev/rdisk6`
with 149 successful 16 MiB blocks and `FAST_DD_STATUS:0`.

The final clean monitor used one owner at 115200 baud. The physical log then
showed one `gabix uart smoke booted` marker and 4,080 `gabix uart smoke alive`
markers. The Pi 2 was running Gabix on real hardware.

### Baseline for future experiments

- Keep one UART monitor owner and start a fresh unique log before every power
  cycle.
- Preserve the official image as the source baseline.
- Verify the local image hash and the individual FAT boot files.
- Confirm the exact raw target after the card is inserted; unmount it before
  writing.
- Show progress, throughput, ETA, and a final status for every long write or
  read.
- Use a slow heartbeat for human-readable experiments. The current loop emits
  roughly 450–480 messages per second because the UART transmission time is
  only about 2.1 ms per 24-byte message.
- For a production-quality Gabix boot path, query the UART clock through the
  Raspberry Pi firmware mailbox or calculate the divisor from an explicit
  clock contract instead of relying only on `init_uart_clock`.

## 2026-11-15 — SD lesson context and visual preference

The user requested an interactive isometric microworld showing actual components,
data flow, and clocks. Begin with the concrete source/destination scenario: a
512-byte RAM buffer and a numbered card sector. Explicitly distinguish CPU PIO
copies from device access to RAM through VirtIO descriptors. Include optional
register and ARM-instruction detail, plus polling versus interrupt wakeup.
Questions show these are teaching needs; understanding has not yet been assessed.
