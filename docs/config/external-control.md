# External control (chat)

Drive OpenTraderWorld from **Telegram, Slack or Discord**. A message you send to your bot becomes a run of one of your [agent personas](/modules/agent), answered in the chat, with exactly the access the token you picked allows.

**It is off by default**, and it adds no permission system of its own: the ceiling is an [MCP token](/config/ai-agents), the same one the in-app assistant uses.

## Nothing new listens on your machine

All three transports **dial out**: Telegram long polling, Slack Socket Mode, the Discord gateway. There is no public URL to publish, no port to open and no inbound route to attack, so this works unchanged on the default localhost-only install and behind NAT.

The channel you already use for notifications can only **send** (a Slack or Discord webhook URL is write-only). Receiving needs a real bot, so a binding holds its own credential:

| Platform | Credential to paste | On the platform |
|---|---|---|
| **Telegram** | the BotFather bot token | nothing else |
| **Slack** | **both** tokens, `xapp-…` and `xoxb-…`, separated by a space or a newline | Socket Mode on, `message.im` event, `chat:write` scope |
| **Discord** | the bot token | the **Direct Messages** intent (the privileged message-content intent is not needed for DMs) |

## Set one up

1. **Settings → Notifications**: create the channel if you have none. This is the reply path.
2. **Settings → MCP**: create or edit a token, set its per-module levels, and tick **Allow external access**.
3. **Settings → External control**: **New binding**, pick the channel, the persona and that token, paste the bot credential (or plug it from the [Vault](/config/settings#vault)), optionally set the provider and model new chats start on, enable the binding.
4. Turn the section switch on. The binding shows **Connected** within a few seconds.
5. **Pair**: click *Pair*, then send the 6 digit code to your bot **from the account that should be allowed to drive**. It works once, and lasts as long as *Pairing code lasts* says (an hour by default, 5 minutes to a day).

Until someone is paired the bot answers nobody, including you.

## Which model answers

Every chat carries its own provider and model, exactly as a conversation in the app does. The binding sets the **default a new chat starts on**: a phone leg is usually worth a cheaper, faster model than the same persona uses in the browser. Leave it empty and a chat inherits the persona's.

Change the default in the binding form. Change one chat from the chat itself:

- `/provider` lists the configured providers, `/provider 2` or `/provider openrouter` switches this chat to one (which resets the model to that provider's default, since a model id belongs to one vendor).
- `/model` lists that provider's models, `/model 3` picks by position and `/model haiku` picks by text when exactly one id matches.

The choice stays on that chat and moves nothing else. `/new` drops the conversation, so the next one starts on the binding's default again.

## Rights

The token decides everything an answer can touch: *no access*, *read*, *read + write*, *full* per module, exactly as on the [MCP page](/config/ai-agents#security-model). The `external` flag widens nothing, it only says that envelope may be reached from outside.

- **One token per binding, one binding per channel.** Revoking a token stops that binding and nothing else, and the trail still says which way a call came in.
- **Use a dedicated token**, and start it read-only. You can widen it later without re-pairing.
- An **expired** token or one that loses the flag stops the binding at the next message, not at the next restart.

## Who may drive

A chat is a place, not an identity, so authority is bound to the platform's **sender id**:

- Only a paired sender is answered. Anyone else is **ignored without a reply**, which is deliberate: a refusal tells a stranger the bot is real.
- Paired senders are listed on the binding. Click one to remove it.
- **Direct messages only.** A group would let several people write into the prompt of an agent that may hold a write grant.

## Writes always ask

Every write is put to you in the chat with the exact method, path and body, and waits for a word. Only `yes`, `y`, `ok`, `okay`, `approve`, `oui` or `go` approve it; anything else, or silence, refuses it and the agent is told it was refused.

The persona's **auto-approve writes** setting does not carry over. It was ticked in an authenticated session in the app; it does not follow you to a phone.

## What a message passes through

In order, all of them:

1. the global switch in **Settings → External control**
2. the binding being enabled
3. the token still carrying `external`, and not expired
4. a direct chat
5. the sender being on the allowlist
6. a rate limit of 12 messages a minute per binding

Then the run itself is subject to the MCP catalog allowlist: account, network, secrets, file storage and data-wipe routes are unreachable whatever the token says, and so is the management of bindings. No agent can create a binding, mint a pairing code or widen its own reach.

## Security notes

- **The bot token is the access.** Whoever holds it can talk to your instance at that binding's level, with the sender allowlist still in the way. Keep it in the [Vault](/config/settings#vault) and start read-only.
- **Prompt injection is the real risk**, not the transport. Content the agent reads (mail, feeds, inbound webhooks) can carry instructions. What contains it is the same thing that contains it in the app: the catalog allowlist, the token's levels, and the write confirmation above.
- **Pairing codes** are six digits, single use and time limited, and only exist between clicking *Pair* and being redeemed. Shorten the window in the section header if a code will sit on screen.
- Bot credentials are sealed at rest and never printed, including in error messages.
- Disabled wholesale in [demo mode](/guide/demo).

## Limits

- **Text only.** No charts and no files; a chart still lives in the app.
- **No token-by-token streaming.** Chat platforms only offer message edits, and rate limit them, so the reply lands in blocks of about a second and a half and is split at the platform cap (4096, 3000 and 2000 characters).
- Commands: `/new` starts a fresh conversation for that chat, `/provider` and `/model` list and switch what that chat answers with, `/whoami` shows the id you are paired as, `/help`.
- A restart drops a pending write confirmation. Nothing runs unconfirmed, you are simply asked again.
