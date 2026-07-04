// explorable.js — wires up the "click to reveal detail" pattern.
//
// For each `.explorable` container:
//   1. Find all elements with `data-reveals="some-id"` (the clickable cells)
//   2. Find the first `.detail-panel` inside (where content gets shown)
//   3. On click of any cell:
//      - Hide the empty prompt
//      - Find the `<template id="some-id">` (must be inside the container)
//      - Clone its content into the panel's `.detail-content`
//      - Mark the clicked cell as active (CSS handles the outline)
//
// Multiple `.explorable` containers on one page each get independent
// state — clicking a cell in one container only affects that
// container's panel.
//
// No dependencies. Vanilla JS. ~25 lines.

(function () {
  function wireContainer(container) {
    const cells = container.querySelectorAll('[data-reveals]');
    if (cells.length === 0) return;

    const panel = container.querySelector('.detail-panel');
    if (!panel) return;

    const empty = panel.querySelector('.detail-empty');
    const content = panel.querySelector('.detail-content');
    if (!content) return;

    // --- Toggle button (show all / collapse) ---
    const toggle = document.createElement('button');
    toggle.className = 'detail-toggle';
    toggle.textContent = 'show all';
    toggle.type = 'button';
    panel.insertBefore(toggle, panel.firstChild);

    function collapseAll() {
      panel.classList.remove('show-all');
      content.innerHTML = '';
      content.classList.remove('visible');
      if (empty) empty.style.display = '';
      toggle.textContent = 'show all';
      cells.forEach((c) => c.classList.remove('active'));
    }

    function expandAll() {
      panel.classList.add('show-all');
      content.innerHTML = '';
      const templates = container.querySelectorAll('template');
      templates.forEach((tpl) => {
        const item = document.createElement('div');
        item.className = 'detail-item';
        item.appendChild(tpl.content.cloneNode(true));
        content.appendChild(item);
      });
      if (empty) empty.style.display = 'none';
      content.classList.add('visible');
      toggle.textContent = 'collapse';
      cells.forEach((c) => c.classList.remove('active'));
    }

    toggle.addEventListener('click', () => {
      if (panel.classList.contains('show-all')) {
        collapseAll();
      } else {
        expandAll();
      }
    });

    // --- Cell click (only meaningful when not in show-all mode) ---
    cells.forEach((cell) => {
      cell.addEventListener('click', () => {
        // If currently in show-all mode, collapse first then show this one
        if (panel.classList.contains('show-all')) {
          collapseAll();
        }

        const id = cell.dataset.reveals;
        const tpl = container.querySelector('template#' + CSS.escape(id));
        if (!tpl) return;

        content.innerHTML = '';
        content.appendChild(tpl.content.cloneNode(true));

        if (empty) empty.style.display = 'none';
        content.classList.add('visible');

        cells.forEach((c) => c.classList.remove('active'));
        cell.classList.add('active');
      });
    });
  }

  function init() {
    document.querySelectorAll('.explorable').forEach(wireContainer);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
