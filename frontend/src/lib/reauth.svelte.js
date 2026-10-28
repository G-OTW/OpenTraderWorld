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
 *
 * The same prompt answers `trading_locked`: with the trading lock on, an order route refuses
 * until this session has opened trading with an authenticator code. The prompt then asks for
 * the code alone and posts it to the unlock route instead.
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
export function promptReauth(once = false, unlock = false) {
  if (request) return request.promise;
  let settle;
  const promise = new Promise((resolve) => (settle = resolve));
  request = {
    promise,
    /** Single-use: the password unlocks the one refused change, not the next few minutes
     *  (`reauth_once_required`, e.g. turning trading on for a broker account). */
    once,
    /** True when the prompt opens trading for this session (`trading_locked`) rather than
     *  re-entering the password. */
    unlock,
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
  if (status !== 403) return false;
  if (body?.code === 'reauth_required') return await promptReauth();
  if (body?.code === 'reauth_once_required') return await promptReauth(true);
  if (body?.code === 'trading_locked') return await promptReauth(false, true);
  return false;
}
