/* A deliberately small terminal model. No real device or syscall is invoked. */
(function () {
  function initial(mode = 'canonical') {
    return {mode, edit:'', ready:'', user:'', editor:'', display:'', task:'Running', pending:false, result:null, actor:'terminal', note:'Call read first, or type before the shell calls read.'};
  }
  function step(before, action) {
    const s = {...before};
    const canonical = s.mode === 'canonical';
    function complete() {
      const limit = canonical ? Math.min(64, s.ready.indexOf('\n') + 1) : 64;
      s.user = s.ready.slice(0,limit); s.ready = s.ready.slice(limit);
      s.result = s.user.length; s.pending = false; s.task = 'Running'; s.actor = 'process';
      if (!canonical) {
        for (const ch of s.user) {
          if (ch === '\x7f') s.editor = s.editor.slice(0,-1);
          else if (ch === '\n') { s.note = `Editor submits ${JSON.stringify(s.editor)} to the shell parser.`; s.editor = ''; }
          else s.editor += ch;
        }
        s.display = s.editor;
      }
    }
    if (action === 'read') {
      if (s.pending) { s.note = 'This read is already pending. Deliver input or run the ready reader.'; return s; }
      s.user = ''; s.result = null; s.actor = 'kernel';
      if (s.ready) { s.note = 'Input was already readable: read copies it and returns immediately.'; complete(); }
      else { s.pending = true; s.task = 'Blocked'; s.note = 'Kernel registers the reader and blocks it. Another task can run.'; }
    } else if (action === 'run') {
      if (s.task === 'Ready' && s.pending) { s.note = 'Scheduler resumes the read handler; it rechecks, copies bytes and returns.'; complete(); }
      else s.note = 'No pending read can resume yet. Call read, or deliver input to a waiting reader.';
    } else {
      s.actor = 'kernel';
      const byte = action === 'enter' ? '\n' : action === 'erase' ? '\x7f' : action;
      if (canonical) {
        if (action === 'erase') { if(s.edit) { s.edit = s.edit.slice(0,-1); s.display = s.display.slice(0,-1); } }
        else if (action === 'enter') { s.ready += s.edit + '\n'; s.edit = ''; s.display += '\n'; }
        else { s.edit += byte; s.display += byte; }
        s.note = action === 'enter' ? 'CR is mapped to LF; the line becomes readable. Kernel echo updates the display.' : (action === 'erase' ? 'Backspace removes the last pending character, if present. The terminal shows the edited line.' : 'The kernel stores '+byte+' in its unfinished line and echoes '+byte+' to the terminal.');
      } else {
        s.ready += byte;
        s.note = 'Noncanonical input becomes readable. Kernel echo is off; the editor updates the display after read returns.';
      }
      if (s.ready && s.pending) { s.task = 'Ready'; s.note += ' Wake makes the reader Ready; its user buffer is still unchanged.'; }
    }
    return s;
  }
  if (typeof module !== 'undefined') module.exports = {initial, step};
  if (typeof document === 'undefined') return;
  const $ = id => document.getElementById(id);
  let history = [initial()], cursor = 0, guided = 0;
  const script = ['read','l','x','erase','s','enter','run'];
  const safe = value => String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const bytes = value => value ? JSON.stringify(value) : '∅';
  function render() {
    const s = history[cursor];
    $('state').innerHTML = `<p class="event">${safe(s.note)}</p><dl>${[
      ['Kernel · pending edit',bytes(s.edit)],['Kernel · readable queue',bytes(s.ready)],
      ['Bytes copied to program by this read',bytes(s.user)],
      ['Terminal · visible text',bytes(s.display)],['Reader state',s.task],
      ['Read result',s.result === null ? 'No return yet' : s.result+' bytes']
    ].map(([k,v])=>`<div><dt>${k}</dt><dd>${safe(v)}</dd></div>`).join('')}</dl>`;
    $('back').disabled = cursor === 0;
    $('next').disabled = cursor === history.length-1 && guided === script.length;
    $('next').textContent = cursor < history.length-1 ? 'Forward →' : 'Next →';
    document.querySelectorAll('.actor').forEach(el=>el.classList.toggle('active',el.id === 'actor-'+s.actor));

  }
  function animateTransfer(action, before, after) {
    const dot = document.querySelector?.('.tty-transfer');
    if (!dot || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const input = ['l','s','x','erase','enter'].includes(action);
    const copy = after.result !== null && (action === 'run' && before.task === 'Ready' || action === 'read' && before.ready);
    if (!input && !copy) return;
    const points = input ? [[268,140],[382,140],[382,193],[268,193]] : [[598,140],[712,140]];
    dot.animate(points.map(([x,y]) => ({transform:'translate('+x+'px,'+y+'px)',opacity:1})), {duration:input ? 900 : 450});
  }
  function apply(action) { history = history.slice(0,cursor+1); history.push(step(history[cursor],action)); cursor++; render(); animateTransfer(action,history[cursor-1],history[cursor]); }
  function reset() {history=[initial()];cursor=0;guided=0;render();}
  $('back').onclick=()=>{cursor--;render();};
  $('next').onclick=()=>{if(cursor<history.length-1){cursor++;render();}else if(guided<script.length)apply(script[guided++]);};
  $('reset').onclick=reset;
  document.querySelectorAll('[data-action]').forEach(b=>b.onclick=()=>{guided=script.length;apply(b.dataset.action);});

  render();
})();
