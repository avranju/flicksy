(() => {
  const panels = document.getElementById('credits-panels');
  const add = document.getElementById('add-credits-panel');
  if (!panels || !add) return;

  let nextId = 2;
  function updatePanels() {
    const items = [...panels.children];
    items.forEach((panel, index) => {
      panel.querySelector('label').textContent = `Panel ${index + 1}`;
      const remove = panel.querySelector('.credits-panel-remove');
      remove.hidden = items.length === 1;
      remove.setAttribute('aria-label', `Remove panel ${index + 1}`);
    });
  }

  add.addEventListener('click', () => {
    const panel = panels.firstElementChild.cloneNode(true);
    const input = panel.querySelector('textarea');
    input.id = `credits-panel-${nextId++}`;
    input.value = '';
    panel.querySelector('label').htmlFor = input.id;
    panels.append(panel);
    updatePanels();
    input.focus();
  });
  panels.addEventListener('click', event => {
    const remove = event.target.closest('.credits-panel-remove');
    if (!remove || panels.children.length <= 1) return;
    const panel = remove.closest('.credits-panel');
    const next = panel.nextElementSibling || panel.previousElementSibling;
    panel.remove();
    updatePanels();
    next.querySelector('textarea').focus();
  });
})();
