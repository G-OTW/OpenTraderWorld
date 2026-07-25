# ── Build stage ──────────────────────────────────────────────────────────────
FROM node:26-slim AS builder

WORKDIR /build
COPY package.json package-lock.json ./
RUN npm ci

COPY . .
RUN npm run build

# ── Publish stage ────────────────────────────────────────────────────────────
# Tiny image whose only job: copy the built static assets into the shared volume
# that Caddy serves. Exits 0 after copying (compose treats it as a one-shot).
FROM busybox:1.36 AS publish

WORKDIR /publish
COPY --from=builder /build/build /publish/build

# On start: clear stale assets from the mounted /www volume, then copy the fresh
# build in, then exit. Wiping first is essential — SvelteKit emits content-hashed
# filenames, so a plain `cp` would leave old hashed chunks/CSS behind and the app
# could keep serving stale assets across rebuilds.
CMD ["sh", "-c", "rm -rf /www/* /www/.[!.]* 2>/dev/null; cp -r /publish/build/. /www/ && echo 'frontend assets published to /www' && ls /www"]
