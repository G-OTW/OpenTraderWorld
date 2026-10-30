// The step between "the user filled the form" and "the pull runs".
//
// An import modal hands its pull to `guard`: the server estimates how long the broker will
// take, and a short pull just runs. A long one raises the LongRunModal instead: wait here,
// or send it to the background, close the import modal, and be told when it is done (the
// server writes a notification; the watcher below makes it show at once rather than at
// the bell's next minute). Opening the import modal again takes the answer back.
import { tasksApi } from './api.js';
import { notifStore } from '$lib/modules/remindme/store.svelte.js';

const POLL_MS = 3000;

// ── App-wide watcher ─────────────────────────────────────────────────────────

let watched = new Set();
let timer = null;

async function tick() {
  let tasks = [];
  try {
    tasks = await tasksApi.list();
  } catch {
    return; // offline: try again next tick
  }
  const running = new Set(tasks.filter((t) => t.status === 'running').map((t) => t.id));
  const finished = [...watched].some((id) => !running.has(id));
  watched = new Set([...watched].filter((id) => running.has(id)));
  if (finished) notifStore.poll();
  if (!watched.size) {
    clearInterval(timer);
    timer = null;
  }
}

/** Follow a background task until it ends, then surface its notification right away. */
export function watch(task) {
  if (!task?.id) return;
  watched.add(task.id);
  if (!timer) timer = setInterval(tick, POLL_MS);
}

/** On load: keep following what was sent to the background before a reload. */
export async function watchRunning() {
  try {
    for (const t of await tasksApi.list()) if (t.status === 'running') watch(t);
  } catch {
    /* offline: the bell still catches the notification at its next poll */
  }
}

// ── Per modal ────────────────────────────────────────────────────────────────

export class LongRun {
  /** { seconds, run, background } while the user decides. */
  prompt = $state(null);
  /** The task running in the background for this modal's scope, if any. */
  running = $state(null);
  #timer = null;
  #opts;

  /**
   * scope(): the task scope this modal reads back (`journal:<id>`, `taxcalc`…).
   * onresult(task): a finished task's answer, taken. onerror(message). onsent(): the pull
   * went to the background, close the import modal.
   */
  constructor(opts) {
    this.#opts = opts;
  }

  /** On open: pick up what the background did for this scope while the modal was shut. */
  async attach() {
    this.detach();
    let tasks = [];
    try {
      tasks = await tasksApi.list(this.#opts.scope());
    } catch {
      return;
    }
    const task = tasks[0];
    if (!task) return;
    if (task.status === 'running') {
      this.running = task;
      this.#timer = setInterval(() => this.#poll(), POLL_MS);
    } else {
      await this.#take(task);
    }
  }

  detach() {
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = null;
    this.running = null;
    this.prompt = null;
  }

  async #poll() {
    if (!this.running) return;
    let tasks = [];
    try {
      tasks = await tasksApi.list(this.#opts.scope());
    } catch {
      return;
    }
    const task = tasks.find((t) => t.id === this.running.id);
    if (task?.status === 'running') return;
    clearInterval(this.#timer);
    this.#timer = null;
    this.running = null;
    if (task) await this.#take(task);
  }

  async #take(task) {
    try {
      const done = await tasksApi.take(task.id);
      if (done.status === 'failed') this.#opts.onerror?.(done.error);
      else this.#opts.onresult?.(done);
    } catch (e) {
      this.#opts.onerror?.(e.message);
    }
  }

  /** Stop the background pull this modal is waiting on. */
  async cancel() {
    if (!this.running) return;
    const id = this.running.id;
    this.detach();
    try {
      await tasksApi.cancel(id);
    } catch (e) {
      this.#opts.onerror?.(e.message);
    }
  }

  /**
   * Ask how long the pull would take; run it now when it is short, else ask the user.
   * `estimate` is the body of the estimate call; `run()` does the pull here; `background()`
   * starts it on the server and resolves to `{ task }`.
   */
  async guard(accountId, estimate, run, background) {
    let est = null;
    try {
      est = await tasksApi.estimate(accountId, estimate);
    } catch {
      // No estimate is no reason to refuse the pull: it runs as it always did.
    }
    if (!est?.long) return run();
    this.prompt = { seconds: est.seconds, run, background };
  }

  /** "Wait here": the pull runs in the modal, as a short one would. */
  wait() {
    const p = this.prompt;
    this.prompt = null;
    return p?.run();
  }

  /** "Run in background": start it on the server, then let the user go. */
  async sendToBackground() {
    const p = this.prompt;
    this.prompt = null;
    if (!p) return;
    try {
      const { task } = await p.background();
      watch(task);
      this.#opts.onsent?.(task);
    } catch (e) {
      this.#opts.onerror?.(e.message);
    }
  }

  dismiss() {
    this.prompt = null;
  }
}

/** "about 2 min", in the viewer's language. `t` is the resolved `$t`. */
export function fmtDuration(seconds, t) {
  const s = Math.max(1, Math.round(Number(seconds) || 0));
  if (s < 60) return t('tasks.duration.seconds', { n: s });
  const m = Math.round(s / 60);
  if (m < 60) return t('tasks.duration.minutes', { n: m });
  return t('tasks.duration.hours', { n: Math.round((m / 60) * 10) / 10 });
}
