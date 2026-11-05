/**
 * What an automatic refresh left alone to spare API quotas, as one sentence: who, why,
 * when it will be asked again, and that Refresh asks now.
 * `tr` is the `$t` translator; `deferred` the API's `[{ label, reason, error, failed_at, retry_at }]`.
 */
const when = (ms) => new Date(ms).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' });
const clip = (s) => (s.length > 160 ? `${s.slice(0, 157)}...` : s);

export function deferredText(tr, deferred = []) {
  const parts = deferred.map((d) =>
    d.reason === 'quota'
      ? tr('fundamentals.deferred.quota', { provider: d.label, next: when(d.retry_at) })
      : tr('fundamentals.deferred.failed', { provider: d.label, when: when(d.failed_at), error: clip(d.error), next: when(d.retry_at) })
  );
  return [tr('fundamentals.deferred.lead'), ...parts, tr('fundamentals.deferred.retry')].join(' ');
}

/** The text to show for a failed load: the deferral when that is what it was. */
export const errorText = (tr, e) => (e?.deferred ? deferredText(tr, e.deferred) : (e?.message ?? String(e)));
