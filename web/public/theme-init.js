// Applies the saved theme before first paint so there's no flash. External
// file (not inline) so it works under the CSP's script-src 'self'.
try {
  const t = localStorage.getItem('kanban.theme')
  if (t === 'light' || t === 'dark') document.documentElement.dataset.theme = t
} catch {}
