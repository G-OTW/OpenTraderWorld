/**
 * Doc submission client. Sends a document from the editor to the review pipeline for
 * publication. The frontend only talks to its own backend (`/api/community-docs/submit`);
 * otw-core relays to the review site and adds the secret token server-side — no secret is
 * ever shipped in this bundle.
 */
import { redirectIfUnauthorized } from '$lib/auth.js';

/**
 * Submit a doc for editorial review.
 * @param {object} doc
 * @param {string} doc.title
 * @param {string|null} [doc.icon]
 * @param {string} [doc.layout]
 * @param {string} doc.html            rendered HTML (faithful body)
 * @param {object|null} doc.source_json ProseMirror JSON (re-editable source)
 * @param {string} doc.language        ISO language code, e.g. "en"
 * @param {string[]} doc.categories
 * @param {string|null} [doc.author_name]
 * @param {string|null} [doc.author_email]
 */
export async function submitDoc(doc) {
  const res = await fetch('/api/community-docs/submit', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(doc)
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `submission failed (${res.status})`);
  return body;
}
