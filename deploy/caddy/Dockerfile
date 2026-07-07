# Caddy with ACME DNS-challenge providers baked in (stock caddy:2-alpine has none).
# Needed by the "LAN + HTTPS" network mode: certificates for a LAN-only host are
# obtained via DNS-01 (a TXT record at the provider), so no inbound port is required.
FROM caddy:2-builder-alpine AS builder

RUN xcaddy build \
    --with github.com/caddy-dns/duckdns \
    --with github.com/caddy-dns/cloudflare

FROM caddy:2-alpine
COPY --from=builder /usr/bin/caddy /usr/bin/caddy
