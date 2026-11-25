// Each row follows one input through three owners. The marker locates the current operation.
const ttySequences = {
  route: [
    [['Serial terminal','UART','Serial TTY'], [
      [0,'The terminal transmits the bits for lowercase l over the cable.'],
      [1,'The computer’s UART assembles the byte. The serial driver reads it.'],
      [2,'The driver delivers l to the line discipline. The shell’s descriptor 0 refers to this serial TTY.']]],
    [['Emulator','PTY master','Slave TTY'], [
      [0,'The emulator receives the key event for lowercase l.'],
      [1,'It writes the byte to the master endpoint. The kernel delivers it to the paired slave.'],
      [2,'The slave’s line discipline accepts l. The shell reads this slave through descriptor 0. Echo travels back to the emulator through the master.']]]
  ],
  editor: [
    [['Terminal','Kernel edit','Shell buffer'], [
      [0,'Type lx while the shell waits in read.', ['', '', '']],
      [1,'The kernel stores the characters and echoes them to the display.', ['lx','lx','']],
      [1,'Backspace removes x from the kernel edit buffer and updates the display.', ['l','l','']],
      [1,'Typing s appends it to the pending line.', ['ls','ls','']],
      [1,'Enter commits the line. The shell is Ready and its read is still pending.', ['ls ↵','queued: ls ↵','']],
      [2,'The scheduler runs the shell. Its read returns three bytes.', ['ls ↵','','ls ↵']]]],
    [['Terminal','Kernel queue','Shell editor'], [
      [0,'Type lx with kernel echo disabled.', ['','','']],
      [1,'The bytes are available before Enter.', ['','lx','']],
      [2,'The editor reads lx into its own buffer and writes lx to the display.', ['lx','','lx']],
      [2,'The editor reads Backspace, removes x from its buffer and redraws the line.', ['l','','l']],
      [2,'The editor reads s, appends it and redraws the command.', ['ls','','ls']],
      [2,'The editor reads Enter and passes ls to the parser within the shell process.', ['ls ↵','','parser: ls']]]]
  ]
};
for (const figure of document.querySelectorAll('[data-sequence]')) {
  const select = figure.querySelector('select');
  const back = figure.querySelector('[data-prev]');
  const next = figure.querySelector('[data-next]');
  let index = 0;
  function draw() {
    const [labels, steps] = ttySequences[figure.dataset.sequence][select.selectedIndex];
    const [station, caption, contents] = steps[index];
    figure.querySelectorAll('[data-station]').forEach((part, i) => {
      part.querySelector('text').textContent = labels[i];
      if (contents) {
        let value = part.querySelector('.buffer-value');
        if (!value) {
          value = document.createElementNS('http://www.w3.org/2000/svg','text');
          value.setAttribute('class','buffer-value');
          value.setAttribute('x','65');
          value.setAttribute('y','111');
          value.setAttribute('text-anchor','middle');
          part.append(value);
        }
        value.textContent = contents[i] || '∅';
      }
      part.classList.toggle('active', i === station);
    });
    figure.querySelector('.location').style.transform = 'translateX(' + station * 157 + 'px)';
    figure.querySelector('figcaption').textContent = (index + 1) + ' of ' + steps.length + ' · ' + caption;
    back.disabled = index === 0;
    next.disabled = index === steps.length - 1;
  }
  select.onchange = () => { index = 0; draw(); };
  back.onclick = () => { index--; draw(); };
  next.onclick = () => { index++; draw(); };
  draw();
}
