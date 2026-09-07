/**
 * App-wide toast queue — transient status messages (saved, error, warning).
 *
 * Distinct from the reminder ToastBandeau (that's the RemindMe module's timed
 * notifications). This is the generic UI feedback channel: call toast.ok('Saved'),
 * toast.err(msg), toast.warn(msg). Render <ToastHost/> once at the app root.
 *
 * Currently unused. Errors are shown where they happen, by <ErrorText> — a toast that
 * disappears is the wrong home for a message the user has to act on. What remains for
 * this queue is *success* feedback ("Saved"), which today only AdminSection has. Adopting
 * it app-wide adds notifications where there are none, so it is a product decision rather
 * than part of the consistency sweep, and is deliberately left for one.
 */

let seq = 0;

class ToastStore {
  items = $state([]); // { id, kind: 'ok'|'err'|'warn', text }

  push(kind, text, ttl = 3200) {
    const id = ++seq;
    this.items = [...this.items, { id, kind, text }];
    if (ttl > 0) setTimeout(() => this.dismiss(id), ttl);
    return id;
  }
  ok(text, ttl) {
    return this.push('ok', text, ttl);
  }
  err(text, ttl = 6000) {
    return this.push('err', text, ttl);
  }
  warn(text, ttl) {
    return this.push('warn', text, ttl);
  }
  dismiss(id) {
    this.items = this.items.filter((t) => t.id !== id);
  }
}

export const toast = new ToastStore();
