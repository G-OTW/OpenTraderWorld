/**
 * Installed-modules store — which feature modules are available (browsable/usable).
 *
 * The backend tracks the set of installed ids in `app_settings`; the registry holds each
 * module's name/icon/description. The switcher and dashboard show only installed modules;
 * the Settings → Modules section installs/detaches them.
 *
 * `dashboard` (home) is core and always present regardless of this set.
 */
import { writable, derived, get } from 'svelte/store';
import { redirectIfUnauthorized } from '$lib/auth.js';

/** Set of installed module ids. `null` until first load. */
export const installedIds = writable(null);

async function req(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...options
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

/** Load the installed set from the backend (idempotent; safe to call repeatedly). */
export async function loadInstalled() {
  const body = await req('/settings/modules');
  installedIds.set(new Set(body.installed ?? []));
  return get(installedIds);
}

/** Load once; subsequent calls reuse the cached set unless `force` is set. */
let loaded = false;
export async function ensureInstalled(force = false) {
  if (loaded && !force) return get(installedIds);
  loaded = true;
  return loadInstalled();
}

/** Make a module available. Returns the new installed set. */
export async function installModule(id) {
  const body = await req('/settings/modules/install', {
    method: 'POST',
    body: JSON.stringify({ module: id })
  });
  installedIds.set(new Set(body.installed ?? []));
  return get(installedIds);
}

/** Hide a module; optionally wipe its data. Returns the new installed set. */
export async function detachModule(id, wipeData = false) {
  const body = await req('/settings/modules/detach', {
    method: 'POST',
    body: JSON.stringify({ module: id, wipe_data: wipeData })
  });
  installedIds.set(new Set(body.installed ?? []));
  return get(installedIds);
}

/** True once the installed set has loaded. */
export const installedReady = derived(installedIds, (s) => s !== null);
