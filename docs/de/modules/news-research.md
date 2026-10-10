# News & Recherche

## News {#news}

Ein selbst gehosteter News-Aggregator. Baue **Dashboards** (z. B. *Krypto*, *Makro*), füge jedem **Quellen** hinzu und lass den Scheduler sie im Hintergrund abfragen.

### Quellen

- **RSS / Atom**: Feed-URL einfügen, fertig.
- **API (JSON)** für alles ohne RSS: Lege Endpunkt, Methode, Header und Query-Parameter fest und ordne dann JSON-Pfade den Item-Feldern zu (Items-Array, Titel, URL, Datum, Zusammenfassung, eindeutige ID für die Deduplizierung). API-Schlüssel gehören in **Geheimnisse** pro Feed, verschlüsselt gespeichert und in Headern oder Parametern als <code v-pre>{{secret:NAME}}</code> referenziert, und werden nie wieder angezeigt.

Eine Feed-URL, ein Header oder ein Parameter kann auch einen Platzhalter <code v-pre>{{vault.item}}</code> tragen, der auf den gemeinsamen [Tresor](/de/config/settings#vault) zeigt. Der Scheduler löst ihn beim Abruf auf, sodass ein Schlüssel über Feeds und Module hinweg wiederverwendet wird, ohne je in der Feed-Konfiguration gespeichert zu sein.

Jede Quelle hat ihr eigenes **Abfrageintervall**; doppelte Quellen werden erkannt, damit derselbe Feed nicht über mehrere Dashboards hinweg zweimal geholt wird. Starte/stoppe das Polling pro Dashboard oder aktualisiere eine Quelle auf Anforderung.

### Lesen

Filtere Einträge nach Suche, Quelle, Typ und Datumsbereich; kompakte oder volle Ansicht; optional 60-Sekunden-Autoaktualisierung mit einem Banner „{n} Updates, zum Laden klicken“. Ein News-Widget kann auch auf der Startseite deines Dashboards sitzen.

## Mailbox {#mailbox}

Deine Newsletter, Marktnachrichten-Mails und Broker-Mails, gelesen aus **deiner eigenen Mailbox**: Nichts läuft über Dritte.

### Eine Mailbox verbinden

Wähle deinen Anbieter (Fastmail, Gmail, iCloud, Zoho, mailbox.org, Posteo, Migadu, Proton Bridge oder jeden anderen IMAP-Server), und die Servereinstellungen sind vorbefüllt; du lieferst ein **App-Passwort**, das im gemeinsamen [Tresor](/de/config/settings#vault) gespeichert wird und sonst nirgends.

**Outlook.com / Microsoft 365** akzeptieren für IMAP kein Passwort mehr, daher melden sie sich stattdessen mit OAuth an: Ein Tab öffnet sich bei Microsoft, du genehmigst den Zugriff, und es kehrt direkt in diese App zurück (Authorization Code + PKCE, nirgends wird ein Secret gespeichert). Das braucht eine einmalige, kostenlose App-Registrierung von dir: Entra ID → App registrations → new registration, dann Authentication → *Mobile and desktop applications* mit der Redirect-URI, die das Formular dir zeigt (`http://localhost:5454/mailbox/oauth` bei einer lokalen Standardinstallation), Public-Client-Flows erlaubt, und API permissions → delegated `IMAP.AccessAsUser.All`. Füge die Application-(Client-)ID ins Formular ein. Die entstehende Anmeldung wird im Tresor verschlüsselt und bei jedem Abruf automatisch erneuert.

Microsoft akzeptiert nur eine Redirect-URI, die `https://…` ist, oder `http://` auf localhost, und sein Portal lehnt eine als `127.0.0.1` getippte `http`-URI ab. Öffne die App also unter `http://localhost:5454` (der Port wird beim Abgleich einer localhost-Redirect-URI ignoriert) oder stelle sie unter [Einstellungen → Netzwerk](/de/config/settings#netzwerk) hinter HTTPS. Ist deine eine unverschlüsselte LAN-Adresse, fällt die Anmeldung auf einen Code zurück, den du auf `microsoft.com/devicelogin` eintippst; das funktioniert für persönliche Outlook.com-Konten weiterhin, aber Microsoft-365-Tenants blockieren die Geräte-Code-Anmeldung inzwischen standardmäßig.

Diese Erneuerung ist das Einzige, was zur Wartung zu wissen ist: Microsoft verwirft eine Anmeldung nach **90 Tagen ohne Nutzung**, eine Mailbox, die du monatelang pausiert hast, will also neu verbunden werden. Die App warnt nach 60 Tagen Leerlauf, und wird die Anmeldung widerrufen (Passwortänderung, MFA-Reset, Admin-Richtlinie), zeigt die Mailbox **Anmeldung nötig** mit einer Schaltfläche Neu verbinden, statt stillschweigend zu scheitern.

Der Zugriff ist strikt **schreibgeschützt**: Der Ordner wird schreibgeschützt geöffnet, und auf deinem Server wird nie etwas markiert, verschoben oder gelöscht. Verbinde mehrere Mailboxen, wenn du mehr als eine hast.

> Erwäge eine **eigene Adresse** für Newsletter. Deine persönliche Post bleibt dann ganz außerhalb der App, das App-Passwort ist mit einem Klick widerrufbar, und an dem Tag, an dem ein Absender seine Liste leakt, weißt du genau, welcher es war.

### Was aufbewahrt wird

Mailinglisten-Mails (alles mit `List-Unsubscribe`, `List-Id` oder `Precedence: bulk`) werden automatisch aufbewahrt. Alles andere wird nur als *Absender, der auf deine Entscheidung wartet*, vermerkt, und kein Inhalt wird gespeichert, bis du ihn ablegst. So kommen die Auszüge eines Brokers hinein: ein Klick auf den neuen Absender, abgelegt als **Broker**.

Absender werden in vier Kategorien abgelegt (**News**, **Newsletter**, **Broker**, **Sonstige**), jederzeit umschaltbar, und die Leseansicht hat einen Ein-Klick-Schalter pro Kategorie sowie einen Filter nach Mailbox, wenn du mehrere hast.

### Lesen

Nachrichten werden beim Eingang bereinigt (Skripte, Styles, Formulare und Frames entfernt) und in einem Sandbox-Frame angezeigt. **Externe Bilder bleiben blockiert**, bis du sie anforderst, sodass das Tracking-Pixel in einem Newsletter nie feuert und der Absender nicht erfährt, dass du ihn geöffnet hast. Anhänge (Broker-Auszüge, PDFs) sind aus der Nachricht herunterladbar.

Pro Nachricht: Stern, als ungelesen markieren, archivieren, **Erinnere mich** (heute Abend / morgen / dieses Wochenende, direkt in [RemindMe](/de/modules/productivity#remindme)) und **Abbestellen**, für dich gesendet, wenn der Absender One-Click unterstützt, sonst in einem Tab geöffnet.

### Der Store

Der Tab **Store** ist deine eigene Newsletter-Liste: eine Karte pro Publikation mit Name, Link, kurzer Beschreibung und Thema (Mindset, Finanzen, Trading, Geopolitik, Wirtschaft, Sonstiges), nach Domain gruppiert und mit einem Klick zu öffnen. Er steht für sich und ist auch ohne verbundene Mailbox nützlich.

## Economic Calendar {#economics}

Anstehende Makro-Ereignisse (Zentralbankentscheide, CPI-Veröffentlichungen, Arbeitsmarktdaten) in einer Kalenderansicht, damit du weißt, was vor deiner Sitzung ansteht. Ein Klick fügt eine Erinnerung für ein Ereignis hinzu.

## FinanceDatabase {#findb}

Ein durchsuchbarer Katalog mit **über 300.000 Instrumenten**: Aktien, ETFs, Fonds, Indizes, Währungen und Kryptowährungen.

Bei der ersten Nutzung **installierst du den Katalog** (ein einmaliger Download von ~15 MB, im Hintergrund importiert). Danach liegt er lokal, und **Suchen berühren nie das Netzwerk**. Suche nach Symbol oder Name, filtere nach Anlageklasse und Attributen und markiere Instrumente mit einem Stern als **Favoriten**, organisiert in Ordnern mit Notizen (z. B. ein Ordner *Watchlist*).

Der Katalog hat einen eigenen Release-Zyklus, getrennt von der App: Die Kopfzeile zeigt, welcher **Snapshot** installiert ist, und die Schaltfläche **Nach Updates suchen** fragt den Herausgeber, ob ein neuerer existiert. Das Aktualisieren importiert den Katalog an Ort und Stelle neu; deine Favoriten bleiben erhalten und verknüpfen sich mit den neuen Zeilen neu.

Der Katalog wird aus dem Open-Source-Projekt [FinanceDatabase](https://github.com/JerBouma/FinanceDatabase) von Jeroen Bouma gebaut, einem von der Community gepflegten Datensatz von Finanzinstrumenten.

## Resources {#resources}

Eine Lesezeichen-Bibliothek für Trading-Bücher, Artikel, Videos und Werkzeuge: Name, optionaler Link, Beschreibung, in Kategorien organisiert. Absichtlich einfach.

Drei Darstellungen: **Karten**, **Liste** und eine **Galerie** mit einem Vorschaubild pro Lesezeichen. Ein Vorschaubild wird hochgeladen, als URL eingefügt oder mit einem Klick aus der eigenen Social-Vorschau des Links geholt; ein Lesezeichen ohne eines bekommt eine Initialen-Kachel statt eines Lochs im Raster.

## Community Docs {#community-docs}

Von der Community geschriebene Anleitungen, synchronisiert aus der [Bibliothek auf opentraderworld.com](https://opentraderworld.com/docs) und **offline lesbar** in der App. Nach Kategorie stöbern, suchen und Favoriten mit einem Stern markieren.

Docs erscheinen nach deiner Wahl als **Karten oder als Liste**, und eine Kategorie-Karte zeigt eine Vorschau der enthaltenen Docs, damit du weißt, was drin ist, bevor du sie öffnest.

Du kannst beitragen: Schreibe ein Dokument im [Editor](/de/modules/productivity#editor) und nutze **Zur Veröffentlichung einreichen**. Es geht in eine Prüfwarteschlange und erscheint in jeder Bibliothek, sobald es genehmigt ist.
