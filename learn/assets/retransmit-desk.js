/* ============================================================
   retransmit-desk.js — "The Retransmit Desk" TCP sender minigame
   The player runs the TCP sender from Lesson 32. A 96-byte file
   leaves in 12-byte segments (the test MSS of step 32.6) over a
   link with ~1000 ms RTT that silently drops one segment. The
   player's decisions are the ones the lesson implements:
     - SEND only while in-flight bytes fit inside
       send_window = min(advertised_window, congestion_window)
     - hold every sent segment in the retransmit queue until a
       cumulative ACK covers it
     - RETRANSMIT the oldest held segment only after its RTO
       expires (early = waste, late = stall; each retransmit
       doubles the RTO, a clean sample cancels the backoff)
     - never sample RTT from an ACK that covers a retransmitted
       segment (Karn's algorithm)
   ============================================================ */

(function () {
  'use strict';

  var root = document.querySelector('[data-retransmit-desk]');
  if (!root) return;

  /* ---- tunables (milliseconds; wall clock runs 1:1) ---- */
  var MSS = 12;               /* the 12-byte test MSS of step 32.6     */
  var TOTAL = 96;             /* bytes the application sends           */
  var CWND = 4 * MSS;         /* fixed congestion window, like mini-os */
  var TICK = 100;
  var ONE_WAY = 500;          /* segment or ACK travel time            */
  var GRAN = 10;              /* clock granularity G in RTO = SRTT + max(G, 4*RTTVAR) */
  var INIT_RTO = 3000;        /* SRTT 1000 + 4 * RTTVAR 500, the lesson's init */
  var MIN_RTO = 1000;
  var MAX_RTO = 6000;
  var DROP_FIRST = { 60: true }; /* first transmission of seq 60 vanishes */
  var SHRINK_AT = 48;         /* receiver shrinks its window here      */
  var REOPEN_AT = 84;         /* receiver reopens its window here      */

  var NSEG = TOTAL / MSS;

  /* ---- dom handles ---- */
  var streamEl  = root.querySelector('[data-rd-stream]');
  var uaLine    = root.querySelector('[data-rd-ua]');
  var nxLine    = root.querySelector('[data-rd-nx]');
  var edgeLine  = root.querySelector('[data-rd-edge]');
  var legendEl  = root.querySelector('[data-rd-legend]');
  var winEl     = root.querySelector('[data-rd-winread]');
  var queueEl   = root.querySelector('[data-rd-queue]');
  var rttEl     = root.querySelector('[data-rd-rtt]');
  var wireEl    = root.querySelector('[data-rd-wire]');
  var cardEl    = root.querySelector('[data-rd-card]');
  var recvEl    = root.querySelector('[data-rd-recv]');
  var logEl     = root.querySelector('[data-rd-log]');
  var hintEl    = root.querySelector('[data-rd-hint]');
  var deliveredEl = root.querySelector('[data-rd-delivered]');
  var strikesEl = root.querySelector('[data-rd-strikes]');
  var wastedEl  = root.querySelector('[data-rd-wasted]');
  var samplesEl = root.querySelector('[data-rd-samples]');
  var endEl     = root.querySelector('[data-rd-end]');
  var verdictEl = root.querySelector('[data-rd-verdict]');

  /* ---- mutable state ---- */
  var t, timerId, segs, appAvail, sendUnacked, sendNext, advertised,
      haveSample, srtt, rttvar, computedRto, currentRto,
      recvNext, buffered, flying, strikes, wasted, samples,
      card, ended, nudged, renderedSig;

  /* ---- helpers ---- */
  function clamp(v, lo, hi) { return Math.min(hi, Math.max(lo, v)); }

  function log(msg, cls) {
    var li = document.createElement('li');
    li.textContent = msg;
    li.className = cls || 'rd-wire';
    logEl.appendChild(li);
    logEl.scrollTop = logEl.scrollHeight;
  }

  function score() {
    deliveredEl.textContent = 'delivered ' + recvNext + '/' + TOTAL;
    deliveredEl.className = 'rd-chip' + (recvNext === TOTAL ? ' rd-good' : '');
    strikesEl.textContent = 'strikes ' + strikes;
    strikesEl.className = 'rd-chip' + (strikes > 0 ? ' rd-bad' : '');
    wastedEl.textContent = 'wasted ' + wasted + ' B';
    wastedEl.className = 'rd-chip' + (wasted > 0 ? ' rd-bad' : '');
    samplesEl.textContent = 'samples ' + samples;
    samplesEl.className = 'rd-chip' + (samples > 0 ? ' rd-good' : '');
  }

  function flashHint(msg) {
    hintEl.textContent = msg;
    hintEl.classList.add('flash');
    setTimeout(function () { hintEl.classList.remove('flash'); }, 1800);
  }

  function strike(msg) {
    strikes++;
    log('✗ ' + msg, 'rd-err');
    score();
  }

  /* ---- sender state ---- */
  function firstInflight() {
    for (var i = 0; i < segs.length; i++) if (segs[i].state === 'inflight') return segs[i];
    return null;
  }

  function renderStream() {
    segs.forEach(function (s) {
      var st = s.state;
      if (st === 'pending' || st === 'ready') st = s.seq < appAvail ? 'ready' : 'pending';
      s.blockEl.dataset.state = st;
    });
  }

  function renderWin() {
    var win = Math.min(advertised, CWND);
    var edge = Math.min(sendUnacked + win, TOTAL);
    winEl.textContent =
      'advertised ' + advertised + ' · congestion ' + CWND +
      ' · send_window = ' + win + ' B · in flight ' + (sendNext - sendUnacked) + ' B';
    legendEl.textContent =
      'send_unacked ' + sendUnacked + ' · send_next ' + sendNext + ' · window edge ' + edge;
    uaLine.style.left = (sendUnacked / TOTAL * 100) + '%';
    nxLine.style.left = (sendNext / TOTAL * 100) + '%';
    edgeLine.style.left = (edge / TOTAL * 100) + '%';
  }

  function renderRtt() {
    if (!haveSample) {
      rttEl.innerHTML = 'no RTT sample yet<br>RTO ' + currentRto + ' ms (initial value, RFC 6298 §2.1)';
      return;
    }
    rttEl.innerHTML =
      'SRTT ' + srtt + ' ms · RTTVAR ' + rttvar + ' ms<br>RTO ' + currentRto + ' ms' +
      (currentRto > computedRto
        ? ' <span class="rd-backoff">backoff — a clean sample restores ' + computedRto + ' ms</span>'
        : '');
  }

  function renderRecv() {
    recvEl.textContent =
      'recv_next ' + recvNext + '\n' +
      'delivered ' + recvNext + '/' + TOTAL + ' B\n' +
      'buffered ' + (buffered.length
        ? buffered.map(function (s) { return s + '–' + (s + MSS - 1); }).join(', ') + ' (waiting on ' + recvNext + ')'
        : '—') + '\n' +
      'advertised window ' + advertised + ' B';
  }

  /* ---- retransmit queue panel ---- */
  function syncQueue() {
    var sig = segs.filter(function (s) { return s.state === 'inflight'; })
      .map(function (s) { return s.seq + ':' + s.retrans; }).join(',');
    if (sig === renderedSig) return;
    renderedSig = sig;
    queueEl.innerHTML = '';
    var list = segs.filter(function (s) { return s.state === 'inflight'; });
    if (!list.length) {
      queueEl.innerHTML = '<p class="rd-empty">nothing held — every sent byte is acknowledged</p>';
      return;
    }
    var oldest = firstInflight();
    list.forEach(function (s) {
      var el = document.createElement('div');
      el.className = 'rd-qentry' + (s === oldest ? ' rd-oldest' : '');
      el.innerHTML =
        '<code>seq ' + s.seq + '–' + (s.seq + MSS - 1) + '</code>' +
        '<span class="rd-qstate"></span>' +
        (s === oldest ? '<span class="rd-qbar"><i></i></span>' : '');
      s.qEl = el;
      s.qStateEl = el.querySelector('.rd-qstate');
      s.qBarEl = el.querySelector('.rd-qbar i');
      queueEl.appendChild(el);
    });
  }

  function paintQueue() {
    var oldest = firstInflight();
    segs.forEach(function (s) {
      if (s.state !== 'inflight' || !s.qEl) return;
      var held = ((t - s.sentAt) / 1000).toFixed(1);
      if (s === oldest) {
        var frac = (t - s.sentAt) / currentRto;
        var expired = frac >= 1;
        s.qEl.classList.toggle('rd-expired', expired);
        s.qStateEl.textContent = expired
          ? 'RTO EXPIRED — retransmit now'
          : 'held ' + held + ' s · re×' + s.retrans + ' · RTO ' + Math.round(t - s.sentAt) + '/' + currentRto + ' ms';
        if (s.qBarEl) s.qBarEl.style.width = Math.min(100, frac * 100) + '%';
      } else {
        s.qStateEl.textContent = 'held ' + held + ' s · re×' + s.retrans + ' · queued behind seq ' + oldest.seq;
      }
    });
  }

  /* ---- flying packets on the wire ---- */
  function makePkt(label, cls, lane) {
    var el = document.createElement('span');
    el.className = 'rd-pkt ' + cls;
    el.textContent = label;
    el.style.top = lane === 'seg' ? '24%' : '58%';
    el.style.left = lane === 'seg' ? '6%' : '90%';
    wireEl.appendChild(el);
    return el;
  }

  function spawnSeg(seg) {
    var drop = !!DROP_FIRST[seg.seq] && seg.retrans === 0 && !seg.dropConsumed;
    if (drop) seg.dropConsumed = true;
    flying.push({
      kind: 'seg', seg: seg, frac: 0, drop: drop, dropLogged: false,
      el: makePkt('seq ' + seg.seq + (seg.retrans > 0 ? ' ·re×' + seg.retrans : ''), 'rd-seg', 'seg')
    });
  }

  function spawnAck(ack, dup) {
    flying.push({
      kind: 'ack', ack: ack, dup: dup, frac: 0,
      el: makePkt('ACK ' + ack + (dup ? ' ·dup' : ''), 'rd-ack' + (dup ? ' rd-dup' : ''), 'ack')
    });
  }

  /* ---- receiver model (slirp: buffers out-of-order, cumulative ACKs) ---- */
  function arriveSeg(seg) {
    if (ended) return;
    var end = seg.seq + MSS;
    if (end <= recvNext) {
      wasted += MSS;
      log('bytes ' + seg.seq + '–' + (end - 1) + ' arrived again — recv_next is past them, so the repeat is dropped and re-ACKed. Sequence numbers block double delivery (the probe below).', 'rd-warn');
      spawnAck(recvNext, true);
      score();
      return;
    }
    if (seg.seq === recvNext) {
      recvNext = end;
      log('rx seq=' + seg.seq + ' in order · recv_next = ' + recvNext);
      var moved = true;
      while (moved) {
        moved = false;
        for (var i = 0; i < buffered.length; i++) {
          if (buffered[i] === recvNext) {
            buffered.splice(i, 1);
            log('buffered segment seq=' + recvNext + ' released past the gap · recv_next = ' + (recvNext + MSS), 'rd-ok');
            recvNext += MSS;
            moved = true;
            break;
          }
        }
      }
      spawnAck(recvNext, false);
      renderRecv();
      score();
      return;
    }
    if (buffered.indexOf(seg.seq) === -1) {
      buffered.push(seg.seq);
      log('rx seq=' + seg.seq + ' out of order — recv_next is stuck at ' + recvNext + '. Bytes buffered; the cumulative ACK cannot pass the gap.', 'rd-warn');
    }
    spawnAck(recvNext, true);
    renderRecv();
  }

  /* ---- ACK processing at the sender ---- */
  function doSample(r, polluted) {
    samples++;
    r = Math.max(1, Math.round(r));
    if (!haveSample) {
      srtt = r;
      rttvar = Math.max(1, Math.round(r / 2));
      haveSample = true;
    } else {
      rttvar = Math.round((3 * rttvar + Math.abs(srtt - r)) / 4);
      srtt = Math.round((7 * srtt + r) / 8);
    }
    computedRto = clamp(srtt + Math.max(GRAN, 4 * rttvar), MIN_RTO, MAX_RTO);
    var old = currentRto;
    currentRto = computedRto;
    log('RTT sample ' + r + ' ms → SRTT ' + srtt + ' · RTTVAR ' + rttvar + ' · RTO ' + computedRto + ' ms' +
      (polluted ? ' (a guess, fed in anyway)' : ''), polluted ? 'rd-err' : 'rd-wire');
    if (!polluted && old > computedRto) {
      log('fresh sample cancels the backoff — current RTO ' + old + ' → ' + computedRto + ' ms (RFC 6298 §2.5)', 'rd-ok');
    }
    renderRtt();
    score();
  }

  function arriveAck(f) {
    if (ended) return;
    if (f.ack <= sendUnacked) {
      log('ACK ' + f.ack + ' brings no progress — send_unacked is already ' + sendUnacked + '. mini-os counts no duplicate ACKs; the timer decides.');
      return;
    }
    var released = segs.filter(function (s) { return s.state === 'inflight' && s.seq + MSS <= f.ack; });
    var retransSeqs = [];
    var lastClean = null;
    released.forEach(function (s) {
      s.state = 'acked';
      if (s.retrans > 0) retransSeqs.push(s.seq); else lastClean = s;
    });
    sendUnacked = f.ack;
    log('ACK ' + f.ack + ' · released ' + released.length + (released.length === 1 ? ' entry' : ' entries') +
      ' (' + released.length * MSS + ' B) · send_unacked = ' + f.ack + ' · the window slides forward', 'rd-ok');
    renderStream();
    renderWin();
    syncQueue();
    if (retransSeqs.length > 0) openCard(f.ack, released.length * MSS, retransSeqs);
    else if (lastClean) doSample(t - lastClean.sentAt, false);
    checkTriggers();
    checkWin();
  }

  /* ---- Karn decision card ---- */
  function openCard(ack, releasedBytes, retransSeqs) {
    if (card) {
      log('an earlier ACK decision was auto-resolved: skipped (Karn is the safe default)', 'rd-warn');
    }
    card = { ack: ack, seqs: retransSeqs };
    var ranges = retransSeqs.map(function (s) { return s + '–' + (s + MSS - 1); }).join(', ');
    cardEl.innerHTML =
      '<h4>ACK ' + ack + ' — ambiguous timing</h4>' +
      '<p>It released ' + releasedBytes + ' B, but seq ' + ranges + ' carried retransmitted bytes. ' +
      'This ACK may confirm the original transmission or the retransmit — you cannot tell which.</p>' +
      '<p>Feed its round-trip time into SRTT?</p>' +
      '<div class="rd-card-actions">' +
      '<button type="button" data-rd-card-btn="sample">SAMPLE</button>' +
      '<button type="button" data-rd-card-btn="skip">SKIP — KARN</button>' +
      '</div>';
    cardEl.hidden = false;
  }

  function decideCard(sampleIt) {
    var c = card;
    if (!c) return;
    card = null;
    cardEl.hidden = true;
    cardEl.innerHTML = '';
    if (sampleIt) {
      strike('Karn’s algorithm: the ACK might confirm the original slow transmission, not the retransmit. The sample is a guess, and SRTT now includes it.');
      var seg = segs[c.seqs[0] / MSS];
      doSample(t - seg.sentAt, true);
    } else {
      log('Karn: byte progress counts, timing does not — no sample from ACK ' + c.ack + '. SRTT stays ' + (haveSample ? srtt : '—') + ' ms.', 'rd-ok');
    }
  }

  cardEl.addEventListener('click', function (ev) {
    var b = ev.target;
    while (b && b !== cardEl && !b.dataset.rdCardBtn) b = b.parentNode;
    if (!b || !b.dataset.rdCardBtn) return;
    decideCard(b.dataset.rdCardBtn === 'sample');
  });

  /* ---- player actions ---- */
  function actSend() {
    if (ended) return;
    var win = Math.min(advertised, CWND);
    var inFlight = sendNext - sendUnacked;
    if (sendNext < appAvail && inFlight >= win) {
      strike('window edge: ' + inFlight + ' B in flight = send_window ' + win +
        ' B (min(advertised ' + advertised + ', congestion ' + CWND + ')). Only an ACK slides the edge.');
      return;
    }
    if (sendNext >= appAvail) {
      flashHint('all ' + TOTAL + ' B are sent — waiting on ACKs');
      return;
    }
    var seg = segs[sendNext / MSS];
    seg.state = 'inflight';
    seg.sentAt = t;
    seg.retrans = 0;
    seg.expiredAnnounced = false;
    sendNext += MSS;
    spawnSeg(seg);
    log('tx seq=' + seg.seq + ' len=' + MSS + ' · copy held in the retransmit queue · in flight ' + (sendNext - sendUnacked) + ' B');
    renderStream();
    renderWin();
    syncQueue();
  }

  function actRexmit() {
    if (ended) return;
    var oldest = firstInflight();
    if (!oldest) {
      flashHint('retransmit queue empty — every sent byte is acknowledged');
      return;
    }
    var remaining = currentRto - (t - oldest.sentAt);
    if (remaining > 0) {
      strike(oldest.dropConsumed
        ? 'RTO had ' + Math.round(remaining) + ' ms left for seq=' + oldest.seq + '. It really was lost — but you could not know that before the timer. The calibrated RTO is the loss detector; guessing also fires on healthy links.'
        : 'RTO had ' + Math.round(remaining) + ' ms left for seq=' + oldest.seq + '. The ACK may still be in flight — a premature retransmit spends bandwidth and marks the segment, so its ACK turns ambiguous under Karn.');
      rexmit(oldest, true);
      return;
    }
    rexmit(oldest, false);
  }

  function rexmit(seg, early) {
    seg.retrans++;
    seg.sentAt = t;
    seg.expiredAnnounced = false;
    var old = currentRto;
    currentRto = Math.min(currentRto * 2, MAX_RTO);
    spawnSeg(seg);
    log((early ? 'retransmitted seq=' : 'RTO expired — retransmit seq=') + seg.seq +
      ' · re×' + seg.retrans + ' · RTO ' + old + ' → ' + currentRto + ' ms (exponential backoff)',
      early ? 'rd-warn' : 'rd-err');
    renderRtt();
    syncQueue();
  }

  /* ---- scripted beats ---- */
  function checkTriggers() {
    if (advertised === 48 && sendUnacked >= SHRINK_AT) {
      advertised = 24;
      log('receiver app stopped reading — advertised window 48 → 24 · send_window = min(24, 48) = 24 B', 'rd-warn');
      renderStream();
      renderWin();
      renderRecv();
    }
    if (advertised === 24 && sendUnacked >= REOPEN_AT) {
      advertised = 48;
      log('receiver app caught up — advertised window 24 → 48 · send_window = min(48, 48) = 48 B', 'rd-warn');
      renderWin();
      renderRecv();
    }
  }

  function checkWin() {
    if (ended || sendUnacked < TOTAL) return;
    ended = true;
    clearInterval(timerId);
    if (card) {
      card = null;
      cardEl.hidden = true;
      cardEl.innerHTML = '';
    }
    var verdict;
    if (strikes === 0) {
      verdict = 'Flawless shift — 96/96 B delivered with zero protocol violations. The queue, the window check, the timer, and Karn’s rule all held.';
    } else if (strikes <= 2) {
      verdict = '96/96 B delivered with ' + strikes + ' strike(s). Scroll the wire log — each strike names the rule it broke.';
    } else {
      verdict = 'Delivered, but ' + strikes + ' strikes. Reread the sliding-window and retransmission-timer sections, then run the desk again.';
    }
    if (wasted > 0) {
      verdict += ' The wire also carried ' + wasted + ' B of duplicates — premature retransmits are not free.';
    }
    verdict += ' Steps 32.1–32.5 wire these exact decisions into the TCB.';
    verdictEl.textContent = verdict;
    endEl.hidden = false;
  }

  /* ---- main loop ---- */
  function tick() {
    t += TICK;

    for (var i = flying.length - 1; i >= 0; i--) {
      var f = flying[i];
      f.frac += TICK / ONE_WAY;
      if (f.kind === 'seg' && f.drop && !f.dropLogged && f.frac >= 0.55) {
        f.dropLogged = true;
        f.el.classList.add('rd-lost');
        f.el.textContent = 'seq ' + f.seg.seq + ' ✕';
        if (f.seg.state !== 'acked') {
          log('seq=' + f.seg.seq + ' vanished on the wire — a full queue somewhere discarded it. No notification reaches either side.', 'rd-err');
        }
      }
      if (f.kind === 'seg' && f.drop && f.frac >= 0.8) {
        f.el.remove();
        flying.splice(i, 1);
        continue;
      }
      if (f.frac >= 1) {
        f.el.remove();
        flying.splice(i, 1);
        if (f.kind === 'seg') arriveSeg(f.seg); else arriveAck(f);
        continue;
      }
      f.el.style.left = f.kind === 'seg' ? (6 + f.frac * 84) + '%' : (90 - f.frac * 84) + '%';
    }

    var oldest = firstInflight();
    if (oldest) {
      var elapsed = t - oldest.sentAt;
      if (!oldest.expiredAnnounced && elapsed >= currentRto) {
        oldest.expiredAnnounced = true;
        log('RTO expired for seq=' + oldest.seq + ' — no ACK in ' + currentRto + ' ms. The segment is presumed lost: retransmit it.', 'rd-err');
      }
      if (elapsed >= 2 * currentRto) {
        strike('the stream stalled behind seq=' + oldest.seq + ' for a full extra RTO. The desk retransmitted for you — mini-os does this on the timer tick, no operator required.');
        rexmit(oldest, false);
      }
    }

    paintQueue();

    if (!nudged && t >= 8000 && sendNext === 0) {
      nudged = true;
      flashHint('SEND NEXT SEGMENT starts the stream');
    }
  }

  /* ---- reset ---- */
  function reset() {
    clearInterval(timerId);
    t = 0;
    sendUnacked = 0;
    sendNext = 0;
    appAvail = TOTAL;
    advertised = 48;
    haveSample = false;
    srtt = 0;
    rttvar = 0;
    computedRto = INIT_RTO;
    currentRto = INIT_RTO;
    recvNext = 0;
    buffered = [];
    flying = [];
    strikes = 0;
    wasted = 0;
    samples = 0;
    card = null;
    ended = false;
    nudged = false;
    renderedSig = null;

    wireEl.querySelectorAll('.rd-pkt').forEach(function (el) { el.remove(); });
    cardEl.hidden = true;
    cardEl.innerHTML = '';
    logEl.innerHTML = '';
    endEl.hidden = true;

    segs = [];
    streamEl.innerHTML = '';
    for (var i = 0; i < NSEG; i++) {
      var b = document.createElement('div');
      b.className = 'rd-block';
      b.innerHTML = '<span>' + (i * MSS) + '</span>';
      streamEl.appendChild(b);
      segs.push({
        seq: i * MSS, state: 'pending', retrans: 0, sentAt: 0,
        expiredAnnounced: false, dropConsumed: false,
        blockEl: b, qEl: null, qStateEl: null, qBarEl: null
      });
    }

    score();
    renderStream();
    renderWin();
    renderRtt();
    renderRecv();
    syncQueue();

    log('connection established (Lesson 31) · sequences shown relative to the first data byte');
    log('application wrote ' + TOTAL + ' B · advertised 48 · congestion 48 · send_window = 48 B');
    log('link RTT ≈ 1000 ms · RTO starts at ' + INIT_RTO + ' ms — no sample yet (RFC 6298 §2.1)');
    log('SEND fills the window; the edge stops the fifth segment. One segment on this link will vanish without a word.');

    timerId = setInterval(tick, TICK);
  }

  /* ---- wiring ---- */
  root.querySelectorAll('[data-rd-act]').forEach(function (btn) {
    btn.addEventListener('click', function () {
      if (btn.dataset.rdAct === 'send') actSend(); else actRexmit();
    });
  });
  root.querySelectorAll('[data-rd-restart]').forEach(function (btn) {
    btn.addEventListener('click', reset);
  });

  reset();
})();
