/**
 * The two global versioning switches, read once and shared by the editor, the backtester
 * and Settings. A refused read (demo sandbox) leaves both off, which hides the feature.
 */
import { versioningApi } from './api.js';

class VersioningState {
  editor = $state(false);
  strategies = $state(false);
  #loading = null;

  /** Load once; later callers share the same request. */
  load() {
    this.#loading ??= this.refresh();
    return this.#loading;
  }

  async refresh() {
    try {
      this.apply(await versioningApi.settings());
    } catch {
      this.editor = false;
      this.strategies = false;
    }
  }

  apply(r) {
    this.editor = !!r.editor;
    this.strategies = !!r.strategies;
  }
}

export const versioning = new VersioningState();

/** Short local date + time for a version row. */
export const fmtVersionDate = (iso) =>
  iso ? new Date(iso).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : '';
