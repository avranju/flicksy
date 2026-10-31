(() => {
  const key = 'flicksy-theme';
  const system = window.matchMedia('(prefers-color-scheme: dark)');
  let preference = 'system';
  try {
    const saved = localStorage.getItem(key);
    if (saved === 'light' || saved === 'dark') preference = saved;
  } catch { /* Use the system preference when storage is unavailable. */ }

  function apply() {
    document.documentElement.classList.toggle('dark', preference === 'dark' || (preference === 'system' && system.matches));
  }
  apply();
  system.addEventListener('change', apply);
  document.addEventListener('DOMContentLoaded', () => {
    const select = document.getElementById('theme-select');
    select.value = preference;
    select.addEventListener('change', () => {
      preference = select.value;
      try {
        if (preference === 'system') localStorage.removeItem(key);
        else localStorage.setItem(key, preference);
      } catch { /* The selection still applies for this page. */ }
      apply();
    });
  });
})();
