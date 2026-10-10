# Fehlerbehebung

| Symptom | Wahrscheinliche Ursache / Lösung |
|---|---|
| `port is already allocated` | Der Port (80/443/5454) wird von etwas anderem belegt. Führe `./setup.sh` erneut aus und wähle einen anderen Port. |
| Chrome/Edge öffnet die App nicht, Safari schon | Der Browser stellt die Adresse zwangsweise auf `https://` um, was die reinen HTTP-Modi nicht bedienen. Tippe `http://` ausdrücklich ein, oder wechsle in den [LAN-+-HTTPS-Modus](/de/config/network#lan-https). |
| Funktioniert über `localhost`, aber nicht über die IP der Maschine (macOS) | Die macOS-Firewall blockiert eingehende Verbindungen von Docker. Systemeinstellungen → Netzwerk → Firewall → Optionen… → **Docker** auf *Eingehende Verbindungen erlauben* setzen. |
| LAN + HTTPS: Zertifikat wird nicht ausgestellt | Prüfe `docker compose logs caddy`. Das DuckDNS-/Cloudflare-Token muss gültig und die Domain exakt geschrieben sein. |
| LAN + HTTPS: Domain wird auf manchen Geräten nicht aufgelöst | Dein Resolver blockiert Antworten mit privaten IPs (DNS-Rebind-Schutz). Siehe [die Lösungen](/de/config/network#dns-rebind). |
| Einrichtungsassistent erscheint nie / `core: offline` in der oberen Leiste | Core erreicht Postgres nicht. Prüfe `docker compose logs core` und `logs postgres`; stelle sicher, dass `DATABASE_URL` zu `POSTGRES_PASSWORD` in `deploy/.env` passt. |
| Fehler zu `POSTGRES_PASSWORD` beim Start | `deploy/.env` fehlt oder ist leer. Führe `./setup.sh` aus, oder kopiere `.env.example` nach `.env` und fülle sie aus. |
| Öffentlicher Modus: HTTPS-Zertifikat wird nicht ausgestellt | Das DNS der Domain muss auf diesen Server zeigen, und die Ports 80/443 müssen aus dem Internet erreichbar sein. |
| Code-Änderungen erscheinen nicht | Neu bauen: `docker compose up --build -d`. |
| Ausgesperrt, Passwort vergessen | Setze es über die Shell des Hosts zurück, siehe [Passwort vergessen](#forgot-password). |
| App nach falsch gewähltem Netzwerkmodus nicht erreichbar | Bearbeite `deploy/network.env` von Hand und starte neu, siehe [Modus über die CLI ändern](/de/config/network#change-mode-cli). |

## Passwort vergessen {#forgot-password}

Es gibt weder eine Reset-Mail noch ein Reset-Formular ohne Anmeldung: OTW läuft auf deinem eigenen Server, und jeder Endpunkt, der ohne Anmeldung ein Passwort ändern könnte, wäre ein Einfallstor. Das Zurücksetzen läuft stattdessen über die **Shell des Hosts**, und der Link *Passwort vergessen?* auf der Anmeldeseite beschreibt dieselben Schritte.

Öffne eine Shell auf der Maschine, die OTW betreibt, und gib ein Einmalpasswort aus:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core reset-password USERNAME
```

Melde dich damit an; die App verlangt sofort ein neues Passwort.

| Fall | Was auszuführen ist |
|---|---|
| Auch den Benutzernamen vergessen | `docker exec opentraderworld-core-1 /app/otw-core list-users` |
| Passwort selbst wählen | `printf '%s' 'my-new-password' \| docker exec -i opentraderworld-core-1 /app/otw-core reset-password USERNAME --stdin` |
| Container heißt anders | `docker ps`, dann den Core-Container anstelle von `opentraderworld-core-1` verwenden. |
| Kein Docker im Einsatz | Führe die Binärdatei `otw-core` mit denselben Argumenten und gesetztem `DATABASE_URL` aus. |

Übergib ein Passwort nie als Kommandozeilenargument: Die Kommandozeile eines Prozesses ist auf dem Host lesbar, deshalb gibt es `--stdin`.

Hinweise: Ein Zurücksetzen **meldet jedes Gerät ab**, und nichts geht verloren. Der Tresor und die gespeicherten Anbieter-Schlüssel sind mit `OTW_SECRET_KEY` versiegelt, nicht mit deinem Passwort.

## Authenticator verloren {#lost-authenticator}

Gleiches Prinzip wie beim Passwort: Stelle den Zugang über die Shell des Hosts wieder her.

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

Der zweite Faktor wird entfernt und jede Sitzung abgemeldet. Melde dich mit deinem Passwort an
und richte ihn dann auf dem neuen Gerät unter **Einstellungen → Sicherheit** neu ein. Siehe
[Zwei-Faktor-Authentifizierung](/de/config/security#totp).

## Von Social Login ausgesperrt {#social-locked}

Das verknüpfte Anbieterkonto ist gesperrt, gelöscht oder nicht erreichbar: Wähle auf der Anmeldeseite **Wiederherstellungscode verwenden**. Die Passwort-Anmeldung wird wieder eingeschaltet.

Auch die Wiederherstellungscodes verloren, auf dem Host:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Entfernt die Verknüpfung, schaltet die Passwort-Anmeldung ein und meldet jede Sitzung ab. Auch das Passwort verloren:
[Passwort vergessen](#forgot-password). Details: [Social Login](/de/config/social-login#rollback).

## Eine lange Anfrage wird abgebrochen {#request-timeout}

Ein Backtest, Sweep oder großer Import, der mit *diese Anfrage hat das Limit von N s überschritten* antwortet, ist am Anfrage-Timeout gescheitert, kein Fehler. Erhöhe es unter **Einstellungen → Sicherheit**; es gilt sofort, ohne Neustart. Siehe [Anfrage-Timeout](/de/config/security#timeout).

## Logs lesen

```bash
cd deploy
docker compose ps              # are all containers up?
docker compose logs -f core    # API server
docker compose logs -f caddy   # proxy / certificates
docker compose logs -f postgres
```

Die App hat außerdem eine eigene Log-Ansicht unter **Einstellungen → Protokolle** (durchsuchbar, mit konfigurierbarer Erfassungsstufe).

## Ganz von vorn beginnen

::: danger Dies löscht alle Daten
```bash
cd deploy
docker compose down -v
./setup.sh
```
:::

## Immer noch festgefahren?

Eröffne ein Issue auf [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues) mit dem Symptom und den relevanten Logzeilen.
