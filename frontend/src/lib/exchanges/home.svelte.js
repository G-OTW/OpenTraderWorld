/**
 * The exchange picked in Settings → Defaults, and whether it is trading now.
 *
 * The server knows the hours, holidays and half days, so the status is asked, never computed
 * here. It is re-asked just after the next open or close the server announced (and every
 * five minutes at most), so the badge flips on time without polling every second.
 */
import { settingsApi } from '$lib/settings/api.js';

const MAX_MS = 5 * 60_000;
const RETRY_MS = 60_000;

class HomeExchange {
  /** {mic, code, name, open, next_change, holidays_unknown} or null when none is picked. */
  info = $state(null);
  #timer = null;

  async refresh() {
    clearTimeout(this.#timer);
    let wait = MAX_MS;
    try {
      const r = await settingsApi.exchangeHome();
      this.info = r.mic ? r : null;
      if (r.next_change) {
        const ms = new Date(r.next_change).getTime() - Date.now() + 2_000;
        wait = Math.min(Math.max(ms, 5_000), MAX_MS);
      }
    } catch {
      wait = RETRY_MS;
    }
    this.#timer = setTimeout(() => this.refresh(), wait);
  }
}

export const homeExchange = new HomeExchange();
