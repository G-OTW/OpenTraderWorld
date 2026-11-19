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

/**
 * Resolve a pasted video URL into an embeddable reference.
 * The core parses the provider and id, looks the video up through the provider's oEmbed
 * endpoint and stores its thumbnail as a normal upload, so the document keeps a local
 * poster and stays readable offline. Returns
 * `{ provider, video_id, url, title, poster }`. `poster` is null when the provider had
 * no thumbnail to give.
 */
export async function resolveVideo(url) {
  const res = await fetch('/api/video-embed', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ url })
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `video lookup failed (${res.status})`);
  return body;
}
