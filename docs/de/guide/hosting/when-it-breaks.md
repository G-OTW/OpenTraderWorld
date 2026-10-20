# Wenn etwas ausfällt

Die Probleme, auf die Leute tatsächlich stoßen, in der Reihenfolge, in der sie meist auftreten.
Jeweils: was du siehst, warum, und was zu tun ist.

::: tip Das Erste, was du immer versuchen solltest
Wenn die Installation stehen geblieben ist, **führe dieselbe Zeile erneut aus**.
Sie merkt sich, was schon funktioniert hat, und macht dort weiter, wo sie aufgehört hat.
Sie stellt nie dieselbe Frage zweimal.
:::

## Während der Installation

### "The name … does not lead anywhere yet"

**Warum:** Deine Adresse (zum Beispiel `app.example.com`) ist noch nicht mit deiner Maschine verbunden.

1. Melde dich bei der Firma an, bei der du die Adresse gekauft hast.
2. Öffne ihre **DNS**-Seite (manchmal **Zone** oder **DNS-Einträge** genannt).
3. Füge einen Eintrag mit genau dem hinzu, was die Meldung ausgegeben hat:

   | Typ | Name | Wert |
   |---|---|---|
   | `A` | das Wort, das die Meldung zeigt (`@` für die nackte Adresse) | die Zahlen, die die Meldung zeigt |

4. Speichere.
5. Warte fünf Minuten.
6. Führe dieselbe Zeile erneut aus.

::: details Das Feld „Name“ ist der übliche Fehler
Gib für `example.com` `@` ein (manche Seiten wollen das Feld leer).
Gib für `app.example.com` nur `app` ein, nicht die vollständige Adresse.
:::

### "The name … does not lead to this machine"

**Warum:** Die Adresse zeigt woandershin: auf eine alte Maschine oder eine Parkseite der Firma, die sie verkauft hat.

1. Öffne dieselbe **DNS**-Seite.
2. Lösche jeden anderen `A`-Eintrag mit diesem Namen.
3. Behalte nur den mit dem Wert, den die Meldung ausgegeben hat.
4. Warte fünf Minuten und führe dann dieselbe Zeile erneut aus.

Wenn du es erst vor wenigen Minuten geändert hast, antworte mit **Ja**, wenn das Installationsprogramm anbietet zu warten.
Es prüft alle 20 Sekunden, bis zu 10 Minuten lang.

### "The address https://… is not answering yet"

**Warum:** fast immer eines von zwei Dingen.

- Du hast die Adresse erst vor wenigen Minuten auf die Maschine gerichtet. **Warte zehn Minuten** und führe dann dieselbe Zeile erneut aus.
- Dein Anbieter hat eine eigene Firewall vor der Maschine, und sie ist geschlossen.

So öffnest du die Firewall des Anbieters:

1. Öffne das Kundenportal deines Anbieters.
2. Suche die **Firewall**- oder **Sicherheits**-Einstellungen der Maschine.
3. Erlaube eingehend **TCP 80** und **TCP 443**.
4. Führe dieselbe Zeile erneut aus.

### "Something on this machine is already answering on port 80 / 443"

**Warum:** Dein Anbieter hat für dich einen Webserver auf der Maschine installiert. Er belegt den Platz, den OpenTraderWorld braucht.

Füge dies ein und führe dann dieselbe Zeile erneut aus:

```bash
systemctl disable --now apache2 nginx caddy 2>/dev/null; true
```

### "This machine has … MB of memory" oder "Only … MB of disk space is free"

**Warum:** Die Maschine ist zu klein. OpenTraderWorld braucht etwa **2 GB Arbeitsspeicher** und **8 GB freien Speicherplatz**.

1. Stelle die Maschine bei deinem Anbieter auf einen größeren Tarif um.
2. Führe dieselbe Zeile erneut aus.

### "This installer only knows Ubuntu and Debian"

**Warum:** Die Maschine wurde mit einem anderen System erstellt.

1. **Installiere** (oder **baue**) die Maschine bei deinem Anbieter mit **Ubuntu 24.04** oder **Debian 13** neu.
2. Führe dieselbe Zeile erneut aus.

### "This needs the machine's administrator rights"

**Warum:** Du bist mit einem Konto angemeldet, das keine Software installieren darf.

Führe die Zeile erneut mit `sudo` in der Mitte aus, genau wie die Meldung es zeigt:

```bash
curl -fsSL https://get.opentraderworld.com/configure_install.sh | sudo bash -s -- --domain app.example.com
```

## Nach der Installation

### Die Seite öffnet sich nicht mehr

1. Melde dich an deiner Maschine an.
2. Tippe:

   ```bash
   otw status
   ```

3. Wenn dort **Nothing is running** oder **is not answering** steht, tippe:

   ```bash
   otw restart
   ```

4. Warte eine Minute und lade die Seite dann neu.

### Ich kann mich nicht mehr an der Maschine selbst anmelden

**Warum:** Das Installationsprogramm hat verschärft, wie die Maschine Leute hereinlässt.

- Wenn du **den Schlüssel** gewählt hast: Melde dich vom selben Computer an, den du am Installationstag benutzt hast. Passwörter werden absichtlich abgelehnt.
- Wenn du **ein Passwort** gewählt hast: Melde dich mit dem Kontonamen auf deiner Karte an, **nicht** mit `root`. Die direkte `root`-Anmeldung wird absichtlich abgelehnt.

Diesen Computer oder dieses Passwort verloren? Nutze die **Konsole** (manchmal **VNC**, **Rescue** oder **Web-Terminal** genannt) im Kundenportal deines Anbieters. Sie funktioniert auch, wenn der normale Zugang gesperrt ist.

### Ich habe das OpenTraderWorld-Passwort vergessen

1. Melde dich an deiner Maschine an.
2. Tippe (ersetze `admin` durch deinen Anmeldenamen, falls du ihn geändert hast):

   ```bash
   docker exec -it opentraderworld-core-1 /app/otw-core reset-password admin
   ```

3. Melde dich mit dem ausgegebenen Passwort in der App an. Die App fordert dich auf, ein neues zu wählen.

Um deine Adresse und deinen Anmeldenamen erneut anzuzeigen, tippe `otw card`.

### Der Browser zeigt „Nicht sicher“ oder lehnt die Seite ab

- Tippe die Adresse mit `https://` davor ein.
- Wenn es erst vor wenigen Minuten nach der Installation gestartet ist, warte zehn Minuten: Das Schloss wird noch ausgestellt.
- Bei einer Heiminstallation (nicht auf einem gemieteten Server) siehe [Fehlerbehebung](/de/guide/troubleshooting).

### Die Maschine ist voll

**Warum:** Nächtliche Backups und heruntergeladene Kurshistorie belegen mit der Zeit Platz.

1. Tippe `otw status`, um zu sehen, wie viel Platz noch frei ist.
2. Gib der Maschine bei deinem Anbieter einen größeren Datenträger.
3. Tippe `otw restart`.

## Immer noch festgefahren?

1. Melde dich an deiner Maschine an.
2. Tippe:

   ```bash
   otw report
   ```

3. Es schreibt eine Datei und gibt aus, wo sie liegt. Die Datei enthält kein Passwort.
4. Eröffne ein Issue auf [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues), schreibe, was du gerade getan hast, und hänge diese Datei an.
