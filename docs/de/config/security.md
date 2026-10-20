# Kontosicherheit

OTW hat ein Konto und keine Passwort-Reset-Mail. Geschützt wird es durch das Passwort (oder ein
verknüpftes Social-Konto), einen optionalen zweiten Faktor und die Tatsache, dass außer dir niemand
die Maschine erreicht. Diese Seite
behandelt, was du einschalten kannst und was zu tun ist, wenn etwas schiefgeht.

Alles hier befindet sich unter **Einstellungen → Sicherheit**, außer dem Passwort selbst, das unter
**Einstellungen → Konto** bleibt.

## Zwei-Faktor-Authentifizierung {#totp}

Ein sechsstelliger Code aus einer App auf deinem Handy, der nach dem Passwort abgefragt wird. Daran scheitert
ein gestohlenes Passwort allein, und es ist das nützlichste, was du einschalten kannst,
bevor du die App ins Internet öffnest.

::: tip Dir wird nichts geschickt
Das ist **TOTP** (RFC 6238), kein Code, der auf Anforderung per SMS oder E-Mail kommt. Dein Authenticator und
der Server teilen einmal bei der Einrichtung ein Geheimnis, dann berechnen beide denselben Code aus der aktuellen
Zeit. Kein SMS, keine E-Mail, kein Drittanbieter-Dienst, und es funktioniert auch, wenn die Maschine offline ist.
:::

### Einschalten

1. **Einstellungen → Sicherheit → Einrichten**. Die App zeigt einen QR-Code, den dahinterliegenden `otpauth://`-Link
   und das Geheimnis selbst.
2. Füge es einer beliebigen Authenticator-App hinzu: Google Authenticator, Aegis, Ente Auth, 1Password,
   Bitwarden, Proton Pass, was du schon nutzt. Scanne den QR-Code, füge den Link ein
   oder tippe das Geheimnis von Hand ein.
3. Tippe die sechs Ziffern, die sie anzeigt, und bestätige.

An der Anmeldung ändert sich nichts, bis dieser letzte Schritt gelingt. Ein Code, den du nicht erzeugen kannst,
wird nie zur Pflicht, eine halb fertige Einrichtung kann dich also nicht aussperren.

### Danach anmelden

Gib wie bisher Benutzername und Passwort ein; die App fragt dann nach dem Code. Jeder Code gilt
einmal und 30 Sekunden lang, mit etwas Toleranz in beide Richtungen für ein Handy, dessen Uhr
abweicht.

### Ausschalten

**Einstellungen → Sicherheit → Ausschalten**, was erneut nach deinem Passwort fragt. Tu das, bevor du
ein Handy zurücksetzt, und richte ihn auf dem neuen wieder ein.

### Wenn du den Authenticator verlierst {#totp-lost}

Es gibt keinen Backup-Code und keine Wiederherstellungs-Mail, aus demselben Grund, aus dem es kein Formular zum
Zurücksetzen des Passworts gibt. Stelle den Zugang über die Shell der Maschine wieder her:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

Der zweite Faktor und sein Geheimnis werden entfernt und jede Sitzung abgemeldet. Melde dich mit
deinem Passwort an und richte ihn dann auf dem neuen Gerät neu ein.

::: warning Behalte einen Weg zurück
Speichere das Geheimnis bei der Einrichtung in deinem Passwortmanager, oder lass das eigene
Backup deines Authenticators eingeschaltet. Sonst ist der einzige Weg zurück der Shell-Zugriff auf die Maschine.
:::

## Social Login {#social}

Anmeldung über Google, Microsoft, GitHub oder OpenID Connect, an ein verknüpftes Konto gebunden, mit
Wiederherstellungscodes und einem optionalen Schalter, um Passwörter abzuschalten. Siehe
[Social Login](/de/config/social-login).

## Aktive Sitzungen {#sessions}

**Einstellungen → Sicherheit** listet jeden Browser auf, der aktuell am Konto angemeldet ist, mit der
Adresse, von der er kam, und wann er zuletzt genutzt wurde. Der, in dem du das hier liest, ist markiert.

- **Schließen** beendet eine davon.
- **Alle anderen Sitzungen schließen** beendet alle außer deiner, und das ist der Klick, wenn du dich
  irgendwo angemeldet hast, wo du es nicht solltest, oder dir nicht sicher bist.

Eine Anmeldung von einer Adresse, die das Konto noch nie genutzt hat, löst außerdem eine Benachrichtigung aus. Sie
landet in der Glocke und wird an jeden Kanal gepusht, der dem Erzeuger **Sicherheit (Anmeldungen)**
unter **Einstellungen → Benachrichtigungen** gewährt wurde (eine Pauschalfreigabe deckt ihn bereits ab). Sie wird einmal
pro Adresse ausgelöst, nicht bei jeder Anmeldung.

Sitzungen dauern eine Woche. In den netzwerkerreichbaren Modi (**LAN + HTTPS** und **Öffentlich**) enden sie außerdem
nach einem Tag ohne Aktivität, damit ein Browser, der auf einer Maschine offen bleibt, die du verlassen hast,
nicht unbegrenzt angemeldet bleibt. Bei localhost und einfachem LAN gilt nur das Limit von einer Woche.

## Die Passwortabfrage, die wiederkommt {#step-up}

Einige Aktionen fragen erneut nach deinem Passwort, obwohl du bereits angemeldet bist:

- einen Zugriffstoken für einen KI-Agenten erzeugen (**Einstellungen → MCP**)
- den Netzwerkmodus ändern (**Einstellungen → Netzwerk**)
- einen Wert in den **Tresor** schreiben
- eine teilweise Sicherung **mit eingeschlossenen Zugangsdaten** herunterladen
- den zweiten Faktor ausschalten

Diese erzeugen entweder eine Zugangsberechtigung, ändern, was die Außenwelt erreichen kann, oder übergeben dir jedes
gespeicherte Geheimnis in einer Datei. Eine Bestätigung deckt alle für fünf Minuten ab, sodass eine Reihe von
Änderungen nur einmal fragt, und die Bestätigung gehört dem Browser, der sie gegeben hat. Ist der zweite
Faktor an, verlangt die Abfrage auch einen Code.

## Passwortregeln {#password}

Gesetzt oder geändert unter **Einstellungen → Konto**, das immer zuerst nach dem aktuellen Passwort fragt
und bei Erfolg jede andere Sitzung abmeldet.

Ein Passwort muss **mindestens 12 Zeichen** lang sein und darf keines sein, das bereits in
öffentlichen Leak-Listen vorkommt. Diese Prüfung ignoriert die Verzierungen, die Leute hinzufügen, um eine Regel zu umgehen, daher
wird `P@ssw0rd!2024` aus demselben Grund abgelehnt wie `password`. Sequenzen (`abcdefghijkl`) und
alles, was deinen Kontonamen enthält, werden ebenfalls abgelehnt.

Das einfachste Passwort, das durchgeht, besteht aus ein paar unzusammenhängenden Wörtern: `fennel-ladder-oxide-73` wird
akzeptiert, ist kurz und tippbar. Es gibt keine Regel zum Mischen von Symbolen und Ziffern, denn
auf die Länge kommt es an.

Vergessen? Siehe [Passwort vergessen](/de/guide/troubleshooting#forgot-password).

## Anfrage-Timeout {#timeout}

**Einstellungen → Sicherheit** legt außerdem fest, wie lange eine einzelne Anfrage laufen darf, bevor der Server sie
abbricht. Der Standard sind 120 Sekunden.

Es existiert, damit eine hängende Anfrage nie eine Verbindung ewig offen halten kann. Es ist **kein** Rate-Limit:
nichts zählt, wie oft du die App aufrufst, und ein KI-Agent, der die API stark nutzt, ist davon nicht betroffen. Live-Kursströme und die Log-Ansicht sind ebenfalls nicht betroffen, denn das Limit gilt
für das Erzeugen einer Antwort, nicht für die Lebensdauer eines Streams.

Erhöhe es, wenn ein langer Backtest oder ein großer Import mit einer Timeout-Meldung abgebrochen wird.

## Erster Start auf einer netzwerkerreichbaren Instanz {#setup-token}

Wird das allererste Konto auf einer Instanz angelegt, die bereits aus dem Netzwerk erreichbar ist
(**LAN + HTTPS** oder **Öffentlich**), fragt der Einrichtungsassistent nach einem **Setup-Token**. Siehe
[Netzwerk & Fernzugriff](/de/config/network#setup-token).
