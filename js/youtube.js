(() => {
  const page = document.getElementById('youtube-page');
  if (!page) return;
  const form = document.getElementById('youtube-upload-form');
  const status = document.getElementById('youtube-status');
  const link = document.getElementById('youtube-link');
  const button = form?.querySelector('button[type=submit]');
  const resume = document.createElement('button');
  resume.type = 'button';
  resume.textContent = 'Resume upload';
  resume.className = 'mt-4 rounded-xl border border-border px-4 py-2 font-semibold';
  resume.hidden = true;
  status.after(resume);
  const abandon = document.createElement('button');
  abandon.type = 'button';
  abandon.textContent = 'Abandon upload';
  abandon.className = 'ml-3 mt-4 rounded-xl border border-border px-4 py-2 font-semibold';
  abandon.hidden = true;
  resume.after(abandon);
  let current = null, timer = null;
  async function json(url, options) {
    const response = await fetch(url, options);
    if (!response.ok) {
      const message = await response.text();
      throw new Error(message && message.length < 300 ? message : `Request failed (${response.status}).`);
    }
    return response.json();
  }
  function show(job) {
    current = job;
    clearTimeout(timer);
    resume.hidden = job.state !== 'interrupted';
    resume.disabled = !form;
    abandon.hidden = job.state !== 'interrupted';
    if (button) button.disabled = ['uploading', 'queued', 'interrupted'].includes(job.state);
    link.hidden = true;
    if (job.state === 'complete') {
      status.textContent = `Uploaded to YouTube.${job.actual_privacy ? ` YouTube reported visibility: ${job.actual_privacy}.` : ' Check visibility in YouTube Studio.'} YouTube may still be processing the video.`;
      link.href = `https://www.youtube.com/watch?v=${encodeURIComponent(job.video_id)}`;
      link.hidden = false;
    } else if (job.state === 'interrupted') {
      status.textContent = job.error || 'Upload interrupted. Resume to check its status.';
    } else if (job.state === 'abandoned') {
      status.textContent = 'Upload abandoned. Check YouTube Studio before starting another upload.';
    } else {
      const percentage = job.total ? Math.min(100, Math.floor(job.sent * 100 / job.total)) : 0;
      status.textContent = `Uploading to YouTube… ${percentage}%`;
      timer = setTimeout(poll, 1500);
    }
  }
  async function poll() {
    try { show(await json(`/youtube/status?id=${encodeURIComponent(current.id)}`)); }
    catch { status.textContent = 'Unable to check upload status. Reload to try again.'; }
  }
  form?.addEventListener('submit', async event => {
    event.preventDefault();
    if (!form.reportValidity()) return;
    button.disabled = true;
    const values = new FormData(form);
    try {
      show(await json('/youtube/upload', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify({id:form.dataset.project,render:Number(form.dataset.render),title:values.get('title'),description:values.get('description'),privacy:values.get('privacy'),made_for_kids:values.get('made_for_kids') === 'true',certify:values.has('certify')}) }));
    } catch (error) { status.textContent = error.message; button.disabled = false; }
  });
  resume.addEventListener('click', async () => {
    resume.disabled = true;
    try { await json('/youtube/resume', {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({id:current.id})}); await poll(); }
    catch (error) { status.textContent = error.message; }
    finally { resume.disabled = false; }
  });
  abandon.addEventListener('click', async () => {
    if (!confirm('The video may already exist on YouTube. Check YouTube Studio before abandoning this upload. Continue?')) return;
    try { show(await json('/youtube/abandon', {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({id:current.id})})); }
    catch (error) { status.textContent = error.message; }
  });
  json(`/youtube/history?id=${encodeURIComponent(page.dataset.project)}&render=${encodeURIComponent(page.dataset.render)}`)
    .then(jobs => {
      const history = document.getElementById('youtube-history');
      const completed = jobs.filter(job => job.state === 'complete');
      if (completed.length) {
        const heading = document.createElement('p');
        heading.textContent = 'Previous YouTube uploads';
        heading.className = 'font-semibold';
        history.append(heading);
        for (const job of completed) {
          const item = document.createElement('a');
          item.href = `https://www.youtube.com/watch?v=${encodeURIComponent(job.video_id)}`;
          item.textContent = `Open video ${job.video_id}`;
          item.target = '_blank';
          item.rel = 'noopener noreferrer';
          item.className = 'block text-accent-ink underline';
          history.append(item);
        }
      }
      if (jobs.length) show(jobs[0]);
    })
    .catch(() => { status.textContent = 'Could not load previous uploads.'; });
})();
