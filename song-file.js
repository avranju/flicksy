(() => {
  const input = document.getElementById('song-file');
  const zone = document.getElementById('song-dropzone');
  const name = document.getElementById('song-file-name');
  const status = document.getElementById('song-file-status');
  if (!input || !zone || !name || !status) return;

  const supported = file => /^(audio|video)\//i.test(file.type) ||
    /\.(mp3|wav|m4a|aac|flac|ogg|opus|aiff|aif|mp4|m4v|mov|webm|mkv|avi|mpeg|mpg)$/i.test(file.name);
  const show = () => {
    name.textContent = input.files[0]?.name || 'Choose an audio or video file';
  };
  input.addEventListener('change', () => {
    const file = input.files[0];
    if (file && (!supported(file) || file.size === 0)) {
      input.value = '';
      status.textContent = 'Choose a non-empty audio or video file.';
    } else {
      status.textContent = '';
    }
    show();
  });
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
    const files = e.dataTransfer?.files;
    if (!files || files.length !== 1 || (!supported(files[0]) || files[0].size === 0)) {
      status.textContent = 'Drop one non-empty audio or video file.';
      return;
    }
    input.files = files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
  });
})();
