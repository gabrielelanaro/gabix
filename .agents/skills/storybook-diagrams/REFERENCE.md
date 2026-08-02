# Storybook diagram patterns

Copy-adapt these. Each pattern has ready CSS (paste into the lesson's inline `<style>`, scoped with the given prefix) and an HTML skeleton. All CSS uses the lesson's variables so dark mode just works.

Golden reference: `lessons/7.04-dwc2-host-controller.html` (tag `golden-704-diagrams`) builds all four.

## Decision aid

| Content shape | Pattern |
|---|---|
| 3–6 steps, each a distinct **state** of the same parts (a line goes high, a signal is asserted) | **Vignettes** |
| A **handoff** — one actor programs, another executes, first reads back | **Pipeline** |
| 6+ **time-ordered** steps across 2–3 lanes (kernel/device, T0..Tn) | **Timeline** |
| One **value mapped to another** (an address transform, an encode) | **Transform** |

---

## Pattern 1 — Vignettes

Recurring component chips; per panel one **state** of the system (e.g. wire levels). Use when the teaching point is *how the state changes between steps*. A legend names the bar/line styles.

**CSS**

```css
.vignettes{margin:1.6rem 0}
.scene{display:flex;align-items:center;justify-content:center;gap:.45rem;flex-wrap:wrap;padding:.7rem .2rem}
.stub{width:14px;height:0;border-top:2px solid var(--border-bright);align-self:center}
.chip{position:relative;display:inline-flex;flex-direction:column;align-items:center;justify-content:center;min-width:56px;min-height:46px;padding:.32rem .5rem;border:2px solid var(--accent);border-radius:7px;background:var(--surface);font-family:var(--font-sans)}
.chip .nm{font-weight:700;font-size:.76rem;color:var(--text);text-align:center}
.chip .sub{font-size:.6rem;color:var(--text-dim);text-align:center}
/* pin ticks above and below the chip body */
.chip::before,.chip::after{content:"";position:absolute;left:5px;right:5px;height:4px;background:repeating-linear-gradient(90deg,var(--border-bright) 0 2px,transparent 2px 6px)}
.chip::before{top:-5px}.chip::after{bottom:-5px}
.chip.active{outline:2px solid var(--accent);outline-offset:3px}
/* give each recurring actor a distinct fill/border so the reader tracks it across panels */
.chip.arm{background:var(--accent-soft);border-color:var(--accent)}
.chip.ctrl{background:var(--surface-soft)}
.chip.dev{background:var(--surface-soft);border-color:var(--border-bright)}
/* link between two chips: labeled arrow, direction by class */
.link{display:flex;flex-direction:column;align-items:center;min-width:80px}
.link .lab{font-family:var(--font-mono);font-size:.64rem;color:var(--accent);white-space:nowrap;margin-bottom:.18rem}
.link .ln{width:48px;height:0;border-top:2px solid var(--border-bright);position:relative}
.link.write .ln{border-top-color:var(--accent)}
.link.write .ln::after{content:"";position:absolute;right:-1px;top:50%;transform:translateY(-50%);border-top:4px solid transparent;border-bottom:4px solid transparent;border-left:7px solid var(--accent)}
.link.read .ln{border-top-color:var(--accent)}
.link.read .ln::before{content:"";position:absolute;left:-1px;top:50%;transform:translateY(-50%);border-top:4px solid transparent;border-bottom:4px solid transparent;border-right:7px solid var(--accent)}
.panel{border:1px solid var(--border);border-radius:10px;background:var(--surface);padding:.85rem 1rem .95rem;margin:1rem 0}
.panel .hd{display:flex;align-items:baseline;gap:.5rem}
.panel .num{display:inline-flex;align-items:center;justify-content:center;width:1.5rem;height:1.5rem;border-radius:50%;background:var(--accent);color:#fff;font-family:var(--font-sans);font-weight:700;font-size:.76rem;flex:0 0 auto}
.panel .ttl{font-family:var(--font-sans);font-weight:700;font-size:.92rem;color:var(--text)}
.panel .src{margin-left:auto;font-family:var(--font-mono);font-size:.64rem;color:var(--text-faint)}
.panel .cap{font-family:var(--font-sans);font-size:.8rem;color:var(--text-dim);margin:.35rem 0 0;line-height:1.45}
```

The "state widget" depends on the lesson (wire levels, register bits, …). The rule: **height = one axis (e.g. level), color/texture = the other (e.g. activity)**. Example for USB wire levels:

```css
.lines{display:flex;flex-direction:column;gap:.14rem;align-items:center}
.lvl{display:flex;align-items:flex-end;gap:.22rem}
.lvl .lb{font-family:var(--font-mono);font-size:.56rem;color:var(--text-dim);width:1.5rem;text-align:right}
.lvl .bar{width:30px;height:8px;background:var(--border-bright);border-radius:1px}
.lvl.hi .bar{height:22px;background:var(--accent)}                 /* tall  = high  */
.lvl.lo .bar{height:8px}                                           /* short = low, idle */
.lvl.se0 .bar{height:8px;background:repeating-linear-gradient(45deg,var(--text-dim) 0 3px,var(--border) 3px 6px)} /* low but driven */
.legend{display:flex;flex-wrap:wrap;gap:.5rem 1.1rem;align-items:center;background:var(--surface-soft);border:1px solid var(--border);border-radius:8px;padding:.6rem .9rem;margin:0 0 1.3rem;font-family:var(--font-sans);font-size:.74rem;color:var(--text-dim)}
.lg-item{display:inline-flex;align-items:center;gap:.4rem}
.lg-bar{width:22px;border-radius:1px;display:inline-block}
```

**Skeleton**

```html
<div class="legend">
  <span class="lg-item"><span class="lg-bar" style="height:16px;background:var(--accent)"></span> line high</span>
  <span class="lg-item"><span class="lg-bar" style="height:7px;background:var(--border-bright)"></span> line low — idle</span>
  <span class="lg-item"><span class="lg-bar" style="height:7px;background:repeating-linear-gradient(45deg,var(--text-dim) 0 3px,var(--border) 3px 6px)"></span> line low — driven</span>
</div>
<div class="vignettes">
  <div class="panel">
    <div class="hd"><span class="num">1</span><span class="ttl">Power the port</span><span class="src">usb.rs:115</span></div>
    <div class="scene">
      <div class="chip arm active"><div class="nm">ARM</div><div class="sub">CPU</div></div>
      <div class="link write"><span class="lab">PORT_POWER</span><div class="ln"></div></div>
      <div class="chip ctrl"><div class="nm">DWC2</div><div class="sub">controller</div></div>
      <div class="stub"></div>
      <div class="lines"><!-- one .lvl per line; class hi | lo | se0 -->
        <div class="lvl hi"><span class="lb">VBUS</span><span class="bar"></span></div>
      </div>
      <div class="stub"></div>
      <div class="chip dev"><div class="nm">device</div><div class="sub">LAN9512</div></div>
    </div>
    <p class="cap">One sentence: who acts, what register/value, what changes electrically. Cite the line.</p>
  </div>
  <!-- more .panel per state -->
</div>
```

---

## Pattern 2 — Pipeline

A left-to-right row of stages, each showing **who acts**. Use when the point is the handoff (one actor fills registers, another executes, the first reads back).

**CSS**

```css
.flow{display:flex;align-items:stretch;gap:0;overflow-x:auto;margin:1.6rem 0}
.stage{flex:1 1 0;min-width:150px;border:1px solid var(--border);border-radius:8px;background:var(--surface);padding:.65rem .75rem;display:flex;flex-direction:column;gap:.35rem}
.stage .sh{display:flex;align-items:center;gap:.4rem}
.stage .sn{display:inline-flex;align-items:center;justify-content:center;width:1.35rem;height:1.35rem;border-radius:50%;background:var(--accent);color:#fff;font-family:var(--font-sans);font-weight:700;font-size:.66rem;flex:0 0 auto}
.stage .st{font-family:var(--font-sans);font-weight:700;font-size:.84rem;color:var(--text)}
.stage .sb{font-family:var(--font-sans);font-size:.75rem;color:var(--text-dim);line-height:1.45}
.stage .who{margin-top:auto;align-self:flex-start;font-family:var(--font-mono);font-size:.6rem;padding:.1rem .4rem;border-radius:3px;background:var(--surface-soft);color:var(--text-dim);border:1px solid var(--border)}
/* color the tag by who acts, so the ARM->DWC2 handoff is a visible switch, not just words */
.stage .who.arm{color:var(--accent);border-color:var(--accent)}
.stage .who.ctrl{color:var(--text);border-color:var(--border-bright)}
.farrow{align-self:center;color:var(--accent);font-size:1.25rem;padding:0 .3rem}
```

**Skeleton**

```html
<div class="flow">
  <div class="stage">
    <div class="sh"><span class="sn">1</span><span class="st">Describe</span></div>
    <div class="sb">What the actor does — name the registers/values.</div>
    <div class="who arm">ARM</div>
  </div>
  <div class="farrow">&rarr;</div>
  <!-- more .stage + .farrow; put the actor name in .who -->
</div>
```

---

## Pattern 3 — Timeline

A vertical list of time-stamped steps, color-banded by **lane** (e.g. kernel vs device). Use for longer flows (6+ steps) where each step is a moment, not a full system state.

**CSS**

```css
.tl{margin:1.6rem 0;border-left:2px solid var(--border-bright);padding-left:1rem}
.tl-row{position:relative;padding:.5rem .8rem;margin:.4rem 0;border-radius:6px;background:var(--surface);border:1px solid var(--border)}
.tl-row::before{content:"";position:absolute;left:-1.32rem;top:.85rem;width:.6rem;height:.6rem;border-radius:50%;background:var(--accent)}
.tl-row.lane-b .tl-tag{color:var(--text-dim)}                 /* lane B accent via tag color */
.tl-t{font-family:var(--font-mono);font-size:.66rem;color:var(--accent);font-weight:700}
.tl-tag{font-family:var(--font-mono);font-size:.62rem;color:var(--accent);float:right}
.tl-row p{margin:.2rem 0 0;font-family:var(--font-sans);font-size:.8rem;color:var(--text-dim);line-height:1.45}
```

Distinguish lanes with one consistent cue per lane — e.g. lane A steps carry `class="lane-a"` (accent tag), lane B steps `class="lane-b"` (grey tag). One axis, one channel.

**Skeleton**

```html
<div class="tl">
  <div class="tl-row lane-a"><span class="tl-t">T0</span><span class="tl-tag">kernel</span><p>What happens, grounded in source.</p></div>
  <div class="tl-row lane-b"><span class="tl-t">T4</span><span class="tl-tag">device</span><p>What the other lane does.</p></div>
  <!-- more rows; order = time -->
</div>
```

---

## Pattern 4 — Transform

Two boxes (input value, output value) joined by an arrow that carries the formula. Use for a single mapping (an address conversion, an encode).

**CSS**

```css
.aspace{margin:1.5rem 0}
.atrans{display:flex;align-items:center;justify-content:center;gap:.6rem;flex-wrap:wrap}
.abox{border:1px solid var(--border-bright);border-radius:6px;background:var(--surface);padding:.5rem .8rem;text-align:center}
.abox .alab{font-family:var(--font-sans);font-size:.68rem;color:var(--text-dim);margin-bottom:.15rem}
.abox .aval{font-family:var(--font-mono);font-size:.82rem;color:var(--accent);font-weight:600}
.aformula{font-family:var(--font-mono);font-size:.66rem;color:var(--text-dim);text-align:center;line-height:1.5;max-width:120px}
.aarrow{color:var(--accent);font-size:1.15rem}
.cap2{font-family:var(--font-sans);font-size:.78rem;color:var(--text-dim);margin:.6rem auto 0;line-height:1.45;text-align:center;max-width:480px}
```

**Skeleton**

```html
<div class="aspace">
  <div class="atrans">
    <div class="abox"><div class="alab">CPU address</div><div class="aval">0x0008_1234</div></div>
    <div class="aformula"><code>&amp; 0x3fff_ffff</code><br><code>| 0xc000_0000</code></div>
    <span class="aarrow">&rarr;</span>
    <div class="abox"><div class="alab">bus address</div><div class="aval">0xc008_1234</div></div>
  </div>
  <p class="cap2">State the invariant (what stays the same, what changes, why). Verify the arithmetic is real.</p>
</div>
```

---

## Checklist before finishing

- [ ] Tags balance (count `<div>`/`</div>`, `<p>`/`</p>`).
- [ ] Only CSS variables used — no hardcoded light-only hex that breaks dark mode.
- [ ] Every panel/stage cites a source line.
- [ ] Re-read each caption against its colors: if the caption says "low", no low thing is colored as "active". One color axis, enforced.
- [ ] A legend is present whenever a bar/line style is not self-evident.
- [ ] The lead-in prose and the code snippet are both still present; the diagram sits between them.
