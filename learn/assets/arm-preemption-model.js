/* A deterministic teaching model of gabix's IRQ wrapper and scheduler.
 * Symbolic addresses and grouped Rust calls are intentional. This is not an
 * instruction-set emulator. The saved values drive the later restore steps.
 */
(function (root) {
  'use strict';
  function buildTrace(options = {}) {
    const { loseR0 = false, loseFlags = false } = options;
    const cpu = { r0: 10, r4: 40, lr: 'A caller', pc: 'A: branch', mode: 'SVC', irq: 'enabled', z: 1, sp: 'A: ordinary calls' };
    const stacks = { A: [], B: [], Dispatcher: [] };
    const tasks = { A: 'Running', B: 'Ready' };
    const banks = { irqLR: 'unused', irqStatus: 'unused', svcStatus: 'prior SVC status' };
    const trace = [];
    let owner = 'A';
    const copy = value => JSON.parse(JSON.stringify(value));
    function emit(title, instruction, explanation, transfer, source = 'boot.S', quiz = null) {
      trace.push(copy({ title, instruction, explanation, transfer, source, quiz, cpu, banks, stacks, tasks, owner }));
    }
    function save(task, kind, values) { stacks[task].push({ kind, values: copy(values) }); }
    function pop(task, kind) {
      const frame = stacks[task].pop();
      if (frame.kind !== kind) throw new Error(`Expected ${kind}, found ${frame.kind}`);
      return frame.values;
    }
    function enterIRQ(task, resumePC) {
      banks.irqLR = `${resumePC} + 4`;
      banks.irqStatus = { mode: cpu.mode, irq: cpu.irq, z: cpu.z };
      cpu.mode = 'IRQ'; cpu.irq = 'masked'; cpu.pc = 'IRQ vector'; cpu.sp = 'shared IRQ stack (unused)';
    }
    function saveReturn(task, resumePC) {
      banks.irqLR = resumePC;
      save(task, 'Exception return', { pc: resumePC, status: banks.irqStatus });
    }
    function saveRegisters(task) {
      save(task, 'Interrupted registers', { r0: cpu.r0, r4: cpu.r4, lr: cpu.lr, others: 'r1–r3, r5–r12 also saved' });
      save(task, 'Wrapper bookkeeping', { svcStatus: banks.svcStatus, r1: 'copied again; keeps this save eight bytes wide' });
      cpu.r4 = `${task}: IRQ frame address`;
    }
    function switchOut(task) {
      save(task, 'Suspended handler calls', { note: 'Compiler-generated Rust call frames; grouped here' });
      save(task, 'Switch context', { r4: cpu.r4, lr: 'resume yield_now', others: 'r5–r11 also saved' });
      tasks[task] = 'Ready'; owner = 'Dispatcher';
      cpu.sp = 'Dispatcher: saved calls'; cpu.pc = 'run_until_finished'; cpu.r4 = 'dispatcher local'; cpu.lr = 'dispatcher caller';
      cpu.r0 = `ptr(${task}.context)`;
    }

    save('B', 'Initial switch context', { r4: 0, lr: 'task_entry', others: 'r5–r11 initialized to zero' });
    emit('A is about to branch', 'cmp r0, #10  →  next: beq A_equal',
      'A compared r0 (10) with 10. The comparison set Z to 1, meaning equal. The next instruction will branch if Z is still 1. A is a kernel task running in SVC mode.',
      'No stack copy yet. The values are in CPU registers.', 'example',
      { question: 'When the timer arrives, where are A’s general-purpose registers saved automatically?', choices: ['A’s private stack', 'Shared IRQ stack', 'No stack memory'], correct: 2,
        answer: 'Hardware saves the return address and status in banked IRQ registers. Our assembly must copy the general-purpose registers to memory.' });
    enterIRQ('A', 'A: branch');
    emit('Hardware enters IRQ mode', 'IRQ entry: LR_irq ← return address; SPSR_irq ← CPSR',
      'Hardware selects IRQ mode and masks IRQs. It records the return address and old processor status in banked IRQ registers. r0 and r4 still contain A’s values. The IRQ stack is selected, but no stack words have been written.',
      'CPU status → SPSR_irq; return address → LR_irq', 'hardware');
    banks.irqLR = 'A: branch'; cpu.pc = '.Lirq: srsdb';
    emit('Correct the return address', 'sub lr, lr, #4',
      'In IRQ mode, lr names LR_irq. Subtracting four gives the address where A must resume. A’s SVC link register is a separate banked register and remains unchanged.',
      'LR_irq ← A: branch');
    saveReturn('A', 'A: branch'); cpu.pc = '.Lirq: cps';
    emit('Copy the return state to A’s stack', 'srsdb sp!, #0x13',
      'SRS stores the corrected IRQ return address and saved status on the SVC stack. Mode 0x13 means SVC. That stack belongs to A. The active CPU mode is still IRQ.',
      'LR_irq + SPSR_irq → A / Exception return');
    cpu.mode = 'SVC'; cpu.sp = 'A: Exception return'; cpu.pc = '.Lirq: stmdb';
    emit('Select the kernel stack', 'cps #0x13',
      'Changing to SVC mode selects the SVC stack pointer and link register. The CPU now uses A’s kernel stack. IRQs remain masked.',
      'Active SP: IRQ bank → A’s SVC stack');
    save('A', 'Interrupted registers', { r0: cpu.r0, r4: cpu.r4, lr: cpu.lr, others: 'r1–r3, r5–r12 also saved' });
    cpu.sp = 'A: Interrupted registers'; cpu.pc = '.Lirq: mrs';
    emit('Save A’s general-purpose registers', 'stmdb sp!, {r0-r12, lr}',
      'The wrapper copies r0 through r12 and A’s SVC link register to A’s stack. Registers are copied; their CPU values have not been erased. Later code can use the CPU registers because this saved copy exists.',
      'CPU r0=10, r4=40, SVC lr → A / Interrupted registers');
    save('A', 'Wrapper bookkeeping', { svcStatus: banks.svcStatus, r1: 'copied again; keeps this save eight bytes wide' });
    cpu.r0 = banks.svcStatus; cpu.r4 = 'A: IRQ frame address'; cpu.sp = 'A: aligned Rust call stack'; cpu.pc = 'arm_irq_entry'; cpu.lr = '.Lirq after bl';
    emit('Prepare to call Rust', 'mrs r0, spsr; push {r0,r1}; mov r4,sp; bic sp,sp,#7; bl arm_irq_entry',
      'The wrapper preserves SVC’s saved-status register, then records the frame address in r4. It aligns SP to eight bytes before calling Rust. Several instructions are grouped in this step. A’s original r0 and r4 remain in the saved register frame.',
      'SVC status → A / Wrapper bookkeeping; r4 ← frame address');
    cpu.r0 = 'handler temporary'; cpu.z = 0; cpu.pc = 'preempt_on_timer'; cpu.lr = 'handler call return (grouped)';
    emit('Acknowledge the timer', 'timer::handle_interrupt(); scheduler::preempt_on_timer();',
      'The board-specific timer handler acknowledges and rearms the timer. The handler can change registers and flags; this model chooses Z=0 to make restoration visible. It then asks the scheduler to preempt A.',
      'CPU registers are available to handler code; A’s saved values stay in memory.', 'main.rs',
      { question: 'While A is paused, where must its interrupt frame remain?', choices: ['On A’s task stack', 'On B’s task stack', 'On the IRQ stack'], correct: 0,
        answer: 'A’s own stack keeps its frame while B runs. Each task needs its own saved frame because B can also receive timer interrupts.' });
    switchOut('A');
    emit('Pause A’s timer handler', 'yield_now() → switch(A.context, DISPATCHER)',
      'yield_now marks A Ready. switch saves the handler’s call-preserved registers and stack pointer, then restores the dispatcher. A is paused inside its timer handler. Its stack holds both the interrupted task state and the handler’s suspended calls.',
      'A’s stack position → A.context.sp; active SP → dispatcher stack', 'scheduler.rs');
    pop('B', 'Initial switch context');
    owner = 'B'; tasks.B = 'Running'; cpu.sp = 'B: initial stack'; cpu.pc = 'task_entry'; cpu.r4 = 0; cpu.lr = 'task_entry';
    emit('The dispatcher selects B', 'switch(DISPATCHER, B.context) → task_entry',
      'The dispatcher selects the next Ready task. B is new, so spawn has prepared a context whose return address starts task_entry. Restoring B’s stack starts that wrapper. A’s frames remain on A’s stack.',
      'B.context.sp → active SP; prepared return address → task_entry', 'scheduler.rs');
    cpu.irq = 'enabled'; cpu.sp = 'B: ordinary calls'; cpu.pc = 'B: calculation'; cpu.lr = 'B caller'; cpu.r0 = 99; cpu.r4 = 44; cpu.z = 0;
    emit('B runs using the same CPU registers', 'task_entry → restore_irqs(true) → B’s function',
      'B enables IRQs and runs its function. In this example it sets r0=99 and r4=44. There is one active set of these CPU registers. A’s 10 and 40 are stored in memory on A’s stack.',
      'CPU r0 ← 99; CPU r4 ← 44; A’s saved copies remain 10 and 40.', 'example',
      { question: 'When A resumes correctly, what value should r0 contain?', choices: ['Value 10', 'Value 40', 'Value 99'], correct: 0,
        answer: 'r0 must return to 10. B’s 99 belongs to B’s execution, so A’s saved register frame must replace it.' });
    enterIRQ('B', 'B: next instruction'); saveReturn('B', 'B: next instruction');
    cpu.mode = 'SVC'; saveRegisters('B'); cpu.sp = 'B: aligned Rust call stack'; cpu.pc = 'arm_irq_entry'; cpu.lr = '.Lirq after bl';
    emit('The next timer interrupts B', 'IRQ entry → SRS → save registers → Rust handler',
      'B repeats the same entry sequence. Its return state and registers go on B’s stack. The banked IRQ registers now describe B, so A must rely on its own saved memory copy. These repeated instructions are grouped in this step.',
      'B’s CPU state → B’s stack; A’s stack is unchanged.');
    cpu.r0 = 'handler temporary'; cpu.z = 0; switchOut('B');
    emit('Pause B and run the dispatcher', 'acknowledge timer → yield_now → switch',
      'After acknowledging B’s timer interrupt, the handler yields. B becomes Ready. The dispatcher sees A is Ready and can select it again.',
      'B’s handler context → B’s stack; active SP → dispatcher stack', 'scheduler.rs');
    owner = 'A'; tasks.A = 'Running';
    const context = pop('A', 'Switch context');
    cpu.r4 = context.r4; cpu.lr = context.lr; cpu.r0 = 'unspecified after call'; cpu.sp = 'A: suspended handler calls'; cpu.pc = 'yield_now after switch';
    emit('Resume A inside yield_now', 'switch: ldr sp,[r1]; pop {r4-r11,lr}; bx lr',
      'The context switch restores A’s handler context. r4 is the IRQ frame address saved by the wrapper. A’s interrupted r4=40 is still deeper on the stack. The switch’s return address resumes yield_now, which will return through the timer handler.',
      'A.context.sp → active SP; A’s saved handler registers → CPU', 'context.rs');
    pop('A', 'Suspended handler calls'); cpu.pc = '.Lirq after bl'; cpu.sp = 'A: Wrapper bookkeeping';
    emit('Return through the handler’s calls', 'yield_now returns → preempt_on_timer returns → arm_irq_entry returns',
      'These Rust function returns remove their call frames. IRQs remain masked because A was paused inside an IRQ handler. Control reaches the assembly wrapper after its call to arm_irq_entry.',
      'Handler call frames removed; A’s interrupted state is still saved.', 'scheduler.rs');
    const book = pop('A', 'Wrapper bookkeeping'); banks.svcStatus = book.svcStatus; cpu.r0 = book.svcStatus; cpu.pc = '.Lirq: ldmia'; cpu.sp = 'A: Interrupted registers';
    emit('Restore wrapper bookkeeping', 'mov sp,r4; pop {r0,r1}; msr spsr_cxsf,r0',
      'The frame address in r4 restores the stack position before alignment. The wrapper restores SVC’s saved status. This is separate from A’s interrupted CPU status, which remains in the exception-return frame.',
      'A / Wrapper bookkeeping → SVC saved status');
    const registers = pop('A', 'Interrupted registers');
    cpu.r0 = loseR0 ? 99 : registers.r0; cpu.r4 = registers.r4; cpu.lr = registers.lr; cpu.pc = '.Lirq: rfeia'; cpu.sp = 'A: Exception return';
    emit(loseR0 ? 'Fault: A’s r0 was not preserved' : 'Restore A’s original registers',
      loseR0 ? 'Fault injection: restore r0 from the wrong value' : 'ldmia sp!, {r0-r12, lr}',
      loseR0 ? 'This deliberate model fault substitutes B’s value 99 for A’s saved r0. The real implementation restores r0 with the other registers.' : 'The wrapper loads A’s original registers from its saved frame. r0 is 10 again and r4 is 40. The processor is still in the wrapper with IRQs masked.',
      'A / Interrupted registers → CPU r0, r4, and the other saved registers');
    const returned = pop('A', 'Exception return');
    cpu.pc = returned.pc; cpu.mode = returned.status.mode; cpu.irq = returned.status.irq; cpu.z = loseFlags ? 0 : returned.status.z; cpu.sp = 'A: ordinary calls';
    emit(loseFlags ? 'Fault: condition flags were not restored' : 'Return to the interrupted instruction',
      loseFlags ? 'Fault injection: keep the handler’s Z=0' : 'rfeia sp!',
      loseFlags ? 'The model restores the address and IRQ setting but deliberately leaves Z=0. A’s following conditional branch will now make the wrong decision.' : 'RFE loads the saved program address and processor status. A resumes at its branch with Z=1 and IRQs enabled. The entire interrupted state shown in this model has been restored.',
      'A / Exception return → PC + processor status');
    cpu.pc = cpu.z === 1 ? 'A_equal' : 'A_not_equal';
    emit('A executes its conditional branch', 'beq A_equal',
      `The branch reads Z=${cpu.z}, so it ${cpu.z === 1 ? 'takes' : 'does not take'} the equal branch. r0 is ${cpu.r0}. ${loseR0 || loseFlags ? 'Compare this result with the intended r0=10 and equal branch. Reset with both faults disabled to compare.' : 'A continues with r0=10, r4=40, and the same branch decision as before the interrupt. B remains Ready with its own suspended handler.'}`,
      'Restored flags determine the next instruction.', 'example');
    return trace;
  }
  const api = { buildTrace };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.ArmPreemptionModel = api;
})(typeof globalThis !== 'undefined' ? globalThis : this);
