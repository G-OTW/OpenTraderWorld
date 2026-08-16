/** Upload helper — posts a File to the core and returns its served URL. */
import { redirectIfUnauthorized } from '$lib/auth.js';

export async function uploadFile(file) {
  const form = new FormData();
  form.append('file', file);
  const res = await fetch('/api/files', { method: 'POST', body: form });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `upload failed (${res.status})`);
  return body; // { id, url, filename, content_type, size }
}

/** Open a file picker and resolve with the chosen File (or null if cancelled). */
export function pickFile(accept = 'image/*') {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = accept;
    input.onchange = () => resolve(input.files?.[0] ?? null);
    // If the dialog is dismissed there's no reliable event; rely on change only.
    input.click();
  });
}
