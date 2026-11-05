/**
 * What every sign-in does once the server has opened a session, whatever proved it
 * (password, social account, recovery code). Shared by the login page and the social
 * sign-in callback.
 */
import { goto } from '$app/navigation';
import { ensureInstalled } from '$lib/modules/installed.js';
import { notifStore } from '$lib/modules/remindme/store.svelte.js';

/** Same-origin paths only: `//host` and `/\host` would leave the app (browsers read `\` as `/`). */
export function sameOriginPath(next) {
  const ok = typeof next === 'string' && next.startsWith('/') && !next.startsWith('//') && !next.includes('\\');
  return ok ? next : '/';
}

/**
 * Enter the app. `mustChange` sends a bootstrap account to the change-password screen first.
 */
export async function enterApp(next, { mustChange = false, username = '' } = {}) {
  if (mustChange) {
    await goto(`/change-password?u=${encodeURIComponent(username)}`);
    return;
  }
  // The root layout mounted while unauthenticated, so its initial installed-set fetch got a
  // 401 and bailed. Now that there is a session, (re)load it before navigating so the
  // dashboard and switcher render without a manual refresh.
  await ensureInstalled(true).catch(() => {});
  notifStore.start();
  await goto(sameOriginPath(next));
}
