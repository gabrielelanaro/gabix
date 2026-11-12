const { test } = require('node:test');
const assert = require('node:assert/strict');
const { buildTrace } = require('../assets/arm-preemption-model.js');

test('test_resume_restores_interrupted_values_and_branch', () => {
  const trace = buildTrace();
  const last = trace.at(-1);
  assert.equal(last.cpu.r0, 10);
  assert.equal(last.cpu.r4, 40);
  assert.equal(last.cpu.z, 1);
  assert.equal(last.cpu.pc, 'A_equal');
  assert.equal(last.cpu.irq, 'enabled');
  assert.equal(last.cpu.mode, 'SVC');
  assert.deepEqual(last.stacks.A, []);
  assert.equal(last.tasks.B, 'Ready');
  assert.ok(last.stacks.B.some(frame => frame.kind === 'Exception return'));
});
test('test_irq_entry_preserves_values_before_software_saves_them', () => {
  const trace = buildTrace();
  assert.equal(trace[1].cpu.mode, 'IRQ');
  assert.equal(trace[1].cpu.r0, 10);
  assert.equal(trace[1].cpu.irq, 'masked');
  assert.equal(trace[1].banks.irqStatus.z, 1);
  assert.deepEqual(trace[1].stacks.A, []);
  const saved = trace.find(s => s.title === 'Save A’s general-purpose registers');
  assert.equal(saved.stacks.A.find(f => f.kind === 'Interrupted registers').values.r0, 10);
});
test('test_running_b_keeps_a_saved_state_unchanged', () => {
  const trace = buildTrace();
  const before = trace.find(s => s.title === 'Pause A’s timer handler');
  const during = trace.find(s => s.title === 'The next timer interrupts B');
  assert.deepEqual(during.stacks.A, before.stacks.A);
  assert.equal(during.stacks.B.find(f => f.kind === 'Interrupted registers').values.r0, 99);
  assert.equal(during.stacks.A.find(f => f.kind === 'Interrupted registers').values.r0, 10);
});
test('test_restore_faults_change_only_the_intended_outcome', () => {
  const badRegister = buildTrace({ loseR0: true }).at(-1);
  assert.equal(badRegister.cpu.r0, 99);
  assert.equal(badRegister.cpu.pc, 'A_equal');
  const badFlags = buildTrace({ loseFlags: true }).at(-1);
  assert.equal(badFlags.cpu.r0, 10);
  assert.equal(badFlags.cpu.pc, 'A_not_equal');
  const both = buildTrace({ loseR0: true, loseFlags: true }).at(-1);
  assert.equal(both.cpu.r0, 99);
  assert.equal(both.cpu.pc, 'A_not_equal');
});
test('test_reset_builds_independent_snapshots', () => {
  const trace = buildTrace();
  trace.at(-1).cpu.r0 = -1;
  assert.equal(trace[0].cpu.r0, 10);
  assert.equal(buildTrace().at(-1).cpu.r0, 10);
  assert.equal(trace.filter(s => s.quiz).length, 3);
});

// Exercise the actual view's controls with a minimal DOM surface. This checks
// interaction wiring; browser layout and accessibility need separate inspection.
test('test_controls_step_back_reset_and_answer_predictions', () => {
  const vm = require('node:vm');
  const fs = require('node:fs');
  class Element {
    constructor() { this.children=[]; this.attributes={}; this.checked=false; this.classList={add(){}}; }
    append(...children) { this.children.push(...children); }
    replaceChildren(...children) { this.children=children; }
    setAttribute(key,value) { this.attributes[key]=value; }
  }
  const views={};
  const root={querySelector(selector){const name=selector.match(/data-view="([^"]+)"/)[1];return views[name] ||= new Element();}};
  const document={querySelector(){return root;},createElement(){return new Element();}};
  vm.runInNewContext(fs.readFileSync(require.resolve('../assets/arm-preemption-view.js'),'utf8'), {document,ArmPreemptionModel:{buildTrace}});
  assert.equal(views.previous.disabled,true);
  assert.match(views.title.textContent,/A is about to branch/);
  const choices=views.quiz.children[2];
  choices.children[2].onclick();
  assert.match(views.quiz.children[3].textContent,/Correct/);
  views.next.onclick();
  assert.match(views.title.textContent,/Hardware enters IRQ/);
  views.previous.onclick();
  assert.match(views.quiz.children[3].textContent,/Correct/);
  views['lose-flags'].checked=true;views['lose-flags'].onchange();
  views.position.oninput({target:{value:views.position.max}});
  assert.match(views.explanation.textContent,/does not take/);
  assert.equal(views.next.disabled,true);
  views.reset.onclick();
  assert.equal(views.previous.disabled,true);
});
