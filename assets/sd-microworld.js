(() => {
  'use strict';
  const $ = (id) => document.getElementById(id);
  const sourceRoot = 'https://github.com/gabrielelanaro/gabix/blob/7f9b66f487fd68bf76012e01bdaabd664bc669a3/';
  const sdSource = `${sourceRoot}targets/armv7/src/raspi2b/sdhci.rs`;
  const steps = [
    {
      title: 'Prepare the SD clock', label: 'Setup', state: 'Preparing the controller',
      body: '<p>The driver asks firmware for the controller’s source clock rate: <strong>200 MHz</strong> on our Pi.</p><p>It calculates and writes the divider. Initialization uses at most 400 kHz; after initialization, divider 4 selects <strong>25 MHz</strong> for transfers.</p>',
      role: '<strong>CPU:</strong> queries firmware and writes configuration.<br><strong>Clock logic:</strong> produces the repeating SD signal.',
      source: `<a href="${sourceRoot}targets/armv7/src/raspi2b/mailbox.rs#L26-L31">mailbox.rs:26–31</a> · <a href="${sdSource}#L266-L281">sdhci.rs:266–281</a>`,
      nodes: ['cpu', 'clock', 'controller'], flows: ['register', 'clock'],
    },
    {
      title: 'Select the destination sector', label: 'Command', state: 'CMD24 → sector 8192',
      body: '<p>The CPU writes <strong>8192</strong> into the controller’s command-argument register, then requests <strong>CMD24</strong>: write one block.</p><p>The controller sends the command to the SD card. This number identifies the destination sector on this SDHC/SDXC card.</p>',
      role: '<strong>In RAM:</strong> the 512 source bytes remain in the kernel’s buffer.<br><strong>At the controller:</strong> the argument identifies the card sector.',
      source: `<a href="${sdSource}#L143-L165">sdhci.rs:143–165</a> · SD specification, CMD24`,
      nodes: ['cpu', 'controller', 'card'], flows: ['register', 'command', 'clock'],
    },
    {
      title: 'The CPU loads four bytes from RAM', label: 'Source bytes', state: 'RAM → CPU register',
      body: '<p>The controller reports that its write buffer is ready. The CPU takes the next <strong>four bytes</strong> from the source buffer in RAM.</p><p>The Rust slice <code>data</code> tells the CPU where those bytes are. They form one 32-bit word for the next store.</p>',
      role: '<strong>RAM pointer:</strong> used by the CPU.<br><strong>Sector number:</strong> already sent as the card destination.',
      source: `<a href="${sdSource}#L167-L170">sdhci.rs:167–170</a>`,
      nodes: ['ram', 'cpu'], flows: ['ram'],
    },
    {
      title: 'The CPU writes that word to DATA', label: 'Programmed I/O', state: 'CPU → controller FIFO',
      body: '<p>The CPU stores the word at the controller’s <strong>DATA register</strong>. The hardware places it in a FIFO: a buffer of words waiting to be transferred.</p><p>The driver repeats the RAM load and DATA store <strong>128 times</strong>: 128 × 4 bytes = 512 bytes.</p>',
      role: '<strong>Current transfer mode:</strong> programmed I/O (PIO).<br>The CPU supplies the words. The controller receives no source RAM address.',
      source: `<a href="${sdSource}#L167-L172">sdhci.rs:167–172</a> · <a href="${sdSource}#L320-L321">volatile DATA store</a>`,
      nodes: ['ram', 'cpu', 'controller'], flows: ['ram', 'register', 'data', 'clock'],
    },
    {
      title: 'The controller sends bits to the card', label: 'Electrical transfer', state: 'DATA bits + CLK timing',
      body: '<p>The controller converts queued words into serial bits on the <strong>DATA connection</strong>. Its clock logic produces the <strong>CLK signal</strong> that times the transfer.</p><p>The card receives the block and performs its internal programming. This hardware work can overlap the CPU feeding the FIFO.</p>',
      role: '<strong>Clock:</strong> provides timing.<br><strong>DATA:</strong> carries the payload bits.<br>The CPU continues running the driver’s stores and status checks.',
      source: `<a href="${sdSource}#L159-L173">PIO transfer selection and completion</a> · BCM2835 manual §5, DATA/CMDTM`,
      nodes: ['clock', 'controller', 'card'], flows: ['data', 'clock'],
    },
    {
      title: 'Check that the write finished', label: 'Completion', state: 'Card ready → return success',
      body: '<p>The CPU polls the controller’s completion flags. Then the driver sends <strong>CMD13</strong> to ask for the card’s status.</p><p>Success requires no reported card errors, <strong>ready for data</strong>, and the card back in <strong>transfer state</strong>. The function can then return.</p>',
      role: '<strong>Our test then:</strong> reads back all 512 bytes, restores the original sector, and checks both neighbors. Success here does not establish power-loss atomicity.',
      source: `<a href="${sdSource}#L172-L197">sdhci.rs:172–197</a> · <a href="../docs/pi2-sd-writes.md">physical verification</a>`,
      nodes: ['cpu', 'controller', 'card'], flows: ['command', 'register'],
    },
  ];
  const descriptions = {
    cpu: 'CPU: executes the kernel instructions. In this PIO path it loads RAM words, stores them into DATA, and polls status flags.',
    ram: 'RAM: holds the kernel’s source array. The CPU uses its pointer to load the next word; the controller never receives this pointer.',
    controller: 'SD controller: hardware inside the Pi’s main chip. It accepts register accesses, buffers words, and sends SD commands, data, and clock timing.',
    clock: 'Clock logic: divides the firmware-controlled source frequency and generates the repeating CLK signal. This is separate from the CPU instruction clock.',
    card: 'SD card: receives a destination sector number and data, then programs its storage. Its internal flash-management details are omitted from this drawing.',
  };
  let currentStep = 0;
  let animate = false;
  function renderStep() {
    const step = steps[currentStep];
    $('sd-step-number').textContent = `Step ${currentStep + 1} of ${steps.length} · ${step.label}`;
    $('sd-step-title').textContent = step.title;
    $('sd-step-body').innerHTML = step.body;
    $('sd-step-role').innerHTML = step.role;
    $('sd-step-source').innerHTML = step.source;
    $('sd-world-state').textContent = step.state;
    $('sd-prev').disabled = currentStep === 0;
    $('sd-next').textContent = currentStep === steps.length - 1 ? 'Start again ↺' : 'Next →';
    document.querySelectorAll('.sd-stage .node').forEach(node => node.classList.toggle('active', step.nodes.includes(node.id.replace('sd-node-', ''))));
    document.querySelectorAll('.sd-stage .flow').forEach(flow => flow.classList.toggle('active', step.flows.includes(flow.id.replace('sd-flow-', ''))));
    document.querySelectorAll('#sd-word-cells .bit').forEach(cell => cell.classList.toggle('current', currentStep === 2 || currentStep === 3));
    document.querySelectorAll('.sd-memory-cell').forEach(cell => cell.classList.toggle('written', currentStep === 5));
    document.querySelectorAll('#sd-step-dots button').forEach((button, index) => {
      if (index === currentStep) button.setAttribute('aria-current', 'step');
      else button.removeAttribute('aria-current');
    });
  }
  steps.forEach((step, index) => {
    const button = document.createElement('button');
    button.type = 'button'; button.setAttribute('aria-label', `Step ${index + 1}: ${step.title}`);
    button.addEventListener('click', () => { currentStep = index; renderStep(); });
    $('sd-step-dots').append(button);
  });
  $('sd-prev').addEventListener('click', () => { currentStep = Math.max(0, currentStep - 1); renderStep(); });
  $('sd-next').addEventListener('click', () => { currentStep = (currentStep + 1) % steps.length; renderStep(); });
  $('sd-play').addEventListener('click', () => {
    animate = !animate; $('sd-stage').classList.toggle('running', animate);
    $('sd-play').textContent = animate ? 'Pause flow' : 'Animate flow';
    $('sd-play').setAttribute('aria-pressed', String(animate));
  });
  Object.entries(descriptions).forEach(([id, description]) => {
    const node = $(`sd-node-${id}`);
    const inspect = () => { $('sd-inspect').textContent = description; };
    node.addEventListener('click', inspect);
    node.addEventListener('keydown', event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); inspect(); } });
  });
  document.querySelectorAll('[data-quiz]').forEach(button => button.addEventListener('click', () => {
    const answers = {
      address: 'In this PIO path the CPU uses the RAM pointer. The controller receives the bytes through DATA stores. VirtIO descriptors use a different mechanism—see the comparison below.',
      fifo: 'Yes. The CPU loads four bytes from RAM and writes one word into DATA, repeated 128 times. The controller transfers those bytes to the selected card sector.',
      sector: 'The sector number identifies the destination on the card. The source bytes still have to reach the controller through the CPU’s DATA stores.',
    };
    $('sd-copy-feedback').textContent = answers[button.dataset.quiz];
  }));
  let divider = 4;
  let sourceCycles = 0;
  const formatRate = value => Number.isInteger(value) ? String(value) : value.toFixed(5).replace(/0+$/, '').replace(/\.$/, '');
  function setDivider(value) { divider = value; sourceCycles = 0; $('sd-divider').value = String(value); renderClock(); }
  function renderClock() {
    const rate = 200 / (2 * divider);
    const period = 2 * divider;
    $('sd-divisor-number').textContent = divider;
    $('sd-clock-rate').textContent = formatRate(rate);
    $('sd-wave-rate').textContent = `${formatRate(rate)} MHz`;
    document.querySelectorAll('[data-divider]').forEach(button => button.setAttribute('aria-pressed', String(Number(button.dataset.divider) === divider)));
    const status = rate > 25
      ? `<strong>Above the mode’s limit.</strong> ${formatRate(rate)} MHz exceeds 25 MHz. With divider 1, our physical Pi reported command timeout and CRC errors. This model flags the invalid setting; it does not predict individual corrupted bits.`
      : divider === 4
        ? '<strong>Fastest permitted setting here.</strong> 200 ÷ 8 = 25 MHz. This is the corrected transfer setting that passed on our Pi.'
        : divider === 250
          ? '<strong>Initialization setting.</strong> 200 ÷ 500 = 0.4 MHz, or 400 kHz. The driver uses this limit while initializing the card.'
          : `<strong>Within the 25 MHz limit.</strong> ${formatRate(rate)} MHz uses more time per SD cycle than divider 4. The card mode stays the same.`;
    $('sd-clock-status').innerHTML = status;
    const x0 = 160, width = 720, shownCycles = Math.min(period, 64), spacing = width / shownCycles;
    let sourcePath = `M${x0} 50`;
    for (let i = 0; i < shownCycles; i++) {
      const x = x0 + i * spacing;
      sourcePath += `H${x + spacing / 2}V20H${x + spacing}V50`;
    }
    $('sd-source-wave').setAttribute('d', sourcePath);
    $('sd-output-wave').setAttribute('d', `M${x0} 125H520V95H880V125`);
    const position = sourceCycles % period;
    $('sd-clock-cursor').setAttribute('x1', x0 + width * position / period);
    $('sd-clock-cursor').setAttribute('x2', x0 + width * position / period);
    $('sd-wave-caption').textContent = period <= 64
      ? `One SD cycle spans ${period} source cycles (${period * 5} ns).`
      : `One SD cycle spans ${period} source cycles. Source waveform is compressed.`;
    $('sd-clock-count').textContent = `Source: ${sourceCycles} · SD cycles: ${Math.floor(sourceCycles / period)} · CLK ${position < divider ? 'low' : 'high'}`;
  }
  document.querySelectorAll('[data-divider]').forEach(button => button.addEventListener('click', () => setDivider(Number(button.dataset.divider))));
  $('sd-divider').addEventListener('change', event => setDivider(Number(event.target.value)));
  $('sd-tick').addEventListener('click', () => { sourceCycles++; renderClock(); });
  $('sd-cycle').addEventListener('click', () => { sourceCycles += 2 * divider; renderClock(); });
  $('sd-clock-reset').addEventListener('click', () => { sourceCycles = 0; renderClock(); });
  document.querySelectorAll('[data-clock-answer]').forEach(button => button.addEventListener('click', () => {
    const answer = Number(button.dataset.clockAnswer);
    $('sd-clock-feedback').textContent = answer === 4
      ? 'Correct. Divider 4 gives 200 ÷ (2 × 4) = 25 MHz. A smaller divider exceeds the limit; a larger divider produces a slower clock.'
      : answer === 1
        ? 'Divider 1 requests 100 MHz, above the default-speed limit. Find the smallest divider that produces 25 MHz or less.'
        : 'Divider 128 is within the limit, but it gives 0.78125 MHz. A smaller valid divider can use the available 25 MHz rate.';
    setDivider(answer);
  }));
  $('sd-enlarge').addEventListener('click', () => {
    const zoomed = $('sd-stage').classList.toggle('zoomed');
    $('sd-enlarge').setAttribute('aria-pressed', String(zoomed));
    $('sd-enlarge').textContent = zoomed ? 'Fit drawing to screen' : 'Enlarge drawing';
  });
  renderStep(); renderClock();
})();
