# Storybook diagram patterns

Copy-adapt these. Each pattern has ready CSS (paste into the lesson's inline `<style>`, scoped with the given prefix) and an HTML skeleton. All CSS uses the lesson's variables so dark mode just works.

Golden reference: `learn/lessons/7.04-dwc2-host-controller.html` (tag `golden-704-scenes`) builds the vignette, flow-scene, and mapping-scene patterns.

## The one rule

A storybook diagram is a **scene of recurring chips** — the components (CPU, controller, device, memory) drawn as little ICs that stay identical wherever they appear, connected by labeled flow arrows. Text-card lists and bare boxes are not scenes; do not emit them. Put the actors on stage as chips, connect them, and let color carry one axis only.

## Shared primitives

Every scene pattern uses these. Paste them once into the lesson's `<style>`.

```css
.chip{position:relative;display:inline-flex;flex-direction:column;align-items:center;justify-content:center;min-width:56px;min-height:46px;padding:.32rem .5rem;border:2px solid var(--accent);border-radius:7px;background:var(--surface);font-family:var(--font-sans)}
.chip .nm{font-weight:700;font-size:.76rem;color:var(--text);text-align:center}
.chip .sub{font-size:.6rem;color:var(--text-dim);text-align:center}
.chip::before,.chip::after{content:"";position:absolute;left:5px;right:5px;height:4px;background:repeating-linear-gradient(90deg,var(--border-bright) 0 2px,transparent 2px 6px)}
.chip::before{top:-5px}.chip::after{bottom:-5px}
.chip.arm{background:var(--accent-soft);border-color:var(--accent)}
.chip.ctrl{background:var(--surface-soft)}
.chip.dev{background:var(--surface-soft);border-color:var(--border-bright)}
.chip.mem{background:var(--surface-soft);border-color:var(--border-bright);border-style:dashed}
.chip.active{outline:2px solid var(--accent);outline-offset:3px}
.link{display:flex;flex-direction:column;align-items:center;min-width:84px}
.link .lab{font-family:var(--font-mono);font-size:.64rem;color:var(--accent);white-space:nowrap;margin-bottom:.18rem}
.link .ln{width:48px;height:0;border-top:2px solid var(--border-bright);position:relative}
.link.write .ln{border-top-color:var(--accent)}
.link.write .ln::after{content:"";position:absolute;right:-1px;top:50%;transform:translateY(-50%);border-top:4px solid transparent;border-bottom:4px solid transparent;border-left:7px solid var(--accent)}
.link.read .ln{border-top-color:var(--accent)}
.link.read .ln::before{content:"";position:absolute;left:-1px;top:50%;transform:translateY(-50%);border-top:4px solid transparent;border-bottom:4px solid transparent;border-right:7px solid var(--accent)}
.link .lab2{font-family:var(--font-mono);font-size:.56rem;color:var(--text-dim);white-space:nowrap;margin-top:.18rem}
.vlink{display:flex;flex-direction:column;align-items:center;justify-content:center}
.vlink .vlab{font-family:var(--font-mono);font-size:.6rem;color:var(--accent);margin-bottom:.1rem;white-space:nowrap}
.vlink .varrow{width:0;height:16px;border-left:2px solid var(--accent);position:relative}
.vlink .varrow::after{content:"";position:absolute;left:50%;bottom:-1px;transform:translateX(-50%);border-left:4px solid transparent;border-right:4px solid transparent;border-top:6px solid var(--accent)}
.cap2{font-family:var(--font-sans);font-size:.78rem;color:var(--text-dim);margin:.6rem auto 0;line-height:1.45;text-align:center;max-width:480px}
```

Chip variants give each recurring actor a distinct identity (`.arm` accent-soft, `.ctrl` surface-soft, `.dev` dim border, `.mem` dashed). `.link.write` arrow points right; `.link.read` points left. `.vlink` is a downward branch. `.lab2` is a second, dimmer line under a link.

## Decision aid

| Content shape | Pattern |
|---|---|
| 3–6 steps, each a distinct **state** of the same parts (a line goes high, a signal is asserted) | **Vignettes** |
| A **handoff / topology** — one actor programs, another executes, first reads back; several parts exchange data | **Flow scene** |
| 6+ **time-ordered** steps across 2–3 lanes (kernel/device, T0..Tn) | **Timeline** |
| One thing seen **two ways** (two addresses for one buffer, two views of one structure) | **Mapping scene** |

---

## Pattern 1 — Vignettes

Recurring chips; per panel one **state** of the system. Use when the teaching point is *how the state changes between steps*. Mark the chip that acts in each panel with `.active` (an accent outline) so the reader sees who does what. A legend names the bar/line styles and the chip actors.

**CSS** (shared primitives + this)

```css
.vignettes{margin:1.6rem 0}
.scene{display:flex;align-items:center;justify-content:center;gap:.45rem;flex-wrap:wrap;padding:.7rem .2rem}
.stub{width:14px;height:0;border-top:2px solid var(--border-bright);align-self:center}
.panel{border:1px solid var(--border);border-radius:10px;background:var(--surface);padding:.85rem 1rem .95rem;margin:1rem 0}
.panel .hd{display:flex;align-items:baseline;gap:.5rem}
.panel .num{display:inline-flex;align-items:center;justify-content:center;width:1.5rem;height:1.5rem;border-radius:50%;background:var(--accent);color:#fff;font-family:var(--font-sans);font-weight:700;font-size:.76rem;flex:0 0 auto}
.panel .ttl{font-family:var(--font-sans);font-weight:700;font-size:.92rem;color:var(--text)}
.panel .src{margin-left:auto;font-family:var(--font-mono);font-size:.64rem;color:var(--text-faint)}
.panel .cap{font-family:var(--font-sans);font-size:.8rem;color:var(--text-dim);margin:.35rem 0 0;line-height:1.45}
.legend{display:flex;flex-wrap:wrap;gap:.5rem 1.1rem;align-items:center;background:var(--surface-soft);border:1px solid var(--border);border-radius:8px;padding:.6rem .9rem;margin:0 0 1.3rem;font-family:var(--font-sans);font-size:.74rem;color:var(--text-dim)}
.lg-item{display:inline-flex;align-items:center;gap:.4rem}
.lg-bar{width:22px;border-radius:1px;display:inline-block}
.lg-chip{display:inline-block;width:18px;height:14px;border:2px solid var(--accent);border-radius:3px;background:var(--accent-soft);vertical-align:middle}
.lg-chip.ctrl{background:var(--surface-soft);border-color:var(--accent)}
.lg-chip.dev{background:var(--surface-soft);border-color:var(--border-bright)}
```

The state widget is lesson-specific. Rule: **height = one axis, color/texture = the other** — never color a low line as "active". Example (USB wire levels):

```css
.lines{display:flex;flex-direction:column;gap:.14rem;align-items:center}
.lvl{display:flex;align-items:flex-end;gap:.22rem}
.lvl .lb{font-family:var(--font-mono);font-size:.56rem;color:var(--text-dim);width:1.5rem;text-align:right}
.lvl .bar{width:30px;height:8px;background:var(--border-bright);border-radius:1px}
.lvl.hi .bar{height:22px;background:var(--accent)}
.lvl.lo .bar{height:8px}
.lvl.se0 .bar{height:8px;background:repeating-linear-gradient(45deg,var(--text-dim) 0 3px,var(--border) 3px 6px)}
```

**Skeleton**

```html
<div class="legend">
  <span class="lg-item"><span class="lg-chip"></span> ARM</span>
  <span class="lg-item"><span class="lg-chip ctrl"></span> controller</span>
  <span class="lg-item"><span class="lg-chip dev"></span> device</span>
  <span class="lg-item"><span class="lg-bar" style="height:16px;background:var(--accent)"></span> high</span>
  <span class="lg-item"><span class="lg-bar" style="height:7px;background:var(--border-bright)"></span> low — idle</span>
  <span class="lg-item"><span class="lg-bar" style="height:7px;background:repeating-linear-gradient(45deg,var(--text-dim) 0 3px,var(--border) 3px 6px)"></span> low — driven</span>
</div>
<div class="vignettes">
  <div class="panel">
    <div class="hd"><span class="num">1</span><span class="ttl">Title</span><span class="src">file.rs:NN</span></div>
    <div class="scene">
      <div class="chip arm active"><div class="nm">ARM</div><div class="sub">CPU</div></div>
      <div class="link write"><span class="lab">VALUE</span><div class="ln"></div></div>
      <div class="chip ctrl"><div class="nm">DWC2</div><div class="sub">controller</div></div>
      <div class="stub"></div>
      <div class="lines"><div class="lvl hi"><span class="lb">L1</span><span class="bar"></span></div></div>
      <div class="stub"></div>
      <div class="chip dev"><div class="nm">device</div><div class="sub">target</div></div>
    </div>
    <p class="cap">One sentence: who acts, what value, what changes. Cite the line.</p>
  </div>
</div>
```

---

## Pattern 2 — Flow scene

The actors as **chips in a spatial scene**, connected by labeled, **numbered** flow arrows. Use for a handoff or a topology (one actor programs, another executes, first reads back; several parts exchange data). Put every actor on stage as a chip; branch secondary actors (memory, a bus) off the spine with vertical links; number the flows in sequence.

**CSS** (shared primitives + this grid)

```css
.xfer-scene{display:grid;grid-template-columns:auto auto auto auto auto;align-items:center;justify-items:center;justify-content:center;gap:.35rem .4rem}
```

**Skeleton** — ARM describes + fires (①②), DWC2 executes (③ wire to device, ③ DMA to memory), ARM reads back (④):

```html
<div class="xfer-scene">
  <div class="chip arm" style="grid-column:1;grid-row:1"><div class="nm">ARM</div><div class="sub">CPU</div></div>
  <div class="link write" style="grid-column:2;grid-row:1"><span class="lab">① ② describe + GO</span><div class="ln"></div><span class="lab2">registers + ENABLE</span></div>
  <div class="chip ctrl" style="grid-column:3;grid-row:1"><div class="nm">DWC2</div><div class="sub">controller</div></div>
  <div class="link write" style="grid-column:4;grid-row:1"><span class="lab">③ wire</span><div class="ln"></div><span class="lab2">tokens</span></div>
  <div class="chip dev" style="grid-column:5;grid-row:1"><div class="nm">device</div><div class="sub">endpoint</div></div>
  <div class="vlink" style="grid-column:3;grid-row:2"><span class="vlab">③ DMA</span><div class="varrow"></div></div>
  <div class="chip mem" style="grid-column:3;grid-row:3"><div class="nm">memory</div><div class="sub">buffer</div></div>
</div>
<p class="cap2">④ The result returns to the ARM. Cite a source line per numbered flow.</p>
```

Place chips with `grid-column`/`grid-row` so secondary actors branch cleanly under their parent. Never collapse this into text cards — the chips on stage are the point.

---

## Pattern 3 — Timeline

A vertical list of time-stamped steps, color-banded by **lane** (e.g. kernel vs device). Use for longer flows (6+ steps) where each step is a moment, not a full system state. (This pattern is the exception to "chips on stage": a long log of moments reads better as a lane-colored list than as dozens of chips.)

**CSS**

```css
.tl{margin:1.6rem 0;border-left:2px solid var(--border-bright);padding-left:1rem}
.tl-row{position:relative;padding:.5rem .8rem;margin:.4rem 0;border-radius:6px;background:var(--surface);border:1px solid var(--border)}
.tl-row::before{content:"";position:absolute;left:-1.32rem;top:.85rem;width:.6rem;height:.6rem;border-radius:50%;background:var(--accent)}
.tl-row.lane-b::before{background:var(--border-bright)}
.tl-t{font-family:var(--font-mono);font-size:.66rem;color:var(--accent);font-weight:700}
.tl-row.lane-b .tl-t{color:var(--text-dim)}
.tl-tag{font-family:var(--font-mono);font-size:.62rem;color:var(--accent);float:right}
.tl-row.lane-b .tl-tag{color:var(--text-dim)}
.tl-row p{margin:.2rem 0 0;font-family:var(--font-sans);font-size:.8rem;color:var(--text-dim);line-height:1.45}
```

Lanes are the ONE color axis: lane A = accent (dot, time, tag); lane B = neutral grey. No second hue.

**Skeleton**

```html
<div class="tl">
  <div class="tl-row lane-a"><span class="tl-t">T0</span><span class="tl-tag">kernel</span><p>What happens, grounded in source.</p></div>
  <div class="tl-row lane-b"><span class="tl-t">T4</span><span class="tl-tag">device</span><p>What the other lane does.</p></div>
  <!-- more rows; order = time -->
</div>
```

---

## Pattern 4 — Mapping scene

Two (or more) chips that each represent a **viewpoint**, both reaching the **same thing**. Use when one thing is seen two ways — two addresses for one buffer, two interpretations of one structure. Do not draw two bare boxes with an arrow; draw the viewpoints as chips converging on the shared object.

**CSS** (shared primitives + this grid)

```css
.addrs{display:grid;grid-template-columns:1fr 1fr;align-items:center;justify-items:center;gap:.35rem 2rem;margin:1.5rem 0}
.memblock{grid-column:1/-1;border:2px dashed var(--border-bright);border-radius:8px;background:var(--surface-soft);padding:.5rem 1.1rem;text-align:center;font-family:var(--font-sans);font-size:.8rem;color:var(--text)}
.memblock .aformula{display:block;font-family:var(--font-mono);font-size:.66rem;color:var(--accent);margin-top:.2rem}
```

**Skeleton** — CPU and DMA both reach one buffer at different addresses:

```html
<div class="addrs">
  <div class="chip arm"><div class="nm">ARM</div><div class="sub">view A<br>0x0008_1234</div></div>
  <div class="chip ctrl"><div class="nm">DWC2 · DMA</div><div class="sub">view B<br>0xc008_1234</div></div>
  <div class="vlink"><span class="vlab">reaches</span><div class="varrow"></div></div>
  <div class="vlink"><span class="vlab">reaches</span><div class="varrow"></div></div>
  <div class="memblock">the shared thing — one place, two views<span class="aformula">&amp; 0x3fff_ffff | 0xc000_0000</span></div>
</div>
<p class="cap2">State the invariant (what stays the same, what changes, why). Verify any arithmetic is real.</p>
```

---

## Checklist before finishing

- [ ] **Chips on stage, not text cards.** Every actor is a `.chip`; no diagram is just a row of cards or two bare boxes.
- [ ] Tags balance (count `<div>`/`</div>`, `<p>`/`</p>`).
- [ ] Only CSS variables used — no hardcoded light-only hex that breaks dark mode.
- [ ] Every step/flow/panel cites a source line you have opened and confirmed holds the named register/value.
- [ ] Re-read each caption against its colors: if a caption says "low", no low thing is colored as "active". One color axis, enforced.
- [ ] A legend is present whenever a bar/line style is not self-evident.
- [ ] Re-read each caption's technical claims (bit numbers, address semantics, what a constant does) and verify them against the code — a caption that mis-describes a constant is a defect.
- [ ] The lead-in prose and the code snippet are both still present; the diagram sits between them.
