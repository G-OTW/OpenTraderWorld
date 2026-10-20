/**
 * Hand a prompt to the floating assistant from elsewhere in the app (voice control).
 *
 * The assistant owns the conversation, the model and the write-consent prompt, so a caller
 * does not run the agent itself: it posts the text here and the mounted widget sends it as
 * if typed, then settles the promise when the run ends. With no widget mounted (the Agent
 * page hides it) the request is refused at once rather than left hanging.
 */

let mounted = 0;
let pending = $state(null);

export const assistantBus = {
  get request() {
    return pending;
  },

  /** Called by the widget while it is on screen; returns the unmount callback. */
  attach() {
    mounted++;
    return () => {
      mounted = Math.max(0, mounted - 1);
    };
  },

  /** Ask the assistant. Resolves when the run is over, rejects with the reason it failed. */
  ask(text) {
    if (!mounted) return Promise.reject(new Error('unavailable'));
    pending?.reject(new Error('superseded'));
    return new Promise((resolve, reject) => {
      pending = { text, resolve, reject };
    });
  },

  /** The widget takes the request it is about to handle. */
  take() {
    const p = pending;
    pending = null;
    return p;
  }
};
