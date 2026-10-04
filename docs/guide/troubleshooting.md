# Troubleshooting

| Symptom | Likely cause / fix |
|---|---|
| `port is already allocated` | The port (80/443/5454) is in use by something else. Re-run `./setup.sh` and pick a different port. |
| Chrome/Edge can't open the app but Safari can | The browser force-upgrades the address to `https://`, which plain-HTTP modes don't serve. Type `http://` explicitly, or switch to [LAN + HTTPS mode](/config/network#lan-https). |
| Works on `localhost` but not via the machine's IP (macOS) | The macOS firewall blocks Docker's inbound connections. System Settings → Network → Firewall → Options… → set **Docker** to *Allow incoming connections*. |
| LAN + HTTPS: certificate not issued | Check `docker compose logs caddy`. The DuckDNS/Cloudflare token must be valid and the domain spelled exactly. |
| LAN + HTTPS: domain doesn't resolve on some devices | Your resolver blocks private-IP answers (DNS rebind protection). See [the fixes](/config/network#dns-rebind). |
| Setup wizard never appears / `core: offline` in the top bar | Core can't reach Postgres. Check `docker compose logs core` and `logs postgres`; verify `DATABASE_URL` matches `POSTGRES_PASSWORD` in `deploy/.env`. |
| `POSTGRES_PASSWORD` error on startup | `deploy/.env` is missing or empty. Run `./setup.sh`, or copy `.env.example` to `.env` and fill it in. |
| Public mode: HTTPS cert not issued | The domain's DNS must resolve to this server, and ports 80/443 must be reachable from the internet. |
| Code changes not reflected | Rebuild: `docker compose up --build -d`. |
| Locked out, password forgotten | Reset it from the host shell, see [Forgot your password](#forgot-password). |
| Can't reach the app after picking the wrong network mode | Edit `deploy/network.env` by hand and restart, see [changing the mode from the CLI](/config/network#change-mode-cli). |

## Forgot your password {#forgot-password}

There is no reset email and no unauthenticated reset form: OTW runs on your own server, so any endpoint that could change a password without being signed in would be a way in. The reset lives on the **host shell** instead, and the sign-in page's *Forgot password?* link spells out the same steps.

Open a shell on the machine running OTW and print a one-time password:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core reset-password USERNAME
```

Sign in with it; the app asks for a new one straight away.

| Case | What to run |
|---|---|
| Forgot the username too | `docker exec opentraderworld-core-1 /app/otw-core list-users` |
| Choose the password yourself | `printf '%s' 'my-new-password' \| docker exec -i opentraderworld-core-1 /app/otw-core reset-password USERNAME --stdin` |
| Container named differently | `docker ps`, then use the core one in place of `opentraderworld-core-1`. |
| Not running Docker | Run the `otw-core` binary with the same arguments and `DATABASE_URL` set. |

Never pass a password as a command-line argument: a process's command line is readable on the host, which is why `--stdin` exists.

Notes: a reset **signs out every device**, and nothing is lost. The vault and the stored provider keys are sealed with `OTW_SECRET_KEY`, not with your password.

## Lost your authenticator {#lost-authenticator}

Same principle as the password: recover from the host shell.

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

The second factor is removed and every session is signed out. Sign in with your password,
then set it up again on the new device from **Settings → Security**. See
[Two-factor authentication](/config/security#totp).

## A long request is cut off {#request-timeout}

A backtest, sweep or large import that answers with *this request took longer than the
Ns limit* hit the request timeout, not a bug. Raise it in **Settings → Security**; it applies
immediately, no restart. See [Request timeout](/config/security#timeout).

## Reading logs

```bash
cd deploy
docker compose ps              # are all containers up?
docker compose logs -f core    # API server
docker compose logs -f caddy   # proxy / certificates
docker compose logs -f postgres
```

The app also keeps its own log view in **Settings → Logs** (searchable, with a configurable capture level).

## Start completely fresh

::: danger This deletes all data
```bash
cd deploy
docker compose down -v
./setup.sh
```
:::

## Still stuck?

Open an issue on [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues) with the symptom and the relevant log lines.
