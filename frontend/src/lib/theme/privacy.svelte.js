// Privacy screen: one global switch that masks the figures in every dashboard widget.
// A user preference (`$lib/prefs.svelte.js`, mirrored to localStorage as '1' / '0'): it
// hides data on screen, it doesn't restrict access. The widgets keep rendering, so nothing reloads when the eye is flipped back.
import { getPref, onPref, setPref } from '$lib/prefs.svelte.js';

const KEY = 'otw.privacy.hidden';

const read = () => getPref(KEY, false) === true;

export const privacy = $state({ hidden: read() });

// The server's choice arrives after first paint (another browser may have flipped it).
onPref(KEY, () => {
  privacy.hidden = read();
});

export function togglePrivacy() {
  privacy.hidden = !privacy.hidden;
  setPref(KEY, privacy.hidden);
}
