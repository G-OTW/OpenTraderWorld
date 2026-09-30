# Account security

OTW has one account and no password-reset email. What protects it is the password, an
optional second factor, and the fact that nothing but you can reach the machine. This page
covers what you can turn on and what to do when something goes wrong.

Everything here lives in **Settings → Security**, except the password itself, which stays in
**Settings → Account**.

## Two-factor authentication {#totp}

A six-digit code from an app on your phone, asked for after the password. It is what a
stolen password alone cannot get past, and it is the single most useful thing to switch on
before opening the app to the internet.

::: tip Nothing is sent to you
This is **TOTP** (RFC 6238), not a code texted or emailed on demand. Your authenticator and
the server share a secret once, at setup, then each computes the same code from the current
time. No SMS, no email, no third-party service, and it works with the machine offline.
:::

### Turning it on

1. **Settings → Security → Set up**. The app shows a QR code, the `otpauth://` link behind
   it, and the secret itself.
2. Add it to any authenticator app: Google Authenticator, Aegis, Ente Auth, 1Password,
   Bitwarden, Proton Pass, whichever you already use. Scan the QR code, paste the link in,
   or type the secret by hand.
3. Type the six digits it shows and confirm.

Nothing about signing in changes until that last step succeeds. A code you cannot produce
never becomes a requirement, so a half-finished setup cannot lock you out.

### Signing in afterwards

Enter your username and password as before; the app then asks for the code. Each code works
once and lasts 30 seconds, with a little tolerance either side for a phone whose clock has
drifted.

### Turning it off

**Settings → Security → Turn off**, which asks for your password again. Do this before
wiping a phone, and set it up again on the new one.

### If you lose the authenticator {#totp-lost}

There is no backup code and no recovery email, for the same reason there is no password
reset form. Recover from the machine's shell:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

The second factor and its secret are removed and every session is signed out. Sign in with
your password, then set it up again on the new device.

::: warning Keep a way back in
Store the secret in your password manager when you set it up, or keep your authenticator's
own backup switched on. Otherwise the only way back is shell access to the machine.
:::

## Active sessions {#sessions}

**Settings → Security** lists every browser currently signed in to the account, with the
address it came from and when it was last used. The one you are reading this in is marked.

- **Close** ends one of them.
- **Close every other session** ends all but yours, which is what to click if you signed in
  somewhere you should not have, or you are not sure.

A sign-in from an address the account has never used before also raises a notification. It
lands in the bell, and is pushed to any channel granted to the **Security (sign-ins)**
producer in **Settings → Notifications** (a wildcard grant already covers it). It fires once
per address, not on every sign-in.

Sessions last a week. On the network-facing modes (**LAN + HTTPS** and **Public**) they also
end after a day with no activity, so a browser left open on a machine you walked away from
does not stay signed in indefinitely. On localhost and plain LAN only the one-week limit
applies.

## The password prompt that comes back {#step-up}

A handful of actions ask for your password again even though you are already signed in:

- minting an access token for an AI agent (**Settings → AI agents**)
- changing the network mode (**Settings → Network**)
- writing a value into the **Vault**
- downloading a partial backup **with credentials included**
- turning the second factor off

These either create a credential, change what the outside world can reach, or hand you every
stored secret in one file. One confirmation covers all of them for five minutes, so a run of
changes asks once, and the confirmation belongs to the browser that gave it. If the second
factor is on, the prompt asks for a code too.

## Password rules {#password}

Set or changed in **Settings → Account**, which always asks for the current password first
and signs out every other session on success.

A password must be **at least 12 characters** and must not be one that already appears in
public breach lists. That check ignores the decoration people add to get past a rule, so
`P@ssw0rd!2024` is refused for the same reason `password` is. Sequences (`abcdefghijkl`) and
anything containing your account name are refused too.

The easiest password that passes is a few unrelated words: `fennel-ladder-oxide-73` is
accepted, short and typeable. There is no rule about mixing symbols and digits, because
length is what matters.

Forgot it? See [Forgot your password](/guide/troubleshooting#forgot-password).

## Request timeout {#timeout}

**Settings → Security** also sets how long a single request may run before the server stops
it. The default is 120 seconds.

It exists so a stuck request cannot hold a connection open forever. It is **not** a rate
limit: nothing counts how often you call the app, and an AI agent driving the API hard is
unaffected. Live price streams and the log view are unaffected too, because the limit covers
producing a response, not the life of a stream.

Raise it if a long backtest or a large import is cut off with a timeout message.

## First run on a network-facing instance {#setup-token}

When the very first account is created on an instance already reachable from the network
(**LAN + HTTPS** or **Public**), the setup wizard asks for a **setup token**. See
[Network & remote access](/config/network#setup-token).
