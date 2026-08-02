---
name: storybook-diagrams
description: Turn step-by-step mechanism prose in gabix lessons into CSS "storybook" diagrams — vignettes, pipelines, timelines, transforms — drawn with the lesson stylesheet. Use when a lesson section explains a register sequence, handshake, init flow, data path, or value transform as an ordered sequence and reads as a wall of prose or a numbered list.
---

# Storybook diagrams for lessons

## When to use

A lesson section that explains a mechanism **as an ordered sequence** — register writes, a handshake, a bring-up flow, a data path, an address transform — and currently sits as dense prose or a flat numbered list. Replace the step-by-step prose with a diagram. Keep a one-line lead-in and the source code snippet; the diagram goes between them.

Do **not** use this for: a reference list of fields (a table is better), a single concept with no ordering, or a comparison of two options (prose or a split-view serves those).

## Three principles (learned the hard way)

1. **Separate players from interfaces.** Components (CPU, controller, device) are drawn as boxes/chips and **recur unchanged in every panel** — they are the "characters". Interfaces between them (a register, a wire) are drawn as connectors or gates, never as peer boxes. Do not list a register as if it were a component.

2. **Color encodes exactly one axis.** Pick the single axis the reader most needs (state, level, owner, direction) and let color mean *only* that. If a caption says "drives low", a low line must be the low color — never color a low line as "active". When two things differ on a second axis, use a second visual channel (height, texture, arrowhead direction), not a second color. **Add a legend** whenever an encoding is not self-evident.

3. **Ground every step in source.** Each panel or stage cites the file and line it depicts (`usb.rs:123`). The diagram illustrates the code; the code snippet stays beneath it.

## Workflow

1. Read the section's prose **and** its code snippet. List the **players** (mark each as component vs interface) and the **ordered states or steps**.
2. Pick a pattern by shape (templates in [REFERENCE.md](REFERENCE.md)):
   - Few steps, each a distinct **state** of one system → **vignettes** (recurring chips + per-panel state).
   - A linear **handoff** / who-does-what → **pipeline** (stages + actor tags).
   - Many **time-ordered** steps across few lanes → **timeline** (T0..Tn, lane-colored).
   - One **value → another** → **transform** (boxes + formula arrow).
3. Choose the single color axis; define the states; add a legend if any state is not obvious.
4. Emit HTML with a **scoped inline `<style>`** using the lesson's CSS variables (`--accent`, `--border`, `--surface`, `--text-dim`, …). Do **not** override the global `pre` block (the course dark theme styles it). Tag code comments `<span class="c">`.
5. Keep the lead-in prose and the code snippet; place the diagram between them.
6. **Verify** before finishing: tags balance; dark mode works because only variables (not hardcoded light hex) are used; every step cites a source line; re-read each caption against its colors — if a caption says "low", no low thing is colored as "active".

## Conventions

- Use the lesson's CSS variables, never raw colors that break dark mode.
- Reuse the established motifs (numbered circle badges, chip pin-ticks, accent arrows) so the whole lesson reads in one visual language.
- Scope new styles inline in the lesson's existing `<style>` block, with a distinctive prefix (`.vignettes`, `.flow`, …), so they do not leak across lessons.
- Follow `AGENTS.md` writing rules for any prose: plainspoken, mechanism-first, both-architectures-first-class where relevant.

Copy-adapt the four patterns from [REFERENCE.md](REFERENCE.md). Each comes with ready CSS and an HTML skeleton.
