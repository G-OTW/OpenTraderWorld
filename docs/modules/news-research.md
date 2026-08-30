# News & research

## News {#news}

A self-hosted news aggregator. Build **dashboards** (e.g. *Crypto*, *Macro*), add **sources** to each, and let the scheduler poll them in the background.

### Sources

- **RSS / Atom**: paste a feed URL, done.
- **API (JSON)** for anything without RSS: set the endpoint, method, headers and query params, then map JSON paths to item fields (items array, title, URL, date, summary, unique id for dedup). API keys go into per-feed **secrets**, stored encrypted and referenced as <code v-pre>{{secret:NAME}}</code> in headers or params, and are never shown again.

A feed URL, header or param can also carry a <code v-pre>{{vault.item}}</code> placeholder pointing at the shared [Vault](/config/settings#vault). The scheduler resolves it at poll time, so a key is reused across feeds and modules without ever being stored in the feed config.

Each source has its own **poll interval**; duplicate sources are detected so the same feed isn't fetched twice across dashboards. Start/stop polling per dashboard, or refresh a source on demand.

### Reading

Filter items by search, source, type and date range; compact or full view; optional 60-second auto-refresh with a "{n} updates, click to load" banner. A news widget can also sit on your dashboard home page.

## Mailbox {#mailbox}

Your newsletters, market-news mail and broker mail, read from **your own mailbox**: nothing transits a third party.

### Connecting a mailbox

Pick your provider (Fastmail, Gmail, iCloud, Zoho, mailbox.org, Posteo, Migadu, Proton Bridge or any other IMAP server) and the server settings come pre-filled; you supply an **app password**, which is stored in the shared [Vault](/config/settings#vault) and nowhere else.

**Outlook.com / Microsoft 365** no longer accept a password for IMAP, so they sign in with OAuth instead: a tab opens at Microsoft, you approve the access, and it comes straight back to this app (authorisation code + PKCE, no secret is stored anywhere). It needs a one-off, free app registration of your own: Entra ID → App registrations → new registration, then Authentication → *Mobile and desktop applications* with the redirect URI the form shows you (`http://localhost:5454/mailbox/oauth` on a default local install), public client flows allowed, and API permissions → delegated `IMAP.AccessAsUser.All`. Paste the Application (client) ID into the form. The resulting sign-in is encrypted in the vault and renewed automatically at every fetch.

Microsoft only accepts a redirect URI that is `https://…`, or `http://` on localhost, and its portal refuses an `http` URI typed as `127.0.0.1`, so open the app at `http://localhost:5454` (the port is ignored when matching a localhost redirect) or put it behind HTTPS in [Settings → Network](/config/settings#network). If yours is a plain-HTTP LAN address, the sign-in falls back to a code you type on `microsoft.com/devicelogin`; that still works for personal Outlook.com accounts, but Microsoft 365 tenants now block device-code sign-in by default.

That renewal is the only thing to know about maintenance: Microsoft drops a sign-in after **90 days without use**, so a mailbox you paused for months will ask to be reconnected. The app warns after 60 idle days and, if the sign-in is revoked (password change, MFA reset, admin policy), the mailbox shows **Sign-in needed** with a Reconnect button instead of failing silently.

Access is strictly **read-only**: the folder is opened read-only, and nothing is ever flagged, moved or deleted on your server. Connect several mailboxes if you keep more than one.

> Consider a **dedicated address** for newsletters. Your personal mail then stays out of the app entirely, the app password is revocable in one click, and the day a sender leaks its list you know exactly which one did.

### What gets kept

Mailing-list mail (anything carrying `List-Unsubscribe`, `List-Id` or `Precedence: bulk`) is kept automatically. Everything else is only *recorded as a sender awaiting your decision*, and no content is stored until you file it. That is how a broker's statements get in: one click on the new sender, filed as **Broker**.

Senders are filed in four categories (**News**, **Newsletter**, **Broker**, **Other**), switchable at any time, and the reading screen has a one-click toggle per category plus a filter by mailbox when you have several.

### Reading

Messages are sanitised on arrival (scripts, styles, forms and frames removed) and displayed in a sandboxed frame. **Remote images stay blocked** until you ask for them, so the tracking pixel in a newsletter never fires and the sender cannot tell you opened it. Attachments (broker statements, PDFs) are downloadable from the message.

Per message: star, mark unread, archive, **Remind me** (tonight / tomorrow / this weekend, straight into [RemindMe](/modules/productivity#remindme)) and **Unsubscribe**, sent for you when the sender supports one-click, opened in a tab otherwise.

### The store

The **Store** tab is your own list of newsletters: one card per publication with a name, a link, a short description and a topic (mindset, finance, trading, geopolitics, economics, other), grouped by domain and openable in one click. It stands alone, useful even with no mailbox connected.

## Economic Calendar {#economics}

Upcoming macro events (central-bank decisions, CPI prints, employment data) in a calendar view, so you know what's ahead of your session. One click adds a reminder for an event.

## FinanceDatabase {#findb}

A searchable catalog of **300,000+ instruments**: equities, ETFs, funds, indices, currencies and cryptocurrencies.

On first use you **install the catalog** (a one-time ~15 MB download, imported in the background). After that it lives locally and **searches never touch the network**. Search by symbol or name, filter by asset type and attributes, and star instruments into **favorites**, organized in folders with notes (e.g. a *Watchlist* folder).

The catalog is built from the open-source [FinanceDatabase](https://github.com/JerBouma/FinanceDatabase) project by Jeroen Bouma, a community-maintained dataset of financial instruments.

## Resources {#resources}

A bookmark library for trading books, articles, videos and tools: name, optional link, description, organized in categories. Simple on purpose.

Three displays: **cards**, **list**, and a **gallery** with a thumbnail per bookmark. A thumbnail is uploaded, pasted as a URL, or fetched from the link's own social preview in one click; a bookmark without one gets an initials tile instead of a hole in the grid.

## Community Docs {#community-docs}

Guides written by the community, synced from the [library on opentraderworld.com](https://opentraderworld.com/docs) and **readable offline** inside the app. Browse by category, search, and star favorites.

Docs show as **cards or as a list**, your choice, and a category card previews the docs it holds so you know what is inside before opening it.

You can contribute: write a document in the [Editor](/modules/productivity#editor) and use **Submit for publication**. It goes to a review queue and appears in everyone's library once approved.
