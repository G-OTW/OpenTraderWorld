/**
 * Shared auth helper for module API clients.
 *
 * The backend protects every /api route except the auth handshake; an expired or missing
 * session returns 401. Module fetch wrappers call `redirectIfUnauthorized(res)` so a stale
 * session bounces the user to the login screen from anywhere, not just on initial load.
 */
import { goto } from '$app/navigation';

export function redirectIfUnauthorized(res) {
  if (res.status === 401 && typeof window !== 'undefined' && window.location.pathname !== '/login') {
    goto('/login');
  }
}
