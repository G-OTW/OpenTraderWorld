/**
 * Step-up re-authentication: the queue behind the password prompt.
 *
 * The server refuses a handful of actions with `403 {code: "reauth_required"}` until the
 * session has re-entered its password: minting an MCP token, changing the network mode,
 * exporting the sealed credential stores, writing a vault item, turning the second factor
 * off. Rather than every screen growing its own prompt, the API wrappers call
 * [`promptReauth`] when they see that code and retry once.
 *
 * Only the queue lives here. The form that actually posts the password is `ReauthModal`,
 * mounted once in the root layout, which keeps this file free of any import from the API
 * clients that import it.
 *
 * The grant lasts a few minutes and belongs to the session, so a run of actions asks once.
 */

/** The prompt currently on screen, or null. Read by `ReauthModal`. */
let request = $state(null);

export function currentRequest() {
  return request;
}

/**
 * Raise the prompt and resolve once the user answers: true if the password was accepted,
 * false if they cancelled.
 *
 * A second caller arriving while a prompt is already up joins it rather than stacking
 * another, so two parallel requests refused at the same time ask the user once.
 */
export function promptReauth() {
  if (request) return request.promise;
  let settle;
  const promise = new Promise((resolve) => (settle = resolve));
  request = {
    promise,
    done(value) {
      request = null;
      settle(value);
    }
  };
  return promise;
}

/**
 * Wrap a fetch wrapper's error path: returns true when the caller should retry once.
 *
 * Kept here so `settings/api.js`, `vault/api.js` and any future client share one rule
 * instead of three copies of the same `if`.
 */
export async function shouldRetryAfterReauth(status, body) {
  if (status !== 403 || body?.code !== 'reauth_required') return false;
  return await promptReauth();
}
