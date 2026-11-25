(() => {
  const figure = document.querySelector('.teletype');
  const notes = [
    'The keyboard sends a code to the host. In this example the host echoes it to the printer, then sends carriage return and line feed.',
    '1 of 5 · Press lowercase l. The keyboard transmits the character code to the host.',
    '2 of 5 · The host echoes l by transmitting the same code back along the serial connection.',
    '3 of 5 · The printer marks l on the paper and advances the carriage one column.',
    '4 of 5 · The host sends carriage return (CR). The carriage moves back to the left margin on the same line.',
    '5 of 5 · The host sends line feed (LF). The roller advances the paper one line. The carriage keeps its column.'
  ];
  const back = figure.querySelector('.tt-prev');
  const next = figure.querySelector('.tt-next');
  const play = figure.querySelector('.tt-play');
  let index = 0;
  let timer;
  function stop() {
    clearInterval(timer);
    timer = undefined;
    play.textContent = 'Play';
  }
  function show(step) {
    index = step;
    figure.dataset.step = step;
    figure.querySelector('.tt-note').textContent = notes[step];
    back.disabled = step === 0;
    next.disabled = step === notes.length - 1;
    if (next.disabled) stop();
  }
  back.onclick = () => { stop(); show(index - 1); };
  next.onclick = () => { stop(); show(index + 1); };
  play.onclick = () => {
    if (timer) return stop();
    if (index === notes.length - 1) show(0);
    timer = setInterval(() => show(index + 1), 2600);
    play.textContent = 'Pause';
    show(index + 1);
  };
  show(0);
})();
