/**
 * Timers pinned to the top bar. The pinned ids and the chip's label mode are user
 * preferences; the project list is a live copy shared by the top-bar chip and the Time
 * page, so a start or stop on either side shows on the other without a reload.
 */
import { getPref, onPref, setPref } from '$lib/prefs.svelte.js';
import { timeApi } from './api.js';

const IDS_KEY = 'otw.time.pinned';
const LABEL_KEY = 'otw.time.pinLabel'; // 'name' | 'value'

function readIds() {
  const v = getPref(IDS_KEY, []);
  return Array.isArray(v) ? v.filter((x) => typeof x === 'string') : [];
}

export const timerPins = $state({
  ids: readIds(),
  /** Every timer, as last loaded (null until the first load). */
  projects: null,
  /** Client time of that load: a running timer ticks forward from it. */
  loadedAt: Date.now(),
  /** The chip shows the elapsed time instead of the name. */
  showValue: getPref(LABEL_KEY, 'name') === 'value',
  /** Bumped when the top bar starts or stops a timer, so an open Time page reloads. */
  rev: 0
});

onPref(IDS_KEY, () => {
  timerPins.ids = readIds();
});
onPref(LABEL_KEY, (v) => {
  timerPins.showValue = v === 'value';
});

export const isPinned = (id) => timerPins.ids.includes(id);

export function togglePin(id) {
  const ids = isPinned(id) ? timerPins.ids.filter((x) => x !== id) : [...timerPins.ids, id];
  timerPins.ids = ids;
  setPref(IDS_KEY, ids);
}

export function toggleLabel() {
  timerPins.showValue = !timerPins.showValue;
  setPref(LABEL_KEY, timerPins.showValue ? 'value' : 'name');
}

/** Adopts a freshly loaded project list and drops pins whose timer was deleted. */
export function syncProjects(projects, loadedAt = Date.now()) {
  timerPins.projects = projects;
  timerPins.loadedAt = loadedAt;
  const known = new Set(projects.map((p) => p.id));
  const kept = timerPins.ids.filter((id) => known.has(id));
  if (kept.length !== timerPins.ids.length) {
    timerPins.ids = kept;
    setPref(IDS_KEY, kept);
  }
}

export async function refreshPins() {
  if (!timerPins.ids.length) return;
  syncProjects(await timeApi.listProjects());
}

/** Starts or stops a timer from the top bar. */
export async function toggleTimer(p) {
  if (p.running) await timeApi.stop(p.id);
  else await timeApi.start(p.id);
  await refreshPins();
  timerPins.rev += 1;
}

/** Elapsed seconds, ticking forward from the last load while the timer runs. */
export function liveSeconds(p, now) {
  if (!p.running) return p.tracked_seconds;
  return p.tracked_seconds + Math.max(now - timerPins.loadedAt, 0) / 1000;
}
