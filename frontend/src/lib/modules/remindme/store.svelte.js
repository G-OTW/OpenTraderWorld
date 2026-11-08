/**
 * Shared notification state for the global chrome.
 *
 * New notifications arrive on the push stream (`/api/notifications/stream`) the moment their
 * row is inserted, and surface as transient toasts (the slide-in bandeau) with the unread
 * count for the topbar bell. The unread poll stays as the reconciliation path: once on start,
 * each time the stream (re)connects or says it may have missed something, and every five
 * minutes. `since` advances and ids are remembered, so each notification toasts once
 * whichever path brings it first.
 */
import { remindApi } from './api.js';
import { liveStream } from '$lib/live/stream.js';
import { ringGate, takeFresh } from './notifMerge.js';
import {
  armAlertSound,
  getAlertSound,
  isAlertNotification,
  playAlertSound
} from '$lib/notifications/alertSound.js';

const POLL_MS = 5 * 60_000;
const TOAST_MS = 8_000;

let unread = $state(0);
let toasts = $state([]); // [{ id, name, kind, linked_id, details, created_at }]
let since = null; // RFC3339 of the newest notification we've already toasted
let timer = null;
let stream = null;
let started = false;
const seen = new Set(); // ids already toasted, by either path
const ring = ringGate();
const listeners = new Set(); // fn(notification) for each fresh one

export const notifStore = {
  get unread() {
    return unread;
  },
  get toasts() {
    return toasts;
  },

  /** Start the stream and the fallback poll (idempotent: safe to call from the global layout's onMount). */
  start() {
    if (started) return;
    started = true;
    armAlertSound();
    // Seed `since` to "now" so we don't toast a backlog of old notifications on load,
    // but still light up the bell with the current unread count.
    since = new Date().toISOString();
    this.refreshCount();
    this.poll();
    timer = setInterval(() => this.poll(), POLL_MS);
    stream = liveStream({
      url: () => remindApi.streamUrl(),
      events: {
        notification: (d) => {
          if (typeof d?.unread === 'number') unread = d.unread;
          if (d?.notification) this.take([d.notification]);
        },
        // The server's listener reconnected or this tab fell behind: read the table.
        resync: () => this.poll()
      },
      // Whatever was inserted while the stream was down only the poll can bring back.
      onstate: (s) => {
        if (s === 'open') this.poll();
      }
    });
  },

  stop() {
    if (timer) clearInterval(timer);
    timer = null;
    stream?.close();
    stream = null;
    started = false;
  },

  /** Fetch just the unread count (used after acknowledging). */
  async refreshCount() {
    try {
      const { unread: u } = await remindApi.unreadSince(undefined);
      unread = u;
    } catch {
      /* offline: leave count as-is */
    }
  },

  /** Poll for notifications newer than `since`; toast the ones not seen yet and set the count. */
  async poll() {
    try {
      const { notifications, unread: u } = await remindApi.unreadSince(since);
      unread = u;
      this.take(notifications);
    } catch {
      /* offline: try again next tick */
    }
  },

  /** Toast the rows not toasted yet (from either path) and tell the listeners. */
  take(rows) {
    const { fresh, since: next } = takeFresh(rows, seen, since);
    since = next;
    for (const n of fresh) {
      this.pushToast(n);
      for (const fn of listeners) {
        try {
          fn(n);
        } catch {
          /* a listener's failure must not stop the toasts */
        }
      }
    }
    // Once per batch: alerts fired together ring together, not over each other.
    if (fresh.some(isAlertNotification) && ring()) playAlertSound(getAlertSound());
  },

  /** Call `fn(notification)` for each new notification; returns the unsubscribe. */
  onNotification(fn) {
    listeners.add(fn);
    return () => listeners.delete(fn);
  },

  pushToast(n) {
    toasts = [...toasts, n];
    setTimeout(() => this.dismiss(n.id), TOAST_MS);
  },

  dismiss(id) {
    toasts = toasts.filter((t) => t.id !== id);
  },

  /** Acknowledge everything (clears the bell pastille). */
  async ackAll() {
    await remindApi.ackAll();
    unread = 0;
    toasts = [];
  },

  /** Delete everything (the bell and the toasts go with it). */
  async clearAll() {
    await remindApi.clearAll();
    unread = 0;
    toasts = [];
  },

  /** Reflect a single notification being marked read elsewhere (e.g. the page). */
  noteRead() {
    if (unread > 0) unread -= 1;
  }
};
