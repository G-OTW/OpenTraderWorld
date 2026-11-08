// The alerts every chart on screen draws from (see alertMarks.js for what each one shows).
import { readable } from 'svelte/store';
import { histvizApi } from './api.js';
import { notifStore } from '$lib/modules/remindme/store.svelte.js';

/** Fallback only: a chart alert firing refreshes the board at once, through its notification. */
const POLL_MS = 5 * 60_000;

let publish = null;

/** Re-read the alerts: every chart on screen follows, whichever pane changed one. */
export function refreshAlertBoard() {
  return histvizApi
    .alerts()
    .then((rows) => publish?.(rows))
    .catch(() => {
      /* the marks are a convenience: a failure here must not break a chart */
    });
}

/** Every alert, one list for all the panes, read while a chart is on screen. */
export const alertBoard = readable([], (set) => {
  publish = set;
  refreshAlertBoard();
  const timer = setInterval(refreshAlertBoard, POLL_MS);
  // A fired chart alert changes its row (fired, disarmed): its mark leaves the scale now.
  const unsubscribe = notifStore.onNotification((n) => {
    if (n.kind === 'chart') refreshAlertBoard();
  });
  return () => {
    clearInterval(timer);
    unsubscribe();
    publish = null;
  };
});
