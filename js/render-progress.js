(() => {
  const form = document.getElementById('video-form');
  if (form) {
    const status = document.getElementById('submission-status');
    const button = form.querySelector('[type="submit"]');
    form.addEventListener('submit', async event => {
      if (event.defaultPrevented) return;
      event.preventDefault();
      button.disabled = true;
      status.textContent = 'Saving project…';
      try {
        const response = await fetch(form.action, { method: 'POST', body: new FormData(form) });
        if (!response.ok) throw new Error(`Submission failed (${response.status}).`);
        const { id } = await response.json();
        window.location.assign(`/production?id=${encodeURIComponent(id)}`);
      } catch (error) {
        status.textContent = error.message || 'Could not save the project.';
        button.disabled = false;
      }
    });
  }
  const page = document.getElementById('production-page');
  if (!page) return;
  const id = page.dataset.projectId;
  const message = document.getElementById('production-message');
  const state = document.getElementById('production-state');
  const heading = document.getElementById('production-heading');
  const step = document.getElementById('production-step-state');
  const icon = document.getElementById('production-step-icon');
  const bar = document.getElementById('production-bar');
  const retry = document.getElementById('retry-render');
  const cancel = document.getElementById('cancel-render');
  let polling = false;
  function show(stateName, text) {
    const running = stateName === 'running';
    const success = stateName === 'succeeded';
    const cancelled = stateName === 'cancelled';
    message.textContent = text;
    state.textContent = running ? 'In progress' : success ? 'Completed' : cancelled ? 'Cancelled' : 'Failed';
    heading.textContent = running ? 'Creating your video' : success ? 'Video production complete' : 'Production stopped';
    step.textContent = state.textContent;
    icon.textContent = success ? '✓' : running ? '◌' : '!';
    icon.className = `production-step-icon ${success ? 'production-step-complete' : running ? 'production-step-active' : 'production-step-failed'}`;
    bar.hidden = !running;
    retry.hidden = running || success;
    retry.disabled = polling;
    cancel.hidden = !running;
  }
  async function poll() {
    if (polling) return;
    polling = true;
    try {
      while (true) {
        const response = await fetch(`/render-status?id=${encodeURIComponent(id)}`, { cache: 'no-store' });
        if (!response.ok) throw new Error('Production status is unavailable. You can retry the render.');
        const progress = await response.json();
        show(progress.state, progress.message);
        if (progress.state === 'succeeded') { window.location.assign(`/preview?id=${encodeURIComponent(id)}`); return; }
        if (progress.state !== 'running') break;
        await new Promise(resolve => setTimeout(resolve, 1500));
      }
    } catch (error) { show('failed', error.message || 'Could not check production status.'); }
    finally { polling = false; retry.disabled = false; }
  }
  cancel.addEventListener('click', async () => {
    cancel.disabled = true;
    try {
      const response = await fetch('/cancel-render', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id }) });
      if (!response.ok) throw new Error('Could not cancel production.');
      const progress = await response.json();
      show(progress.state, progress.message);
    } catch (error) { message.textContent = error.message; }
    finally { cancel.disabled = false; }
  });
  retry.addEventListener('click', async () => {
    retry.disabled = true;
    try {
      const response = await fetch('/retry-render', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id }) });
      if (!response.ok) throw new Error(`Could not retry production (${response.status}).`);
      show('running', 'Preparing video production…');
      await poll();
    } catch (error) { show('failed', error.message); }
    finally { retry.disabled = false; }
  });
  poll();
})();
