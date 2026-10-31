(() => {
  const button = document.getElementById('projects-button');
  const dialog = document.getElementById('projects-dialog');
  const list = document.getElementById('projects-list');
  const close = document.getElementById('projects-close');

  if (!button || !dialog || !list || !close) return;

  button.addEventListener('click', async () => {
    dialog.showModal();
    list.textContent = 'Loading projects…';
    try {
      const response = await fetch('/projects', { cache: 'no-store' });
      if (!response.ok) throw new Error('Could not load projects. Please try again.');
      const projects = await response.json();
      list.replaceChildren();
      if (!projects.length) {
        list.textContent = 'No projects yet. Create a video to save your first project.';
        return;
      }
      for (const project of projects) {
        const link = document.createElement('a');
        link.href = `/?id=${encodeURIComponent(project.id)}`;
        link.textContent = project.title;
        link.className = 'project-link';
        list.append(link);
      }
    } catch {
      list.textContent = 'Could not load projects. Please try again.';
    }
  });
  close.addEventListener('click', () => dialog.close());
  dialog.addEventListener('click', event => {
    if (event.target === dialog) dialog.close();
  });
})();
