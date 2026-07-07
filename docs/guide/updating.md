# Updating

OpenTraderWorld tells you when a new version is available in **Settings → Update app** (it checks GitHub). The app **cannot update itself by design** — it runs without shell or Docker access to keep the attack surface small — so updates are two commands on the host.

## Before you update

1. **Take a database backup** — see [Backup & restore](/guide/backup-restore).
2. Skim the release notes for breaking changes.

## Update

From the repository root on the host:

```bash
git pull
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d --build
```

That's it:

- Containers are rebuilt with the new code and restarted.
- **Database migrations run automatically** on the new core container's first boot.
- Your data is untouched — it lives in Docker volumes, independent of the images.

The app is briefly offline while containers recreate. The exact commands (with your configured paths) are also shown in **Settings → Update app**.
