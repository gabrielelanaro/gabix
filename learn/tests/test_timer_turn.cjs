const {test}=require('node:test');const assert=require('node:assert/strict');
const {steps}=require('../assets/timer-turn.js');
test('test_timer_request_precedes_handler_and_task_selection',()=>{
 assert.equal(steps[1].who,'Task A');assert.equal(steps[1].b,'Ready');
 assert.equal(steps[2].who,'Interrupt handler');assert.equal(steps[2].a,'Paused');
 assert.equal(steps[5].who,'Scheduler');assert.equal(steps[5].b,'Selected');
 assert.equal(steps.at(-1).who,'Task B');assert.equal(steps.at(-1).a,'Ready');
});
test('test_disabled_timer_keeps_a_running_and_restart_resets_progress',()=>{
 const vm=require('node:vm'),fs=require('node:fs'),parts={};
 const buttons=[{dataset:{answer:'timer'}},{dataset:{answer:'scheduler'}}];
 const lab={dataset:{},querySelector(s){return parts[s.match(/data-part="([^"]+)"/)[1]]||=( {} );},querySelectorAll(){return buttons;}};
 vm.runInNewContext(fs.readFileSync(require.resolve('../assets/timer-turn.js'),'utf8'),{document:{querySelector(){return lab;}}});
 parts.enabled.onchange({target:{checked:false}});
 parts.next.onclick();parts.next.onclick();
 assert.equal(parts.count.textContent,12);assert.equal(parts.who.textContent,'Task A');assert.equal(parts.b.textContent,'Ready');
 parts.enabled.onchange({target:{checked:true}});
 for(let i=0;i<7;i++)parts.next.onclick();
 assert.equal(parts.who.textContent,'Task B');assert.equal(parts.next.disabled,true);
 parts.previous.onclick();assert.equal(parts.who.textContent,'Context-switch code');
 parts.reset.onclick();assert.equal(parts.count.textContent,10);assert.equal(parts.who.textContent,'Task A');
 buttons[1].onclick();assert.match(parts.feedback.textContent,/Yes/);
});
