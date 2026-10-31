(() => {
  const title = document.getElementById('song-title');
  if (!title) return;
  function validate() {
    title.setCustomValidity(title.value.trim() ? '' : 'Enter a song title.');
  }
  title.addEventListener('input', validate);
  title.addEventListener('invalid', validate);
  title.form?.addEventListener('submit', event => {
    validate();
    if (!title.reportValidity()) event.preventDefault();
  });
})();
