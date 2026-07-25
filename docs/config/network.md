# Network & remote access

OpenTraderWorld controls **who can reach the app** through four network modes. After install it is **localhost-only** — nothing on your network can connect until you change this.

The supported way to switch modes is in-app: **Settings → Network**. Saving shows the exact restart command to run on the host (the app can't restart its own containers); the stack is briefly offline while containers recreate.

## The four modes

| Mode | Reachable by | Protocol | Use when |
|---|---|---|---|
| **Localhost only** | this machine | HTTP | Default. Safest — nothing on the network can connect. |
| **Local network (LAN)** | devices on your network, via the machine's IP | plain HTTP | Quick LAN access; fine for trusted home networks. Browsers may warn or force-upgrade to HTTPS. |
| **LAN + HTTPS** | devices on your network, via a real domain | HTTPS, trusted certificate | LAN access without browser warnings. Nothing exposed to the internet. |
| **Public (Web)** | anyone, at your domain | HTTPS (Let's Encrypt) | You want access from anywhere and accept public exposure. |

## LAN + HTTPS (no browser warnings) {#lan-https}

Browsers increasingly refuse or warn on plain-HTTP sites. This mode serves the app to every device on your network with a **real, publicly trusted certificate** — no warnings, nothing installed on client devices, and **nothing exposed to the internet**. Domain ownership is proven with a DNS record (ACME DNS-01 challenge), not an inbound connection, and the domain resolves to your machine's private LAN IP.

Set it up at install time (`./setup.sh`, mode `3`) or later in **Settings → Network → Local network (LAN) + HTTPS**:

1. Sign in at [duckdns.org](https://www.duckdns.org) (free), add a subdomain (e.g. `myotw.duckdns.org`) and copy your account token — or use your own domain on Cloudflare with a DNS-edit API token.
2. Enter the domain + token, plus your machine's LAN IP (with DuckDNS the record is pointed at it automatically; on Cloudflare create the A record yourself).
3. Apply with the restart command shown. The first request can take ~30 s while the certificate is issued.

Then open `https://myotw.duckdns.org` from any device on your network.

::: warning Notes
- Issued certificates appear in public Certificate Transparency logs, so the domain **name** is publicly visible (the app itself stays LAN-only).
- The DNS token is stored in `deploy/dns.env` — never commit or share that file.
- This mode uses ports **80 + 443** instead of the custom port.
:::

### If the domain doesn't resolve on some devices {#dns-rebind}

Some ISP routers/resolvers silently drop DNS answers pointing to a private IP ("DNS rebind protection"). Fixes, best first:

1. **Allow the domain** in your router/DNS settings.
2. **Enable secure DNS (DNS over HTTPS)** in the browser — Chrome: Settings → Privacy and security → Security → *Use secure DNS* → Cloudflare; Firefox: Settings → Privacy → *DNS over HTTPS* → Max Protection.
3. **Hosts-file workaround** (per machine — phones can't do this). Map the domain to the server's LAN IP:

   ```bash
   # macOS / Linux — then flush the cache (macOS only):
   echo "192.168.1.50 myotw.duckdns.org" | sudo tee -a /etc/hosts
   sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder
   ```

   On Windows, edit `C:\Windows\System32\drivers\etc\hosts` as Administrator, add the same line, then run `ipconfig /flushdns`. The hosts file always wins over DNS — remove the line if the server's IP changes.

## Public (Web) {#public}

Exposes the app on your own domain with automatic HTTPS.

**Prerequisites:**

1. A **domain** with a public **A / AAAA record** pointing at your server's public IP.
2. Inbound TCP **80** and **443** reaching the server — open them in the router/cloud firewall and port-forward if behind NAT. Port 80 is required for the certificate (HTTP-01 challenge) and redirects to HTTPS.

Pick mode `4` in `./setup.sh` or switch in **Settings → Network**. Caddy obtains and auto-renews a Let's Encrypt certificate on the first request (~30 s).

::: danger Anyone can reach the login page once this is on
Only enable it after your admin account exists and with a strong password. Keep `deploy/.env` secret. Switch back to a private mode any time in Settings → Network.
:::

## Changing the mode from the CLI {#change-mode-cli}

If you picked the wrong mode at install and can't reach the app at all (e.g. chose *localhost* on a headless server), edit `deploy/network.env` directly and restart:

```bash
cd deploy
# make it reachable on your LAN over plain HTTP (mode 2):
#   OTW_BIND=0.0.0.0     (was 127.0.0.1)
#   OTW_HTTP_PORT=5454   (or your chosen port)
$EDITOR network.env
docker compose --env-file .env --env-file network.env up -d
```

`network.env` is secret-free: it holds the bind interface (`127.0.0.1` = this machine only, `0.0.0.0` = all interfaces) and ports, interpolated by Compose. LAN + HTTPS needs more than one line (certificate + DNS token) — set that one up from Settings or `./setup.sh` mode `3`. Once you can open the app, use **Settings → Network**.
