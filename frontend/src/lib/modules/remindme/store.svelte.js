/**
 * Shared notification state for the global chrome.
 *
 * Polls the unread endpoint every minute (and once on start), tracking the unread count
 * for the topbar bell and surfacing freshly-created notifications as transient toasts
 * (the slide-in bandeau). `since` advances so each notification only toasts once.
 */
import { remindApi } from './api.js';

const POLL_MS = 60_000;
const TOAST_MS = 8_000;

let unread = $state(0);
let toasts = $state([]); // [{ id, name, kind, linked_id, details, created_at }]
let since = null; // RFC3339 of the newest notification we've already toasted
let timer = null;
let started = false;

export const notifStore = {
  get unread() {
    return unread;
  },
  get toasts() {
    return toasts;
  },

  /** Start polling (idempotent — safe to call from the global layout's onMount). */
  start() {
    if (started) return;
    started = true;
    // Seed `since` to "now" so we don't toast a backlog of old notifications on load,
    // but still light up the bell with the current unread count.
    since = new Date().toISOString();
    this.refreshCount();
    this.poll();
    timer = setInterval(() => this.poll(), POLL_MS);
  },

  stop() {
    if (timer) clearInterval(timer);
    timer = null;
    started = false;
  },

  /** Fetch just the unread count (used after acknowledging). */
  async refreshCount() {
    try {
      const { unread: u } = await remindApi.unreadSince(undefined);
      unread = u;
    } catch {
      /* offline — leave count as-is */
    }
  },

  /** Poll for notifications newer than `since`; toast them and bump the count. */
  async poll() {
    try {
      const { notifications, unread: u } = await remindApi.unreadSince(since);
      unread = u;
      for (const n of notifications) {
        this.pushToast(n);
        since = n.created_at; // results are ascending, so the last wins
      }
    } catch {
      /* offline — try again next tick */
    }
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

  /** Reflect a single notification being marked read elsewhere (e.g. the page). */
  noteRead() {
    if (unread > 0) unread -= 1;
  }
};
