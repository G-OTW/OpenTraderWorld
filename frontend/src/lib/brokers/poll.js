/** How often an open account view re-reads its money. IB recomputes its figures on its own
 *  cycle (balances about every three minutes, positions and P&L faster), a paper account on
 *  every fill and mark: five seconds keeps both close to live without loading the socket. */
export const ACCOUNT_MS = 5000;

/** Run `read` now, then every `ms` while the page is visible: a hidden tab stops reading and
 *  reads again the moment it shows. One read at a time, the next one `ms` after the last ends.
 *  Returns the stop function, for an `$effect` cleanup. */
export function poll(read, ms = ACCOUNT_MS) {
  let timer = null;
  let busy = false;
  let stopped = false;
  async function tick() {
    clearTimeout(timer);
    if (stopped || busy || document.hidden) return;
    busy = true;
    try {
      await read();
    } finally {
      busy = false;
    }
    if (!stopped) timer = setTimeout(tick, ms);
  }
  const visibility = () => !document.hidden && tick();
  document.addEventListener('visibilitychange', visibility);
  tick();
  return () => {
    stopped = true;
    clearTimeout(timer);
    document.removeEventListener('visibilitychange', visibility);
  };
}
