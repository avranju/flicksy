(() => {
  const input = document.getElementById('images');
  const zone = document.getElementById('images-dropzone');
  const list = document.getElementById('images-list');
  const status = document.getElementById('images-status');
  if (!input || !zone || !list || !status) return;

  let selected = [];
  const saved = [...list.querySelectorAll('[data-saved-number]')];
  let nextNumber = Math.max(Number(list.dataset.imageHighWater) || 0, ...saved.map(item => Number(item.dataset.savedNumber))) + 1;
  function updateRequired() { input.required = !list.querySelector('[data-saved-number]'); }
  for (const item of saved) {
    item.querySelector('button').addEventListener('click', () => {
      const next = item.nextElementSibling?.querySelector('button') || item.previousElementSibling?.querySelector('button') || input;
      item.remove();
      updateRequired();
      next.focus();
    });
  }
  const supported = file => /\.(jpe?g|png|webp)$/i.test(file.name);

  function syncInput() {
    const transfer = new DataTransfer();
    selected.forEach(entry => transfer.items.add(entry.file));
    input.files = transfer.files;
  }

  function addFiles(files) {
    const incoming = Array.from(files);
    const valid = incoming.filter(file => supported(file) && file.size > 0);
    for (const file of valid) {
      const url = URL.createObjectURL(file);
      const item = document.createElement('li');
      item.className = 'image-tile';
      const image = document.createElement('img');
      image.src = url;
      image.alt = '';
      image.addEventListener('error', () => {
        image.hidden = true;
        item.classList.add('image-tile-error');
      });
      const number = document.createElement('span');
      number.className = 'image-tile-number';
      const imageNumber = nextNumber++;
      number.textContent = `Image ${imageNumber}`;
      const numberInput = document.createElement('input');
      numberInput.type = 'hidden';
      numberInput.name = 'image-number';
      numberInput.value = imageNumber;
      const filename = document.createElement('span');
      filename.className = 'image-tile-name';
      filename.textContent = file.name;
      const remove = document.createElement('button');
      remove.type = 'button';
      remove.className = 'image-tile-remove';
      remove.setAttribute('aria-label', `Remove ${file.name}`);
      remove.textContent = '×';
      const entry = { file, url, item };
      remove.addEventListener('click', () => {
        const nextFocus = item.nextElementSibling?.querySelector('button') ||
          item.previousElementSibling?.querySelector('button') || input;
        selected = selected.filter(candidate => candidate !== entry);
        item.remove();
        URL.revokeObjectURL(url);
        syncInput();
        nextFocus.focus();
      });
      item.append(image, number, filename, remove, numberInput);
      list.append(item);
      selected.push(entry);
    }
    syncInput();
    status.textContent = valid.length === incoming.length ? '' :
      'Only non-empty JPG, PNG, and WebP files can be added.';
  }
  window.addEventListener('pagehide', event => {
    if (!event.persisted) selected.forEach(entry => URL.revokeObjectURL(entry.url));
  });
  input.addEventListener('change', () => addFiles(input.files));
  for (const event of ['dragenter', 'dragover']) {
    zone.addEventListener(event, e => {
      e.preventDefault();
      zone.classList.add('song-dragging');
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    });
  }
  zone.addEventListener('dragleave', e => {
    if (!zone.contains(e.relatedTarget)) zone.classList.remove('song-dragging');
  });
  zone.addEventListener('drop', e => {
    e.preventDefault();
    zone.classList.remove('song-dragging');
    if (e.dataTransfer?.files.length) addFiles(e.dataTransfer.files);
  });
})();
