// The core fetch wrapper: what a caller gets back when the server refuses, and when a
// stale session bounces the user to the login screen.
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '../mocks/node.js';
import { apiError } from '../mocks/handlers.js';
import { goto } from '$app/navigation';
import { api, handleUnauthorized } from '$lib/api.js';
import { journalApi } from '$lib/modules/journal/api.js';

vi.mock('$app/navigation', () => ({ goto: vi.fn() }));

beforeEach(() => {
  goto.mockClear();
  window.history.replaceState(null, '', '/journal');
});

describe('api request', () => {
  test('a success returns the parsed body', async () => {
    await expect(api.health()).resolves.toEqual({
      status: 'ok',
      services: { core: 'ok', postgres: 'ok' }
    });
  });

  test('a refusal throws the server message with its code and status', async () => {
    server.use(http.post('*/api/login', () => apiError(403, 'second factor required', 'totp_required')));
    const err = await api.login('u', 'p').catch((e) => e);
    expect(err).toBeInstanceOf(Error);
    expect(err.message).toBe('second factor required');
    expect(err.code).toBe('totp_required');
    expect(err.status).toBe(403);
  });

  test('an error without a JSON body names the status', async () => {
    server.use(http.get('*/api/health', () => new HttpResponse('Bad Gateway', { status: 502 })));
    const err = await api.health().catch((e) => e);
    expect(err.message).toBe('request failed (502)');
    expect(err.code).toBeNull();
    expect(err.status).toBe(502);
  });

  test('a 401 on an auth handshake route is shown, not redirected', async () => {
    server.use(http.get('*/api/health', () => apiError(401, 'not signed in')));
    await expect(api.health()).rejects.toThrow('not signed in');
    server.use(http.post('*/api/auth/social/start', () => apiError(401, 'expired')));
    await expect(api.socialStart()).rejects.toThrow('expired');
    expect(goto).not.toHaveBeenCalled();
  });

  test('a 401 on a module route sends the user to the login screen', async () => {
    server.use(http.get('*/api/journal/categories', () => apiError(401, 'session expired')));
    await expect(journalApi.listCategories()).rejects.toThrow('session expired');
    expect(goto).toHaveBeenCalledExactlyOnceWith('/login');
  });

  test('the login screen itself never redirects to itself', () => {
    window.history.replaceState(null, '', '/login');
    handleUnauthorized();
    expect(goto).not.toHaveBeenCalled();
    window.history.replaceState(null, '', '/journal');
    handleUnauthorized();
    expect(goto).toHaveBeenCalledExactlyOnceWith('/login');
  });

  test('the sent body is the JSON the server expects', async () => {
    let seen = null;
    server.use(
      http.post('*/api/login', async ({ request }) => {
        seen = await request.json();
        return HttpResponse.json({ ok: true });
      })
    );
    await api.login('alice', 'pw', '', true);
    expect(seen).toEqual({ username: 'alice', password: 'pw', code: null, remember: true });
  });

  test('isAuthenticated is false on a 401 and on a network failure, true on 200', async () => {
    server.use(http.get('*/api/settings/me', () => apiError(401, 'no')));
    expect(await api.isAuthenticated()).toBe(false);
    server.use(http.get('*/api/settings/me', () => HttpResponse.error()));
    expect(await api.isAuthenticated()).toBe(false);
    server.use(http.get('*/api/settings/me', () => HttpResponse.json({ user: 'a' })));
    expect(await api.isAuthenticated()).toBe(true);
    expect(goto).not.toHaveBeenCalled();
  });
});
