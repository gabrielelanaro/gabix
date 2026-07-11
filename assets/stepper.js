(function () {
  function wire(stepper) {
    const steps = Array.from(stepper.querySelectorAll('[data-step]'));
    const indexLabel = stepper.querySelector('[data-step-index]');
    const previous = stepper.querySelector('[data-step-previous]');
    const next = stepper.querySelector('[data-step-next]');
    if (!steps.length || !indexLabel || !previous || !next) return;
    let index = 0;
    function draw() {
      steps.forEach((step, i) => { step.hidden = i !== index; });
      indexLabel.textContent = String(index + 1);
      previous.disabled = index === 0;
      next.disabled = index === steps.length - 1;
    }
    previous.addEventListener('click', function () { if (index > 0) { index -= 1; draw(); } });
    next.addEventListener('click', function () { if (index + 1 < steps.length) { index += 1; draw(); } });
    draw();
  }
  function init() { document.querySelectorAll('[data-stepper]').forEach(wire); }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
}());
