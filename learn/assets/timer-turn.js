(function(root){
'use strict';
const steps = [
 {title:'A is running. B is waiting.',who:'Task A',timer:'Counting time',a:'Running',b:'Ready',saved:'A’s values are still in the CPU',flow:'',text:'A keeps counting in a loop. B is ready, which means the kernel could run it. On this one core, B waits while A runs. A never calls yield_now().',source:'scheduler.rs'},
 {title:'The timer requests an interrupt.',who:'Task A',timer:'Interrupt requested',a:'Running',b:'Ready',saved:'A’s values are still in the CPU',flow:'request',text:'The hardware timer reaches its deadline and signals the CPU. This signal is an interrupt request. It asks the CPU to enter an interrupt handler. The timer has no list of tasks and has not selected B.',source:'raspi2b/timer.rs'},
 {title:'The CPU starts the kernel’s interrupt handler.',who:'Interrupt handler',timer:'Request being handled',a:'Paused',b:'Ready',saved:'A’s return address is recorded by the CPU',flow:'request',text:'With interrupts enabled, the CPU pauses A and jumps to an address the kernel configured during boot. That address begins the interrupt handler. The handler is kernel code executing on the same CPU; B still has not run.',source:'boot.S'},
 {title:'The handler saves A’s state.',who:'Interrupt handler',timer:'Request being handled',a:'Paused',b:'Ready',saved:'A’s values and resume location are saved in memory',flow:'save',text:'The assembly at the start of the handler saves the register values needed to resume A. They go into A’s stack, a region of memory reserved for its function calls and saved state. This lets other code use the CPU registers.',source:'boot.S'},
 {title:'The handler acknowledges the timer.',who:'Interrupt handler',timer:'Counting toward next interrupt',a:'Paused',b:'Ready',saved:'A’s state remains in memory',flow:'',text:'The handler tells the timer that its interrupt has been handled and prepares the next tick. It then calls the scheduling code. A handler could simply return to A here; our preemptive handler asks the scheduler to choose a task.',source:'main.rs'},
 {title:'The scheduler selects B.',who:'Scheduler',timer:'Counting toward next interrupt',a:'Ready',b:'Selected',saved:'A’s state remains in memory',flow:'',text:'The scheduler is kernel code that chooses from ready tasks. It marks A ready to run again, then selects B as the next ready task. This is where the choice of task happens.',source:'scheduler.rs'},
 {title:'The kernel restores B’s context.',who:'Context-switch code',timer:'Counting toward next interrupt',a:'Ready',b:'Selected',saved:'B’s prepared starting state is loaded into the CPU',flow:'load',text:'The context-switch code selects B’s stack and loads its saved context. In this example B is starting for the first time, so the kernel prepared a starting context when it created B. A’s saved state stays in A’s memory.',source:'context.rs'},
 {title:'B starts running.',who:'Task B',timer:'Counting toward next interrupt',a:'Ready',b:'Running',saved:'A’s saved state is kept for its next turn',flow:'',text:'The CPU executes B’s function. A never asked to stop: the timer interrupt gave the kernel an opportunity to switch tasks. A later timer interrupt can let the scheduler select A again.',source:'scheduler.rs'}
];
if(typeof module!=='undefined'&&module.exports){module.exports={steps};return;}
const lab=document.querySelector('#timer-turn');if(!lab)return;
const get=id=>lab.querySelector(`[data-part="${id}"]`);let position=0,count=10,enabled=true;
function draw(){
 const s=steps[position];
 for(const key of ['title','who','timer','a','b','saved','text'])get(key).textContent=s[key];
 get('count').textContent=count;
 get('progress').textContent=`${position+1} / ${steps.length}`;
 get('previous').disabled=position===0;
 get('next').disabled=position===steps.length-1;
 get('next').textContent=position===0?'Let time pass':position===steps.length-1?'B has its turn':'Next step';
 get('next').disabled=enabled&&position===steps.length-1;
 lab.dataset.flow=s.flow;
 get('request').textContent=s.flow==='request'?'interrupt request →':'no request';
 get('memory-flow').textContent=s.flow==='save'?'save →':s.flow==='load'?'← load':'saved in memory';
 get('source').href=`../../targets/armv7/src/${s.source}`;
 get('source').textContent=`Code: ${s.source}`;
 get('feedback').textContent='';
 get('question').hidden=position!==5;
 get('counter-label').textContent=position===7?'A’s saved counter':'A’s counter';
 if(!enabled){get('text').textContent=`Timer interrupts are disabled in this example. A continues counting (${count}). B stays ready. A never yields, so this scheduler gets no opportunity to switch to B.`;get('timer').textContent='Interrupts disabled';get('next').textContent='Let A keep counting';}
}
get('next').onclick=()=>{if(enabled)position=Math.min(steps.length-1,position+1);else count++;draw();};
get('previous').onclick=()=>{position=Math.max(0,position-1);draw();};
get('reset').onclick=()=>{position=0;count=10;draw();};
get('enabled').onchange=e=>{enabled=e.target.checked;position=0;count=10;draw();};
lab.querySelectorAll('[data-answer]').forEach(button=>button.onclick=()=>{get('feedback').textContent=button.dataset.answer==='scheduler'?'Yes. The timer requests attention; the kernel scheduler chooses B from its ready tasks.':'The timer requested the interrupt, and the CPU entered the handler. The kernel scheduler makes the choice between A and B.';});
draw();
})(globalThis);
