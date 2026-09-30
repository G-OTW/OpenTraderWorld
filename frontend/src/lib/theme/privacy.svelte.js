// Privacy screen: one global switch that masks the figures in every dashboard widget.
// Client-only (localStorage), no backend: it hides data on screen, it doesn't restrict
// access. The widgets keep rendering, so nothing reloads when the eye is flipped back.
const KEY = 'otw.privacy.hidden';

function read() {
  try {
    return localStorage.getItem(KEY) === '1';
  } catch {
    return false; // storage blocked - start visible
  }
}

export const privacy = $state({ hidden: read() });

export function togglePrivacy() {
  privacy.hidden = !privacy.hidden;
  try {
    localStorage.setItem(KEY, privacy.hidden ? '1' : '0');
  } catch {
    /* storage blocked - the choice just doesn't survive a reload */
  }
}
