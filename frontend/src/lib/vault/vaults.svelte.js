/**
 * One vault list shared by every VaultPicker on the page, so a vault created from one
 * picker shows up in all the others (the other secrets of a form, the other cards)
 * without a page reload.
 */
import { vaultApi } from '$lib/vault/api.js';

export const vaultStore = $state({ vaults: [], loaded: false });

let inflight = null;
// Bumped by every write to the store: a fetch that started before a fresher list was set
// is dropped instead of overwriting it.
let seq = 0;

/** Refetch the list. Pickers mounting together share one request. On failure the list
 *  already shown is kept. */
export function refreshVaults() {
  if (inflight) return inflight;
  const mine = ++seq;
  inflight = vaultApi
    .list()
    .catch(() => null)
    .then((list) => {
      inflight = null;
      if (mine === seq) {
        if (list) vaultStore.vaults = list;
        vaultStore.loaded = true;
      }
      return vaultStore.vaults;
    });
  return inflight;
}

/** Publish a list fetched elsewhere (after a vault save) to every picker. */
export function setVaults(list) {
  seq++;
  vaultStore.vaults = list;
  vaultStore.loaded = true;
}
