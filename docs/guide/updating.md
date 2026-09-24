# Updating

OpenTraderWorld tells you when a new version is available in **Settings → Update app** (it checks GitHub). The app **cannot update itself by design** (it runs without shell or Docker access, to keep the attack surface small), so updates are a couple of commands on the host.

## Before you update

1. **Take a database backup**: see [Backup & restore](/guide/backup-restore).
2. Skim the release notes for breaking changes.

## Which install do you have?

Look at your install directory:

- Only `deploy/` inside, no `.git`: **image install** (the one-command installer, or
  `setup.sh` without `--build`). This is the default.
- `core/`, `frontend/` and a `.git`: **source build** (`install.sh --build`, or a clone
  plus `./setup.sh --build`).

## Image install

Nothing is built and there is no git checkout, so refresh `deploy/` from the release
(that is where the new image tags are pinned), then pull and restart:

```bash
cd /path/to/opentraderworld
TMP=$(mktemp -d)
curl -fsSL https://codeload.github.com/G-OTW/OpenTraderWorld/tar.gz/master | tar -xz -C "$TMP"
cp -R "$TMP"/*/deploy/. deploy/
rm -rf "$TMP"
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  pull
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d
```

Your configuration survives: `.env`, `network.env` and `dns.env` are not part of the
release, so the copy never overwrites them. Everything else in `deploy/` is replaced by
the new version, which is the point: local edits to `docker-compose.yml` or `Caddyfile`
are lost, keep them in a patch if you need them.

## Source build

Refresh the checkout, then rebuild:

```bash
cd /path/to/OpenTraderWorld
git fetch origin
git reset --hard origin/master
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d --build
```

::: warning Use `git reset --hard`, not `git pull`
Each release is published as a fresh snapshot of the repository, so `git pull` reports
"divergent branches" and fails. `git reset --hard origin/master` makes your checkout
match the new release exactly. Your data and configuration are untouched: they live in
Docker volumes and in `.env` / `network.env`, which are not tracked by git. If you edited
tracked files locally, stash them first (`git stash`).
:::

To pull the published images instead of rebuilding from your checkout, add
`-f deploy/docker-compose.images.yml` and use `pull` + `up -d` as in the image install.

That's it:

- Containers are recreated with the new version and restarted.
- **Database migrations run automatically** on the new core container's first boot.
- Your data is untouched: it lives in Docker volumes, independent of the images.

The app is briefly offline while containers recreate. The exact commands (with your configured paths) are also shown in **Settings → Update app**.

## One-off: retire the bootstrap password {#retire-bootstrap-password}

If core logs this on startup, it is talking to you:

```
OTW_ADMIN_PASSWORD is still set but the admin account already exists.
```

`setup.sh` creates the admin from `deploy/.env` on first boot and then blanks that line. On
an install made before it did so, the value is still sitting in the file, and in the
container's environment where `docker inspect` hands it to anyone who can reach the Docker
daemon. It grants nothing at this point, it is simply one more copy of a password on disk.

```bash
sed -i.bak 's/^OTW_ADMIN_PASSWORD=.*/OTW_ADMIN_PASSWORD=/' deploy/.env
chmod 600 deploy/.env && rm -f deploy/.env.bak
```

Then recreate core so it drops out of the environment too:

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d core
```

Keep `OTW_ADMIN_USER`: an empty password makes the whole bootstrap a no-op, which is what you
want, and the line still documents how a headless install creates its first account.
