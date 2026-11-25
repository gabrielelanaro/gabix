(function () {
  'use strict';
  const root = document.querySelector('#arm-cpu-lab');
  if (!root) return;
  const get = name => root.querySelector(`[data-view="${name}"]`);
  let index = 0;
  let trace = [];
  const answers = new Map();
  const labels = { r0: 'r0 · working value', r4: 'r4 · preserved value', lr: 'lr · SVC bank', pc: 'PC · next code location', sp: 'SP · selected stack position', mode: 'Processor mode', irq: 'IRQ setting', z: 'Z · equality flag' };
  function printable(value) {
    if (value && typeof value === 'object') return Object.entries(value).map(([k,v]) => `${k}=${v}`).join(' · ');
    return String(value);
  }
  function node(tag, text, className) {
    const el = document.createElement(tag);
    if (text !== undefined) el.textContent = text;
    if (className) el.className = className;
    return el;
  }
  function render() {
    const state = trace[index], prior = trace[Math.max(0, index - 1)];
    get('step').textContent = `Step ${index + 1} of ${trace.length}`;
    get('title').textContent = state.title;
    get('instruction').textContent = state.instruction;
    get('explanation').textContent = state.explanation;
    get('transfer').textContent = state.transfer;
    get('owner').textContent = `Executing: ${state.owner === 'Dispatcher' ? 'dispatcher' : `task ${state.owner} or its interrupt handler`}`;
    get('previous').disabled = index === 0;
    get('next').disabled = index === trace.length - 1;
    get('position').value = index;
    get('position').setAttribute('aria-valuetext', `${index + 1}: ${state.title}`);
    const registers = get('registers'); registers.replaceChildren();
    for (const [key, label] of Object.entries(labels)) {
      const row = node('div', undefined, 'cpu-register');
      const changed = state.cpu[key] !== prior.cpu[key];
      if (changed) row.classList.add('changed');
      row.append(node('dt', label), node('dd', printable(state.cpu[key])));
      if (changed) row.append(node('small', `was ${printable(prior.cpu[key])}`));
      registers.append(row);
    }
    get('banks').replaceChildren();
    for (const [key,label] of [['irqLR','LR_irq'],['irqStatus','SPSR_irq'],['svcStatus','SPSR_svc']]) {
      get('banks').append(node('p', `${label}: ${printable(state.banks[key])}`));
    }
    for (const task of ['A','B']) {
      get(`state-${task}`).textContent = state.tasks[task];
      const stack = get(`stack-${task}`); stack.replaceChildren();
      if (!state.stacks[task].length) stack.append(node('p', 'No interrupt or switch frames here. Ordinary task calls are omitted.', 'empty-stack'));
      [...state.stacks[task]].reverse().forEach(frame => {
        const box = node('div', undefined, 'cpu-frame');
        box.append(node('strong', frame.kind));
        for (const [key,value] of Object.entries(frame.values)) box.append(node('div', `${key}: ${printable(value)}`));
        stack.append(box);
      });
    }
    const source = get('source');
    const paths = {'boot.S':'boot.S','scheduler.rs':'scheduler.rs','context.rs':'context.rs','main.rs':'main.rs'};
    source.hidden = !paths[state.source];
    if (paths[state.source]) { source.href = state.source === 'boot.S' ? '../src/arch/armv7/boot.S' : `../targets/armv7/src/${paths[state.source]}`; source.textContent = `Read ${state.source}`; }
    const quiz = get('quiz'); quiz.replaceChildren(); quiz.hidden = !state.quiz;
    if (state.quiz) {
      quiz.append(node('h3', 'Predict before the next step'), node('p', state.quiz.question));
      const feedback = node('output'); feedback.setAttribute('aria-live', 'polite');
      const choices = node('div', undefined, 'cpu-choices');
      state.quiz.choices.forEach((text, choice) => {
        const button = node('button', text); button.type = 'button';
        button.onclick = () => { answers.set(index, choice); showFeedback(); };
        choices.append(button);
      });
      function showFeedback() {
        if (!answers.has(index)) return;
        const choice = answers.get(index);
        feedback.textContent = `${choice === state.quiz.correct ? 'Correct.' : 'Review this step.'} ${state.quiz.answer}`;
        [...choices.children].forEach((button, i) => button.setAttribute('aria-pressed', String(i === choice)));
      }
      quiz.append(choices, feedback); showFeedback();
    }
  }
  function reset() {
    trace = ArmPreemptionModel.buildTrace({loseR0:get('lose-r0').checked,loseFlags:get('lose-flags').checked});
    index = 0; answers.clear(); get('position').max = trace.length - 1; render();
  }
  get('next').onclick = () => { index = Math.min(trace.length-1,index+1); render(); };
  get('previous').onclick = () => { index = Math.max(0,index-1); render(); };
  get('reset').onclick = reset;
  get('position').oninput = event => { index = Number(event.target.value); render(); };
  get('lose-r0').onchange = reset;
  get('lose-flags').onchange = reset;
  reset();
})();
