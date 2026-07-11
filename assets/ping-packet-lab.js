(function () {
  const world = document.querySelector('.world');
  const packet = document.querySelector('.packet');
  const status = document.querySelector('[data-status]');
  const progress = document.querySelector('[data-progress]');
  const typeValues = Array.from(document.querySelectorAll('[data-type]'));
  const ipValue = document.querySelector('[data-ip]');
  const arpValue = document.querySelector('[data-arp]');
  const queueValue = document.querySelector('[data-queue]');
  const matchValue = document.querySelector('[data-match]');
  const stations = Array.from(document.querySelectorAll('.station'));
  const keyboardHint = document.querySelector('[data-keyboard-hint]');
  const routeKicker = document.querySelector('[data-route-kicker]');
  const routeSummary = document.querySelector('[data-route-summary]');
  const routeScope = document.querySelector('[data-route-scope]');
  const routeFootnote = document.querySelector('[data-route-footnote]');
  const routeButtons = Array.from(document.querySelectorAll('[data-route]'));
  const boundary = document.querySelector('[data-boundary]');

  const routes = {
    lab: {
      scope: 'QEMU lab',
      kicker: 'Interactive network lab · level 01',
      summary: 'You are an ICMP echo request. Walk the packet through each layer, interact at the amber checkpoint, reach QEMU’s gateway, then carry the reply home. The route follows the real mini-os stack.',
      footnote: 'The QEMU lab mirrors the mini-os proof target. The second route mode is a conceptual Internet journey; a live route requires measurement and can change with routing policy.',
      start: 'You are a blank packet. Walk to mini-os and press E to build the request.',
      cards: {
        kernel: ['01 · KERNEL', 'mini-os', 'build echo / match reply'],
        ipv4: ['02 · NETWORK', 'IPv4', 'protocol 1 / addresses'],
        arp: ['03 · LINK', 'ARP cache', 'gateway IP → MAC'],
        nic: ['04 · DEVICE', 'virtio-net', 'TX queue / RX queue'],
        gateway: ['05 · QEMU', '10.0.2.2', 'gateway echo engine'],
      },
      stages: [
        { station: 'kernel', message: 'Build an ICMP echo request: type 8, code 0, ID 0x2400, sequence 0, payload "mini-os".', label: 'ICMP request built' },
        { station: 'ipv4', message: 'Wrap the echo bytes in IPv4: protocol 1, source 10.0.2.15, destination 10.0.2.2.', label: 'IPv4 envelope sealed' },
        { station: 'arp', message: 'Resolve the local gateway MAC address through ARP.', label: 'ARP cache resolved' },
        { station: 'nic', message: 'Place the Ethernet frame in the virtio TX queue and ring the device.', label: 'TX descriptor submitted' },
        { station: 'gateway', message: 'The QEMU gateway receives the echo request and returns a reply with type 0.', label: 'Gateway answered' },
        { station: 'nic', message: 'Take the returned frame from the virtio RX queue.', label: 'RX descriptor consumed' },
        { station: 'ipv4', message: 'Read IPv4 protocol 1 and demultiplex the payload to ICMP.', label: 'ICMP payload delivered' },
        { station: 'kernel', message: 'Match source, type, checksum, identifier, sequence, and payload.', label: 'Round trip verified' },
      ],
    },
    internet: {
      scope: 'Mac → Google',
      kicker: 'Interactive network lab · level 02',
      summary: 'Start on your Mac, cross the local link and home router, travel through an ISP and transit networks, reach a Google edge, then bring the ICMP reply back. This is a conceptual path: the live Internet chooses routes dynamically.',
      footnote: 'Names resolve through DNS before IP packets travel. ISP and Google routing can choose different forward and return paths, while firewall policy can filter ICMP. Use a traceroute for a live measurement.',
      start: 'You are a blank packet on your Mac. Walk to the host stack and press E to build the request.',
      cards: {
        kernel: ['01 · YOUR HOST', 'Mac network stack', 'build echo / match reply'],
        ipv4: ['02 · LOCAL LINK', 'Wi‑Fi or Ethernet', 'carry frame to router'],
        arp: ['03 · HOME EDGE', 'router + NAT', 'forward to ISP'],
        nic: ['04 · WIDE AREA', 'ISP + transit', 'route toward Google'],
        gateway: ['05 · DESTINATION', 'Google edge', 'return echo reply'],
      },
      stages: [
        { station: 'kernel', message: 'Build an ICMP echo request on the Mac. DNS has already resolved the Google name to an IP address.', label: 'Host request built' },
        { station: 'ipv4', message: 'Encapsulate the IP packet for the local Wi‑Fi or Ethernet link toward the home router.', label: 'Local-link frame sent' },
        { station: 'arp', message: 'The home router accepts the frame, applies routing and often network address translation, then forwards it to the ISP.', label: 'Home edge forwarded' },
        { station: 'nic', message: 'The ISP and transit networks choose a path toward a Google edge. This path can differ from the next ping.', label: 'Internet transit crossed' },
        { station: 'gateway', message: 'A Google edge receives the ICMP echo and forms the reply: type 0, matching ID, sequence, and payload.', label: 'Google edge replied' },
        { station: 'nic', message: 'The reply returns through Internet transit and the ISP. Reverse-path routing can use different routers.', label: 'Return transit crossed' },
        { station: 'ipv4', message: 'The home link delivers the returned frame to the Mac.', label: 'Local-link reply received' },
        { station: 'kernel', message: 'The Mac validates the ICMP reply against the request it sent.', label: 'Round trip verified' },
      ],
    },
  };

  const player = { x: 75, y: 445, size: 52, speed: 205, stage: 0, moving: new Set(), last: 0 };
  let routeMode = 'lab';

  function route() { return routes[routeMode]; }

  function applyRoute(mode) {
    routeMode = mode;
    const selected = route();
    routeKicker.textContent = selected.kicker;
    routeSummary.textContent = selected.summary;
    routeScope.textContent = selected.scope;
    routeFootnote.textContent = selected.footnote;
    boundary.textContent = mode === 'internet' ? 'public internet' : 'slirp boundary';
    world.classList.toggle('internet-route', mode === 'internet');
    stations.forEach((station) => {
      const [id, name, detail] = selected.cards[station.dataset.station];
      station.querySelector('.station-id').textContent = id;
      station.querySelector('strong').textContent = name;
      station.querySelector('small').textContent = detail;
    });
    routeButtons.forEach((button) => button.classList.toggle('active', button.dataset.route === mode));
    reset();
  }

  function bounds() {
    const rect = world.getBoundingClientRect();
    return { width: rect.width, height: rect.height };
  }

  function clampPlayer() {
    const { width, height } = bounds();
    player.x = Math.max(0, Math.min(width - player.size, player.x));
    player.y = Math.max(0, Math.min(height - player.size, player.y));
  }

  function drawPlayer() {
    clampPlayer();
    packet.style.setProperty('--packet-x', `${player.x}px`);
    packet.style.setProperty('--packet-y', `${player.y}px`);
    packet.style.transform = `translate(${player.x}px, ${player.y}px)`;
  }

  function nearestStation() {
    const worldRect = world.getBoundingClientRect();
    const px = worldRect.left + player.x + player.size / 2;
    const py = worldRect.top + player.y + player.size / 2;
    let closest = null;
    let distance = Infinity;
    stations.forEach((station) => {
      const rect = station.getBoundingClientRect();
      const cx = rect.left + rect.width / 2;
      const cy = rect.top + rect.height / 2;
      const d = Math.hypot(px - cx, py - cy);
      if (d < distance) { closest = station; distance = d; }
    });
    return { station: closest, distance };
  }

  function setStatus(message, tone = 'normal') {
    status.textContent = message;
    status.dataset.tone = tone;
  }

  function refreshConsole() {
    const stage = player.stage;
    const stages = route().stages;
    progress.textContent = `${stage}/${stages.length} checkpoints`;
    typeValues.forEach((node) => { node.textContent = stage === 0 ? 'unset' : stage < 5 ? '8 · request' : '0 · reply'; });
    ipValue.textContent = stage < 2 ? 'unwrapped' : 'proto 1';
    arpValue.textContent = stage < 3 ? 'unresolved' : routeMode === 'internet' ? 'home-router MAC' : '52:55:0a:00:02:02';
    queueValue.textContent = stage < 4 ? 'idle' : stage < 6 ? routeMode === 'internet' ? 'Internet transit' : 'virtio RX/TX' : 'returned';
    matchValue.textContent = stage < 8 ? 'waiting' : 'exact match';
    [...typeValues, ipValue, arpValue, queueValue, matchValue].forEach((node) => node.classList.remove('good', 'warn'));
    if (stage >= 1) typeValues.forEach((node) => { node.classList.add('good'); });
    if (stage >= 2) ipValue.className = 'good';
    if (stage >= 3) arpValue.className = 'good';
    if (stage >= 4) queueValue.className = 'good';
    if (stage >= 8) matchValue.className = 'good';
    packet.textContent = stage === 0 ? 'EMPTY' : stage < 5 ? 'ICMP 8' : 'ICMP 0';
    packet.classList.toggle('reply', stage >= 5);
    packet.classList.toggle('success', stage >= 8);
    stations.forEach((station) => station.classList.remove('active'));
    if (stage < stages.length) {
      const expected = document.querySelector(`[data-station="${stages[stage].station}"]`);
      expected.classList.add('active');
    }
    const completed = stages.slice(0, stage).map((step) => step.station);
    stations.forEach((station) => station.classList.toggle('done', completed.includes(station.dataset.station)));
    world.classList.toggle('success', stage >= 8);
  }

  function interact() {
    const stages = route().stages;
    if (player.stage >= stages.length) {
      setStatus('Round trip complete. Press R to become a fresh packet.', 'good');
      return;
    }
    const { station, distance } = nearestStation();
    const expected = stages[player.stage];
    if (distance > 90) {
      setStatus('Walk inside a station’s signal radius, then press E.', 'warn');
      return;
    }
    if (station.dataset.station !== expected.station) {
      setStatus(`This packet is waiting for ${expected.station.toUpperCase()}. Move to the amber station.`, 'warn');
      return;
    }
    player.stage += 1;
    setStatus(`${expected.label}: ${expected.message}`, 'good');
    refreshConsole();
  }

  function reset() {
    player.x = 75;
    player.y = Math.max(310, bounds().height - 112);
    player.stage = 0;
    packet.classList.remove('reply', 'success');
    setStatus(route().start);
    refreshConsole();
    drawPlayer();
  }

  function nudge(dx, dy) {
    player.x += dx;
    player.y += dy;
    drawPlayer();
  }

  function frame(now) {
    const dt = Math.min((now - player.last) / 1000 || 0, .05);
    player.last = now;
    let dx = 0;
    let dy = 0;
    if (player.moving.has('left')) dx -= 1;
    if (player.moving.has('right')) dx += 1;
    if (player.moving.has('up')) dy -= 1;
    if (player.moving.has('down')) dy += 1;
    if (dx || dy) {
      const length = Math.hypot(dx, dy);
      player.x += (dx / length) * player.speed * dt;
      player.y += (dy / length) * player.speed * dt;
      drawPlayer();
    }
    requestAnimationFrame(frame);
  }

  const keyMap = { ArrowLeft: 'left', a: 'left', A: 'left', ArrowRight: 'right', d: 'right', D: 'right', ArrowUp: 'up', w: 'up', W: 'up', ArrowDown: 'down', s: 'down', S: 'down' };
  window.addEventListener('keydown', (event) => {
    if (keyMap[event.key]) {
      event.preventDefault();
      player.moving.add(keyMap[event.key]);
      keyboardHint.textContent = 'Keyboard control active';
    }
    if (event.key === 'e' || event.key === 'E' || event.key === ' ') { event.preventDefault(); interact(); }
    if (event.key === 'r' || event.key === 'R') { reset(); }
  });
  window.addEventListener('keyup', (event) => { if (keyMap[event.key]) player.moving.delete(keyMap[event.key]); });
  window.addEventListener('resize', drawPlayer);

  document.querySelectorAll('[data-move]').forEach((button) => {
    button.addEventListener('click', () => {
      const direction = button.dataset.move;
      const steps = { left: [-28, 0], right: [28, 0], up: [0, -28], down: [0, 28] };
      nudge(...steps[direction]);
    });
  });
  document.querySelectorAll('[data-interact]').forEach((button) => button.addEventListener('click', interact));
  document.querySelectorAll('[data-reset]').forEach((button) => button.addEventListener('click', reset));
  routeButtons.forEach((button) => button.addEventListener('click', () => applyRoute(button.dataset.route)));
  stations.forEach((station) => station.addEventListener('click', () => {
    const { station: nearest, distance } = nearestStation();
    if (nearest === station && distance <= 90) interact();
    else setStatus('Walk your packet near this station before interacting.', 'warn');
  }));

  applyRoute('lab');
  requestAnimationFrame(frame);
})();
