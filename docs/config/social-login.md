# Social sign-in

Sign in with a Google, Microsoft, GitHub or OpenID Connect account (Authentik, Keycloak,
Authelia, Zitadel…).

Sign-in is locked to the account you link, by the provider's user id, not by e-mail. Another
account at the same provider is refused. If two-factor authentication is on, the code is
still asked for.

All settings are in **Settings → Security → Social sign-in**. Each change asks for your
password.

## Requirements {#requirements}

- **Redirect URI**: `<address of OTW>/auth/social`, e.g. `https://otw.example.com/auth/social`.
  Settings shows the exact value for the address you are on (click to copy). It must match
  the browser address exactly: scheme, host and port.
- **Google**: `https://` and a domain name. Plain `http://` and raw IP addresses are refused,
  except `localhost` / `127.0.0.1`.
- **Microsoft**: `https://`, or `http://localhost`.
- **GitHub** and self-hosted providers: whatever they allow.

An instance reached over plain HTTP on a LAN address must switch to `lan_https` or `web`
first for Google and Microsoft ([Network](/config/network)).

## Setup {#setup}

### 1. Register OTW at the provider

::: details Google
1. [Google Cloud console](https://console.cloud.google.com/) → **Google Auth Platform**.
   On first use, fill in the app name and support e-mail, audience **External**.
2. **Audience**: while the app is in *Testing*, only listed test users can sign in. Add your
   Google account there.
3. **Clients → Create client**, type **Web application**.
4. **Authorized redirect URIs**: the redirect URI from Settings. Leave *Authorized JavaScript
   origins* empty.
5. Copy the client ID and the client secret. Google can take a few minutes to apply a new
   redirect URI.
:::

::: details Microsoft
1. [Microsoft Entra admin center](https://entra.microsoft.com/) → **App registrations → New
   registration**.
2. **Supported account types**: include personal accounts if you sign in with one
   (Outlook.com, Hotmail, Xbox).
3. **Authentication → Add a platform → Web**: the redirect URI from Settings.
4. **Certificates & secrets → Client secrets → New client secret**. Copy the **Value**, it
   is shown once. It expires (24 months at most), see [Secret expired](#secret-expired).
5. Copy the **Application (client) ID** from **Overview**.
6. Tenant in OTW: `common` (default, any account), `consumers` (personal only),
   `organizations` (work or school only), or your tenant ID or domain.
:::

::: details GitHub
1. GitHub → **Settings → Developer settings → OAuth Apps → New OAuth App**.
2. **Authorization callback URL**: the redirect URI from Settings.
3. Copy the client ID, then **Generate a new client secret** and copy it.
:::

::: details OpenID Connect (self-hosted)
1. Create a confidential OpenID Connect client: authorization code flow, scopes
   `openid email profile`, the redirect URI from Settings.
2. Copy the client ID, the client secret and the **issuer URL**: the address that serves
   `/.well-known/openid-configuration`, e.g. `https://auth.example.com/application/o/otw` on
   Authentik.
3. The issuer must be `https://` (`http://` only on localhost) and must match the `issuer`
   field of that document exactly.
:::

### 2. Enter it in OTW

Pick the provider, paste the client ID and secret (and the tenant or issuer URL), **Save**.
OTW contacts the provider on save: a wrong tenant or issuer fails here, not at sign-in.

### 3. Link your account

**Link an account** sends you to the provider to choose the account. Back in Settings, the
card shows *Locked to …*. The login page now has a **Continue with …** button.

### 4. Generate recovery codes {#recovery-codes}

**Recovery codes → Generate**: ten single-use codes, shown once. They sign you in when the
provider account is locked, deleted or unreachable. Store them outside this server. A new set
cancels the previous one.

### 5. Turn password sign-in off (optional) {#password-off}

Possible once an account is linked and recovery codes exist. **Password sign-in → Turn off**:
the login form refuses passwords, only the linked account and the recovery codes work.

It turns back on by itself on unlink, on remove, on a recovery-code sign-in, and on a host
password reset.

## Rollback {#rollback}

### Still signed in

- **Password sign-in → Turn on**: passwords work again, social sign-in stays.
- **Unlink**: social sign-in stops, passwords work again. The provider settings are kept.
- **Remove**: provider settings and link deleted, passwords work again.

### Provider account unusable

Login page → **Use a recovery code**. You are signed in, password sign-in is back on, and
Settings → Security opens. Unlinking or linking another account there does not ask for the
password for the next five minutes.

### Recovery codes lost too

On the host:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Removes the link, turns password sign-in on and signs out every session. If the password is
lost as well, run `reset-password` next ([Forgot your
password](/guide/troubleshooting#forgot-password)). `list-users` prints the username.

### Back to the previous version {#downgrade}

This release adds migration `0145_social_login`. A previous version refuses to start on a
database with a migration it does not know, so remove it first:

1. Back up the database ([Backup & restore](/guide/backup-restore)).
2. From `deploy/`:

   ```bash
   docker compose --env-file .env --env-file network.env exec -T postgres \
     psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -c "
       BEGIN;
       DROP TABLE IF EXISTS recovery_codes;
       DROP TABLE IF EXISTS social_login;
       ALTER TABLE users DROP COLUMN IF EXISTS password_login;
       DELETE FROM _sqlx_migrations WHERE version = 145;
       COMMIT;"
   ```

3. Install the previous release ([Updating](/guide/updating)): source build,
   `git reset --hard <previous release commit>` then `up -d --build`; image install, the
   previous release's `deploy/` and images.

Passwords work on the previous version whatever the switch was set to. Provider settings and
recovery codes are deleted.

::: warning
This removes migration 145 only. If a later release with more migrations is installed,
restore the backup taken before that update instead.
:::

Step 2 without step 3 resets social sign-in: the current version recreates the tables empty
on its next start.

## Errors {#errors}

| Message | Fix |
|---|---|
| `redirect_uri_mismatch` (Google), `AADSTS50011` (Microsoft) | The registered redirect URI differs from the browser address. Copy it again from Settings. |
| *The OAuth client was not found* / `invalid_client` | Wrong client ID or secret, or the secret expired. |
| *the provider calls itself …* | The issuer URL differs from the `issuer` in the provider's `/.well-known/openid-configuration`. |
| *this … account is not the one linked to this instance* | Another account was chosen at the provider. |
| *this sign-in was started in another browser or has expired* | More than ten minutes passed, or the sign-in ended in another browser. Start again. |
| *the ID token has expired* | The server clock is wrong. Fix the host time (NTP). |

### Secret expired {#secret-expired}

Sign-in fails with `invalid_client` or a message about an expired secret. Create a new secret
at the provider, then **Edit** in Settings, paste it, **Save**. The link is kept. Locked out?
Sign in with a recovery code first.
