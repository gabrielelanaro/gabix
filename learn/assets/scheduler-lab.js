'use strict';
(() => {
 const lab=document.querySelector('#turn-lab');
 let current=0, counts=[0,0], trace=[], finished=[false,false];
 const say=text=>lab.querySelector('output').textContent=text;
 function draw(){lab.querySelectorAll('.task').forEach((el,i)=>{el.classList.toggle('running',current===i&&!finished[i]);el.querySelector('.meter').textContent=10*(i+1)+counts[i];el.querySelector('.state').textContent=finished[i]?'Finished':current===i?'Running':'Ready';});lab.querySelector('.trace').textContent=trace.join(' → ')||'No output yet';}
 lab.querySelector('[data-action=work]').onclick=()=>{if(finished[current])return;say(`Task ${current?'B':'A'} printed ${10*(current+1)+counts[current]}. It continues running until it yields or returns.`);trace.push(10*(current+1)+counts[current]);counts[current]++;draw();};
 lab.querySelector('[data-action=yield]').onclick=()=>{if(finished.every(Boolean))return;const other=1-current;if(!finished[other])current=other;say(`Dispatcher selected task ${current?'B':'A'}. Recording will use this task’s counter.`);draw();};
 lab.querySelector('[data-action=tick]').onclick=()=>say('The timer interrupt handler ran and returned. This cooperative scheduler keeps the same running task. Preemption is disabled in this simulation; lesson 3.06 explains timer-driven switching.');
 lab.querySelector('[data-action=finish]').onclick=()=>{finished[current]=true;current=1-current;say(finished.every(Boolean)?'Both tasks finished. The dispatcher returns to its caller.':'The finished task will be skipped. The remaining task can continue.');draw();};
 lab.querySelector('[data-action=reset]').onclick=()=>{current=0;counts=[0,0];trace=[];finished=[false,false];say('Reset. Alternate recording and yielding to produce 10 → 20 → 11 → 21 → 12 → 22.');draw();};
 lab.querySelector('[data-action=check]').onclick=()=>say(JSON.stringify(trace)==='[10,20,11,21,12,22]'?'The output matches. Each task recorded three numbers from its own counter. Return from both tasks to finish.':'Compare your trace with 10 → 20 → 11 → 21 → 12 → 22. Record and increment changes the current counter; Yield selects the next ready task. Reset and try again.');draw();
 const save=document.querySelector('#save-lab');let active=0,registers=[10,20],cpu=10;
 const render=()=>{save.querySelector('.meter').textContent=`Current CPU value = ${cpu}; task = ${active?'B':'A'}`;save.querySelector('.trace').textContent=`A saved value: ${registers[0]} · B saved value: ${registers[1]}`;};
 save.querySelector('[data-action=increment]').onclick=()=>{cpu++;render();};
 save.querySelector('[data-action=switch]').onclick=()=>{if(save.querySelector('input').checked)registers[active]=cpu;active=1-active;cpu=registers[active];save.querySelector('output').textContent=save.querySelector('input').checked?'Saved the outgoing value, then restored the incoming value.':'Save was disabled. Switch back: the previous increment has been lost.';render();};
 save.querySelector('[data-action=reset]').onclick=()=>{active=0;registers=[10,20];cpu=10;save.querySelector('output').textContent='Increment A, switch twice, and check whether A kept its value.';render();};render();
})();
