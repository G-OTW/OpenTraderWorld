# Voice control

Drive OpenTraderWorld with your voice, or dictate into any text field. **Push to talk only**: the microphone opens when you press and closes when you release. Nothing listens in between.

**It is off by default.** Turn it on in **Settings → Voice → Speech & shortcut**.

## The microphone needs HTTPS or localhost {#https}

Browsers only give a web page the microphone (and their built-in speech recognition) on a **secure origin**: a page served over **HTTPS**, or opened on **localhost**. This is a browser rule, not an OpenTraderWorld one, and no setting in the app can lift it.

| How you open OTW | Voice works? |
|---|---|
| On the machine that runs it, `http://127.0.0.1:5454` or `http://localhost:5454` | Yes |
| [LAN + HTTPS](/config/network#lan-https) or [Public](/config/network#public) mode | Yes |
| [Local network (LAN)](/config/network) mode over plain HTTP, from another device | **No**, the browser hides the microphone |

To use voice from a phone or another computer on your network, switch **Settings → Network** to **LAN + HTTPS**. On plain HTTP the voice settings page shows a warning and the microphone button explains why it cannot start.

## Two shortcuts, two modes

| | Default | What happens to what you say |
|---|---|---|
| **Command** | `Alt+V` (`⌥V` on a Mac) | Becomes a plan of actions, even when a text field has focus. |
| **Dictation** | `Alt+Shift+V` (`⌥⇧V`) | Is typed, word for word, into the text field that has focus. Never read as a command. |

Hold the shortcut while you speak, release to finish, `Esc` to cancel. The **microphone in the top bar** always runs commands: click to start, click again to stop, or hold it like a walkie-talkie.

Both shortcuts can be changed in **Settings → Voice → Speech & shortcut**. Each needs `Ctrl`, `Alt` or `⌘` (or a function key `F1` to `F12`), so it never gets in the way of normal typing, and the two must differ.

## Speech engine

The engine turns your recording into text. Pick one in **Settings → Voice → Speech & shortcut**.

| Engine | Setup | Where the audio goes |
|---|---|---|
| **This browser** | none | The browser vendor's speech service (Google for Chrome, Microsoft for Edge, Apple for Safari). Not available in Firefox. |
| **Self-hosted Whisper** | the bundled service, see [Example: self-hosted Whisper](#whisper-example) | Stays on your machine |
| **whisper.cpp server** | your own server, its `/inference` endpoint | Stays on your server |
| **OpenAI / Groq** | an API key (pasted or plugged from the [Vault](/config/settings#vault)) | Sent to that provider |
| **Other** | any server exposing the OpenAI `/audio/transcriptions` API | That server |

With a server engine the browser records, converts the audio to a small WAV file and OTW relays it to the engine. **Test** sends half a second of silence to check the URL, key and model. Keys are encrypted at rest and never sent back to the browser.

### Example: self-hosted Whisper, from scratch {#whisper-example}

The bundled Whisper service is optional and not started by a plain `up`. Steps 1 to 3 are done once: the model is kept in the `whisper-cache` volume across restarts.

**1. Start the service**, from the repository root:

```bash
docker compose -f deploy/docker-compose.yml --env-file deploy/.env --env-file deploy/network.env --profile voice up -d whisper
docker logs -f opentraderworld-whisper-1
```

Wait for `Uvicorn running on http://0.0.0.0:8000`, then `Ctrl+C`. The port is only reachable by OTW inside Docker, not from your browser, and that is intended.

**2. Download the model** (about 500 MB, a few minutes):

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen(urllib.request.Request('http://localhost:8000/v1/models/Systran/faster-whisper-small',method='POST'),timeout=1800).read())"
```

**3. Check it is installed**: the answer must list `Systran/faster-whisper-small`.

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen('http://localhost:8000/v1/models').read())"
```

**4. Plug it into OTW.** Open the app on `http://localhost:5454` (or over HTTPS, see [above](#https)), then **Settings → Voice → Speech & shortcut**:

1. **Add engine**, preset **Self-hosted Whisper**: it fills in `http://whisper:8000/v1` and `Systran/faster-whisper-small`.
2. **Save and test**: the engine shows **Works · … ms**.
3. Select the engine, then turn the switch at the top **On**. Optionally set the **Language**.

**5. Try it**: hold `⌥V` / `Alt+V`, say *"open journal"*, release, press `Enter`. For dictation, click into a text field and hold `⌥⇧V` / `Alt+Shift+V`.

The first transcription after a restart is slower while the model loads into memory.

## Voice commands

**Settings → Voice → Voice commands**: a command is a **phrase** (plus other ways of saying it) and an ordered list of **steps**:

- **Open a page**: a module, the dashboard or Settings.
- **Ask the agent**: a prompt sent to the floating assistant, which acts with its own tools.
- **Run a workflow**: start an [Automator](/modules/automator) workflow.
- **Theme**, **Hide figures**: the same as the buttons in the top bar.
- **Speak**: read a sentence aloud.

A phrase fires only when it is said **on its own**, never as a word inside a sentence: dictating "a blue turtle" does not run your *turtle* command.

### Built in, no setup

- **"open &lt;page&gt;"** (*"ouvre &lt;page&gt;"*, *"öffne &lt;Seite&gt;"*, *"abre &lt;página&gt;"*, *"apri &lt;pagina&gt;"*, *"打开 &lt;页面&gt;"*) opens any installed module, the dashboard or Settings. A name that matches several pages is refused with the list, never guessed.
- With **Hand the rest to the agent** on, anything no command matches goes to the assistant as one request.

### Chaining

Say several things in one go, joined by *and* or *then* (*et*, *puis* in French, and so on, following the **Language** setting). A comma in the transcript splits too:

> "turtle, then open settings and compare AAPL and MSFT"

runs your *turtle* command, opens Settings, then asks the agent to "compare AAPL and MSFT". Leftover pieces next to each other are kept together, so the agent gets the whole request.

## Confirmation

Every plan is **shown before it runs**: what was heard, each step and where it came from. `Enter` runs it, `Esc` cancels. Steps run in order and the plan stops at the first failure, naming the step.

A command can be marked **Run without confirmation**. It is **off by default**, and it only applies when that phrase is said alone: chained with anything else, the plan is still shown first. Agent steps keep the assistant's own confirmation for every write.

## Good to know

- The assistant is hidden on the **Agent** page, so a plan with an agent step cannot run from there.
- **Read the outcome aloud** uses the browser's own voices: no setup, no audio leaves the browser.
- The public demo shows the voice pages read-only: it does not save settings or send audio.
- **Your own reverse proxy** in front of OTW must not block the microphone: its `Permissions-Policy` header needs `microphone=(self)`. With `microphone=()` the browser refuses at once, without asking.
