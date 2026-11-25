/* ============================================================
   lease-office.js — "The Lease Office" DHCP server minigame
   The player runs the DHCP server at 10.0.2.2. Clients arrive
   with DISCOVER or REQUEST messages; the player answers with
   OFFER, ACK, or NAK. Rules follow RFC 2131 §3.1:
     - DISCOVER  → OFFER a free pool address (hold it briefly)
     - REQUEST for the address you offered that client → ACK
     - REQUEST for anything else                    → NAK
     - pool empty + DISCOVER → silence is the legal answer
   Leases and holds expire, freeing addresses back to the pool.
   ============================================================ */

(function () {
  'use strict';

  var root = document.querySelector('[data-lease-office]');
  if (!root) return;

  /* ---- tunables (seconds) ---- */
  var PATIENCE = 30;        /* client gives up after this          */
  var HOLD = 12;            /* an OFFER reserves an address this long */
  var LEASE = 40;           /* a signed lease lasts this long      */
  var REQUEST_DELAY = 2.5;  /* well-behaved client requests quickly */
  var TICK = 0.1;

  var POOL_IPS = ['10.0.2.15', '10.0.2.16', '10.0.2.17'];

  /* The day's arrivals. kind 'rogue' skips DISCOVER and demands
     an address it was never offered. */
  var SCRIPT = [
    { at: 1,  name: 'ada',  mac: '52:54:00:00:00:a1', kind: 'normal' },
    { at: 7,  name: 'bram', mac: '52:54:00:00:00:b2', kind: 'normal' },
    { at: 13, name: 'cy',   mac: '52:54:00:00:00:c3', kind: 'normal' },
    { at: 20, name: 'dee',  mac: '52:54:00:00:00:d4', kind: 'normal' },
    { at: 28, name: 'eda',  mac: '52:54:00:00:00:e5', kind: 'rogue', wants: '10.0.2.99' },
    { at: 36, name: 'fox',  mac: '52:54:00:00:00:f6', kind: 'rogue', wants: '10.0.2.15' },
    { at: 45, name: 'gil',  mac: '52:54:00:00:00:07', kind: 'normal' }
  ];

  /* ---- dom handles ---- */
  var poolEl   = root.querySelector('[data-lo-pool]');
  var roomEl   = root.querySelector('[data-lo-room]');
  var logEl    = root.querySelector('[data-lo-log]');
  var prevEl   = root.querySelector('[data-lo-preview]');
  var hintEl   = root.querySelector('[data-lo-hint]');
  var leasesEl = root.querySelector('[data-lo-leases]');
  var strikesEl= root.querySelector('[data-lo-strikes]');
  var endEl    = root.querySelector('[data-lo-end]');
  var verdictEl= root.querySelector('[data-lo-verdict]');
  var emptyEl  = root.querySelector('.lo-empty');

  /* ---- mutable state ---- */
  var t, timerId, spawned, clients, selected, leases, good, strikes, pool;

  function hex(n) { return '0x' + n.toString(16).padStart(4, '0'); }

  function log(msg, cls) {
    var li = document.createElement('li');
    li.textContent = msg;
    li.className = cls || 'lo-wire';
    logEl.appendChild(li);
    logEl.scrollTop = logEl.scrollHeight;
  }

  function score() {
    leasesEl.textContent = 'leases ' + leases;
    strikesEl.textContent = 'strikes ' + strikes;
    strikesEl.className = 'lo-chip' + (strikes > 0 ? ' lo-bad' : '');
    leasesEl.className = 'lo-chip' + (leases > 0 ? ' lo-good' : '');
  }

  function flashHint(msg) {
    hintEl.textContent = msg;
    hintEl.classList.add('flash');
    setTimeout(function () { hintEl.classList.remove('flash'); }, 1800);
  }

  /* ---- pool ---- */
  function buildPool() {
    poolEl.innerHTML = '';
    pool = POOL_IPS.map(function (ip) {
      var el = document.createElement('div');
      el.className = 'lo-slot';
      el.dataset.state = 'free';
      el.innerHTML =
        '<code>' + ip + '</code>' +
        '<span class="lo-slot-state">free</span>' +
        '<span class="lo-slot-bar"><i style="width:0%"></i></span>';
      poolEl.appendChild(el);
      return {
        ip: ip, state: 'free', holder: null, timeLeft: 0,
        el: el,
        stateEl: el.querySelector('.lo-slot-state'),
        barEl: el.querySelector('.lo-slot-bar i')
      };
    });
  }

  function firstFree() {
    for (var i = 0; i < pool.length; i++) if (pool[i].state === 'free') return pool[i];
    return null;
  }

  function poolFull() { return !firstFree(); }

  function renderSlot(s) {
    s.el.dataset.state = s.state;
    if (s.state === 'free') {
      s.stateEl.textContent = 'free';
      s.barEl.style.width = '0%';
      return;
    }
    var total = s.state === 'held' ? HOLD : LEASE;
    var who = s.holder ? s.holder.name : '?';
    s.stateEl.textContent =
      (s.state === 'held' ? 'offered to ' : 'leased to ') + who +
      ' · ' + Math.ceil(s.timeLeft) + 's';
    s.barEl.style.width = Math.max(0, (s.timeLeft / total) * 100) + '%';
  }

  function freeSlot(s, why) {
    var was = s.state, who = s.holder ? s.holder.name : '?';
    s.state = 'free'; s.holder = null; s.timeLeft = 0;
    renderSlot(s);
    if (was === 'leased') log('lease on ' + s.ip + ' expired (was ' + who + ') — back in the pool');
    else if (why !== 'quiet') log('offer of ' + s.ip + ' to ' + who + ' lapsed — back in the pool');
  }

  /* ---- clients ---- */
  function spawnClient(spec, index) {
    var xid = 0x1049 + index * 0x373;
    var c = {
      id: spec.name, name: spec.name, mac: spec.mac, kind: spec.kind,
      xid: xid, stage: spec.kind === 'rogue' ? 'request' : 'discover',
      wants: spec.wants || null,
      offeredIp: null, requestIn: null,
      patience: PATIENCE, done: false,
      el: null, barEl: null, bubbleEl: null
    };

    var el = document.createElement('button');
    el.type = 'button';
    el.className = 'lo-client';
    el.innerHTML =
      '<span class="lo-avatar">' + spec.name[0] + '</span>' +
      '<span class="lo-who"><strong>' + spec.name + '</strong><code>' + spec.mac + '</code></span>' +
      '<span class="lo-bubble"></span>' +
      '<span class="lo-patience"><i></i></span>';
    el.addEventListener('click', function () { select(c); });
    c.el = el;
    c.bubbleEl = el.querySelector('.lo-bubble');
    c.barEl = el.querySelector('.lo-patience i');

    setBubble(c);
    if (emptyEl) emptyEl.hidden = true;
    roomEl.appendChild(el);
    clients.push(c);

    if (c.kind === 'rogue') {
      log(c.name + ' → DHCPREQUEST for ' + c.wants + ' (no DISCOVER first) · xid=' + hex(c.xid));
    } else {
      log(c.name + ' → DHCPDISCOVER · xid=' + hex(c.xid) + ' · src 0.0.0.0:68 → 255.255.255.255:67');
    }
  }

  function setBubble(c) {
    if (c.stage === 'discover') {
      c.bubbleEl.innerHTML =
        '<strong>DISCOVER</strong> xid=' + hex(c.xid) +
        ' · “anyone out there? I need an address”';
    } else if (c.stage === 'request') {
      c.bubbleEl.innerHTML =
        '<strong>REQUEST</strong> xid=' + hex(c.xid) +
        ' · “I’ll take <strong>' + c.wants + '</strong>”';
    } else if (c.stage === 'offered') {
      c.bubbleEl.innerHTML =
        'reading your OFFER… REQUEST for ' + c.offeredIp + ' coming';
    }
  }

  function select(c) {
    if (c.done) return;
    if (selected) selected.el.classList.remove('selected');
    selected = c;
    c.el.classList.add('selected');
    renderPreview();
  }

  function deselect() {
    if (selected) selected.el.classList.remove('selected');
    selected = null;
    renderPreview();
  }

  function removeClient(c) {
    c.done = true;
    if (selected === c) deselect();
    c.el.classList.add('lo-gone');
    setTimeout(function () { c.el.remove(); }, 450);
    checkEnd();
  }

  /* ---- reply preview ---- */
  function renderPreview() {
    if (!selected) { prevEl.textContent = 'select a client to inspect its message'; return; }
    var c = selected;
    if (c.stage === 'discover') {
      var slot = firstFree();
      prevEl.innerHTML =
        'incoming: DHCPDISCOVER xid=' + hex(c.xid) + ' chaddr=' + c.mac + '\n' +
        'OFFER would carry: yiaddr=' + (slot ? slot.ip : '— pool empty') +
        ' · xid=' + hex(c.xid) + ' (echoed) · opt 53=2 · opt 51 lease=' + LEASE + 's';
      return;
    }
    if (c.stage === 'offered') {
      prevEl.innerHTML =
        'you offered ' + c.offeredIp + ' to ' + c.name + ' (held ' + HOLD + 's)\n' +
        'the client will REQUEST it shortly — answer that REQUEST with ACK';
      return;
    }
    var s = pool.filter(function (p) { return p.ip === c.wants; })[0];
    var yours = s && s.state === 'held' && s.holder && s.holder.id === c.id;
    prevEl.innerHTML =
      'incoming: DHCPREQUEST xid=' + hex(c.xid) + ' wants ' + c.wants + '\n' +
      (yours
        ? 'you offered ' + c.wants + ' to ' + c.name + ' — ACK commits the lease'
        : c.wants + ' was never offered to ' + c.name + ' — NAK fails it fast');
  }

  /* ---- player actions ---- */
  function act(kind) {
    if (!selected) { flashHint('select a client first'); return; }
    var c = selected;
    if (c.done) return;

    if (c.stage === 'discover') {
      if (kind !== 'offer') {
        strike(kind === 'ack'
          ? 'ACK answers a REQUEST. ' + c.name + ' only asked who is out there — send an OFFER.'
          : 'NAK tells a client to start over. ' + c.name + ' asked a fair question — send an OFFER.');
        return;
      }
      var slot = firstFree();
      if (!slot) { flashHint('pool empty — wait for a lease to lapse, silence is legal'); return; }
      slot.state = 'held'; slot.holder = c; slot.timeLeft = HOLD;
      c.offeredIp = slot.ip; c.stage = 'offered'; c.requestIn = REQUEST_DELAY;
      setBubble(c);
      good++;
      log('→ DHCPOFFER yiaddr=' + slot.ip + ' xid=' + hex(c.xid) + ' (echoed) · opt 53=2 · lease ' + LEASE + 's', 'lo-ok');
      renderSlot(slot); renderPreview();
      return;
    }

    if (c.stage === 'request') {
      var s = pool.filter(function (p) { return p.ip === c.wants; })[0];
      var heldForC = s && s.state === 'held' && s.holder && s.holder.id === c.id;

      if (kind === 'ack') {
        if (heldForC) {
          s.state = 'leased'; s.timeLeft = LEASE;
          leases++; good++;
          log('→ DHCPACK ' + c.wants + ' → ' + c.name + ' · lease ' + LEASE + 's · ' + c.name + ' is BOUND', 'lo-ok');
          renderSlot(s);
          removeClient(c);
        } else {
          strike('You never offered ' + c.wants + ' to ' + c.name + '. An ACK would hand out an address with no offer behind it — NAK instead.');
        }
        return;
      }
      if (kind === 'nak') {
        if (heldForC) {
          strike('You offered ' + c.wants + ' to ' + c.name + ' moments ago. NAK discards your own offer and sends ' + c.name + ' back to DISCOVER.');
        } else {
          good++;
          log('→ DHCPNAK for ' + c.wants + ' · ' + c.name + ' restarts at DISCOVER (correct: no matching offer)', 'lo-ok');
          removeClient(c);
        }
        return;
      }
      strike('The REQUEST is ' + c.name + ' committing. Answer it — ACK if you made the offer, NAK if you did not.');
      return;
    }

    if (c.stage === 'offered') {
      flashHint('OFFER already sent — ' + c.name + ' will REQUEST in a moment');
    }
  }

  function strike(msg) {
    strikes++;
    log('✗ ' + msg, 'lo-err');
    score();
  }

  /* ---- timeouts ---- */
  function timeoutClient(c) {
    if (c.stage === 'discover' && poolFull()) {
      log(c.name + ' gave up waiting. Pool stayed full, and silence is the legal answer to an unservable DISCOVER.');
      removeClient(c);
      return;
    }
    if (c.stage === 'request' && !c.offeredIp) {
      strike(c.name + ' waited, retried, and started over. A NAK fails a bad REQUEST fast — silence costs everyone time.');
    } else {
      strike(c.name + ' stormed off — its retries are now broadcast noise on port 67.');
    }
    removeClient(c);
  }

  /* ---- main loop ---- */
  function tick() {
    t += TICK;

    while (spawned < SCRIPT.length && SCRIPT[spawned].at <= t) {
      spawnClient(SCRIPT[spawned], spawned);
      spawned++;
    }

    clients.forEach(function (c) {
      if (c.done) return;

      if (c.requestIn !== null) {
        c.requestIn -= TICK;
        if (c.requestIn <= 0) {
          c.requestIn = null;
          c.stage = 'request';
          c.wants = c.offeredIp;
          setBubble(c);
          log(c.name + ' → DHCPREQUEST ' + c.wants + ' · xid=' + hex(c.xid) + ' (same transaction)');
          if (selected === c) renderPreview();
        }
      }

      c.patience -= TICK;
      var frac = Math.max(0, c.patience / PATIENCE);
      c.barEl.style.width = frac * 100 + '%';
      c.el.classList.toggle('impatient', frac < 0.5 && frac >= 0.25);
      c.el.classList.toggle('desperate', frac < 0.25);
      if (c.patience <= 0) timeoutClient(c);
    });

    pool.forEach(function (s) {
      if (s.state !== 'free') {
        s.timeLeft -= TICK;
        if (s.timeLeft <= 0) freeSlot(s);
      }
      renderSlot(s);
    });
  }

  /* ---- end of day ---- */
  function checkEnd() {
    if (spawned < SCRIPT.length) return;
    if (clients.some(function (c) { return !c.done; })) return;

    clearInterval(timerId);
    var verdict;
    if (strikes === 0) {
      verdict = 'Flawless day. ' + leases + ' lease(s) signed, zero protocol violations — RFC 2131 §3.1 in the flesh.';
    } else if (strikes <= 2) {
      verdict = leases + ' lease(s) signed with ' + strikes + ' strike(s). Scroll the wire log — each strike names the rule it broke.';
    } else {
      verdict = leases + ' lease(s), ' + strikes + ' strikes. Reread the DORA exchange above and run the office again.';
    }
    verdictEl.textContent = verdict + ' Now build the other side: step 30.2 writes the client.';
    endEl.hidden = false;
  }

  /* ---- reset ---- */
  function reset() {
    clearInterval(timerId);
    t = 0; spawned = 0; clients = []; selected = null;
    leases = 0; good = 0; strikes = 0;
    roomEl.innerHTML = '';
    logEl.innerHTML = '';
    endEl.hidden = true;
    if (emptyEl) { roomEl.appendChild(emptyEl); emptyEl.hidden = false; }
    buildPool();
    score();
    renderPreview();
    log('office open · pool ' + POOL_IPS.join(', ') + ' · server 10.0.2.2:67');
    timerId = setInterval(tick, TICK * 1000);
  }

  /* ---- wiring ---- */
  root.querySelectorAll('[data-lo-act]').forEach(function (btn) {
    btn.addEventListener('click', function () { act(btn.dataset.loAct); });
  });
  root.querySelectorAll('[data-lo-restart]').forEach(function (btn) {
    btn.addEventListener('click', reset);
  });

  reset();
})();
