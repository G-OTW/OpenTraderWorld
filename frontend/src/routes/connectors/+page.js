import { redirect } from '@sveltejs/kit';

// The data broker lives in Settings now: the old page opens its section there.
export function load() {
  redirect(307, '/settings#connectors');
}
