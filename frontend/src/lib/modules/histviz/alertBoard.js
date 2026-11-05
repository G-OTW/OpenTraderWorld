// The alerts every chart on screen draws from (see alertMarks.js for what each one shows).
import { readable } from 'svelte/store';
import { histvizApi } from './api.js';

/** Long enough to stay cheap, short enough that a fired alert soon leaves the scale. */
const POLL_MS = 30000;

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

/** Every alert, one list for all the panes, polled while a chart is on screen. */
export const alertBoard = readable([], (set) => {
  publish = set;
  refreshAlertBoard();
  const timer = setInterval(refreshAlertBoard, POLL_MS);
  return () => {
    clearInterval(timer);
    publish = null;
  };
});
