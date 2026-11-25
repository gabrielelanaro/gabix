// The same pixel-center test that the Mac uses to check the physical readback.
(() => {
  const blue = '#194296';
  const gold = '#ffcd28';

  function inside(x, y) {
    return y >= 8 && y < 56 && 2 * Math.abs(2 * x + 1 - 64) < 2 * y + 1 - 16;
  }

  function drawFrame(canvas, foreground = gold) {
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = blue;
    ctx.fillRect(0, 0, 64, 64);
    if (canvas.dataset.frame !== 'clear') {
      ctx.fillStyle = foreground;
      for (let y = 8; y < 56; y++) {
        for (let x = 8; x < 56; x++) {
          if (inside(x, y)) ctx.fillRect(x, y, 1, 1);
        }
      }
    }
    if (canvas.dataset.mark === 'yes') {
      ctx.strokeStyle = '#ffffff';
      ctx.lineWidth = 1;
      ctx.strokeRect(17.5, 30.5, 5, 4);
    }
  }

  function drawZoom(canvas) {
    const ctx = canvas.getContext('2d');
    const x0 = Number(canvas.dataset.x);
    const y0 = Number(canvas.dataset.y);
    const cell = 64;
    const cols = canvas.width / cell;
    const rows = canvas.height / cell;
    for (let r = 0; r < rows; r++) {
      for (let c = 0; c < cols; c++) {
        const covered = inside(x0 + c, y0 + r);
        ctx.fillStyle = covered && canvas.dataset.fill === 'yes' ? gold : blue;
        ctx.fillRect(c * cell, r * cell, cell, cell);
        ctx.strokeStyle = '#a4b9d9';
        ctx.strokeRect(c * cell + .5, r * cell + .5, cell - 1, cell - 1);
        ctx.beginPath();
        ctx.arc((c + .5) * cell, (r + .5) * cell, 3.5, 0, 2 * Math.PI);
        ctx.fillStyle = covered && canvas.dataset.fill === 'yes' ? '#222222' : '#ffffff';
        ctx.fill();
      }
    }
    const edge = y => 32 - (y - 8) / 2;
    ctx.strokeStyle = '#ffffff';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo((edge(y0) - x0) * cell, 0);
    ctx.lineTo((edge(y0 + rows) - x0) * cell, rows * cell);
    ctx.stroke();
  }

  function colorBytes(hex) {
    return [hex.slice(5, 7), hex.slice(3, 5), hex.slice(1, 3), 'ff'];
  }

  document.querySelectorAll('canvas[data-frame]').forEach(canvas => drawFrame(canvas));
  document.querySelectorAll('canvas[data-zoom]').forEach(drawZoom);

  for (const lab of document.querySelectorAll('[data-color-lab]')) {
    const input = lab.querySelector('input');
    const output = lab.querySelector('output');
    function edit() {
      const hex = input.value;
      const bytes = colorBytes(hex).join(' ');
      output.textContent = 'r3 = 0xff' + hex.slice(1) + ' · Pixel (20, 32) = ' + bytes +
        ' · Pixel (19, 32) = 96 42 19 ff';
      drawFrame(lab.closest('figure').querySelector('canvas[data-frame="edit"]'), hex);
    }
    input.addEventListener('input', edit);
    edit();
  }

  for (const lab of document.querySelectorAll('[data-offset-lab]')) {
    const xField = lab.querySelector('[name="pixel-x"]');
    const yField = lab.querySelector('[name="pixel-y"]');
    const fileField = lab.querySelector('[type="file"]');
    const output = lab.querySelector('output');
    let frame = null;
    function inspect() {
      const x = xField.valueAsNumber;
      const y = yField.valueAsNumber;
      if (![x, y].every(value => Number.isInteger(value) && value >= 0 && value < 64)) {
        output.textContent = 'Enter whole coordinates from 0 to 63.';
        return;
      }
      const offset = 4 * (64 * y + x);
      const chunk = Math.floor(offset / 512) * 512;
      const bytes = frame
        ? Array.from(frame.subarray(offset, offset + 4), byte => byte.toString(16).padStart(2, '0'))
        : colorBytes(inside(x, y) ? gold : blue);
      output.textContent = (frame ? 'Local file' : 'Triangle model') + ' · Offset ' + offset +
        ' · Chunk ' + chunk + ' + ' + (offset - chunk) + ' · ' + bytes.join(' ');
    }
    xField.addEventListener('input', inspect);
    yField.addEventListener('input', inspect);
    fileField.addEventListener('change', async () => {
      const file = fileField.files[0];
      frame = null;
      if (file && file.size !== 16384) {
        output.textContent = 'Choose a 16,384-byte frame from the Pi test.';
        fileField.value = '';
        return;
      }
      if (file) {
        try {
          const bytes = new Uint8Array(await file.arrayBuffer());
          if (fileField.files[0] !== file) return;
          frame = bytes;
        } catch {
          output.textContent = 'The browser could not read that file.';
          return;
        }
      }
      inspect();
    });
    inspect();
  }
})();
