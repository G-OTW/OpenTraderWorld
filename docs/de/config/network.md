# Netzwerk & Fernzugriff

OpenTraderWorld steuert über vier Netzwerkmodi, **wer die App erreichen kann**. Nach der Installation ist sie **nur über localhost** erreichbar: Nichts in deinem Netzwerk kann sich verbinden, bis du das änderst.

Der unterstützte Weg, den Modus zu wechseln, ist in der App: **Einstellungen → Netzwerk**. Beim Speichern wird der genaue Neustart-Befehl angezeigt, der auf dem Host auszuführen ist (die App kann ihre eigenen Container nicht neu starten); der Stack ist kurz offline, während die Container neu erstellt werden.

## Die vier Modi

| Modus | Erreichbar für | Protokoll | Verwenden, wenn |
|---|---|---|---|
| **Nur Localhost** | diese Maschine | HTTP | Standard. Am sichersten, nichts im Netzwerk kann sich verbinden. |
| **Lokales Netzwerk (LAN)** | Geräte in deinem Netzwerk, über die IP der Maschine | unverschlüsseltes HTTP | Schneller LAN-Zugriff; in vertrauenswürdigen Heimnetzen in Ordnung. Browser warnen oder stellen zwangsweise auf HTTPS um, und sie verweigern das Mikrofon, daher funktioniert die [Sprachsteuerung](/de/config/voice#https) nicht von anderen Geräten. |
| **LAN + HTTPS** | Geräte in deinem Netzwerk, über eine echte Domain | HTTPS, vertrauenswürdiges Zertifikat | LAN-Zugriff ohne Browser-Warnungen. Nichts ist im Internet erreichbar. |
| **Öffentlich (Web)** | jeder, unter deiner Domain | HTTPS (Let's Encrypt) | Du willst von überall zugreifen und akzeptierst die öffentliche Erreichbarkeit. |

## LAN + HTTPS (keine Browser-Warnungen) {#lan-https}

Browser verweigern oder warnen zunehmend bei reinen HTTP-Seiten. Dieser Modus liefert die App an jedes Gerät in deinem Netzwerk mit einem **echten, öffentlich vertrauenswürdigen Zertifikat** aus: keine Warnungen, nichts auf Client-Geräten zu installieren und **nichts im Internet erreichbar**. Der Besitz der Domain wird über einen DNS-Eintrag nachgewiesen (ACME-DNS-01-Challenge), nicht über eine eingehende Verbindung, und die Domain löst auf die private LAN-IP deiner Maschine auf.

Richte ihn bei der Installation ein (`./setup.sh`, Modus `3`) oder später unter **Einstellungen → Netzwerk → Lokales Netzwerk (LAN) + HTTPS**:

1. Melde dich bei [duckdns.org](https://www.duckdns.org) an (kostenlos), füge eine Subdomain hinzu (z. B. `myotw.duckdns.org`) und kopiere das Token deines Kontos, oder nutze deine eigene Domain bei Cloudflare mit einem API-Token zum Bearbeiten von DNS.
2. Gib Domain + Token ein, dazu die LAN-IP deiner Maschine (bei DuckDNS wird der Eintrag automatisch darauf gesetzt; bei Cloudflare legst du den A-Eintrag selbst an).
3. Wende es mit dem angezeigten Neustart-Befehl an. Die erste Anfrage kann ~30 s dauern, während das Zertifikat ausgestellt wird.

Öffne dann `https://myotw.duckdns.org` von einem beliebigen Gerät in deinem Netzwerk.

::: warning Hinweise
- Ausgestellte Zertifikate erscheinen in öffentlichen Certificate-Transparency-Logs, der **Name** der Domain ist also öffentlich sichtbar (die App selbst bleibt nur im LAN erreichbar).
- Das DNS-Token wird in `deploy/dns.env` gespeichert: Diese Datei nie einchecken oder weitergeben.
- Dieser Modus nutzt die Ports **80 + 443** statt des benutzerdefinierten Ports.
:::

### Wenn die Domain auf manchen Geräten nicht aufgelöst wird {#dns-rebind}

Manche Router/Resolver von Providern verwerfen stillschweigend DNS-Antworten, die auf eine private IP zeigen („DNS-Rebind-Schutz“). Lösungen, die beste zuerst:

1. **Erlaube die Domain** in den Einstellungen deines Routers/DNS.
2. **Aktiviere sicheres DNS (DNS over HTTPS)** im Browser. Chrome: Einstellungen → Datenschutz und Sicherheit → Sicherheit → *Sicheres DNS verwenden* → Cloudflare; Firefox: Einstellungen → Datenschutz → *DNS über HTTPS* → Maximaler Schutz.
3. **Hosts-Datei als Umgehung** (pro Rechner, und Handys können das nicht). Ordne die Domain der LAN-IP des Servers zu:

   ```bash
   # macOS / Linux, then flush the cache (macOS only):
   echo "192.168.1.50 myotw.duckdns.org" | sudo tee -a /etc/hosts
   sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder
   ```

   Bearbeite unter Windows `C:\Windows\System32\drivers\etc\hosts` als Administrator, füge dieselbe Zeile hinzu und führe dann `ipconfig /flushdns` aus. Die Hosts-Datei gewinnt immer gegenüber DNS, entferne die Zeile also, wenn sich die IP des Servers ändert.

## Öffentlich (Web) {#public}

Macht die App unter deiner eigenen Domain mit automatischem HTTPS erreichbar.

**Voraussetzungen:**

1. Eine **Domain** mit einem öffentlichen **A-/AAAA-Eintrag**, der auf die öffentliche IP deines Servers zeigt.
2. Eingehendes TCP **80** und **443**, das den Server erreicht: Öffne sie in der Router-/Cloud-Firewall und richte bei NAT eine Portweiterleitung ein. Port 80 wird für das Zertifikat benötigt (HTTP-01-Challenge) und leitet auf HTTPS um.

Wähle Modus `4` in `./setup.sh` oder wechsle unter **Einstellungen → Netzwerk**. Caddy bezieht und erneuert ein Let's-Encrypt-Zertifikat automatisch bei der ersten Anfrage (~30 s).

::: danger Sobald dies aktiv ist, kann jeder die Anmeldeseite erreichen
Aktiviere es erst, wenn dein Admin-Konto existiert, und mit einem starken Passwort. Schalte vorher die
[Zwei-Faktor-Authentifizierung](/de/config/security#totp) ein. Halte `deploy/.env` geheim.
Wechsle jederzeit unter Einstellungen → Netzwerk zurück in einen privaten Modus.
:::

## Das Setup-Token {#setup-token}

Bei **LAN + HTTPS** und **Öffentlich** verlangt der Erststart-Assistent ein **Setup-Token**, bevor er
das erste Konto anlegt.

Der Grund ist ein Wettlauf: Diese Modi antworten dem Netzwerk, und der Assistent ist offen, bis ein Konto
existiert. Ohne Token wird derjenige Administrator, der die Seite zuerst lädt. Das ist ein
echtes Risiko in der Lücke, während sich das DNS verbreitet, und erneut, falls ein Datenbank-Volume je neu erstellt wird.

Der Core erzeugt das Token beim Start und gibt es in sein Log aus:

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env logs core | grep "setup token"
```

Füge es in das zusätzliche Feld ein, das der Assistent zeigt. Bei jedem Neustart wird ein neues erzeugt, ein
verwaistes Token funktioniert also nicht mehr, sobald der Container neu startet.

Du siehst das nicht, wenn du mit `./setup.sh` installiert hast: Es legt das Konto beim ersten Start aus
`deploy/.env` an, bevor irgendetwas den Assistenten erreichen kann. Das Token erscheint nur,
wenn noch kein Konto existiert.

## Den Modus über die CLI ändern {#change-mode-cli}

Wenn du bei der Installation den falschen Modus gewählt hast und die App gar nicht erreichen kannst (z. B. *localhost* auf einem Server ohne Oberfläche), bearbeite `deploy/network.env` direkt und starte neu:

```bash
cd deploy
# make it reachable on your LAN over plain HTTP (mode 2):
#   OTW_BIND=0.0.0.0     (was 127.0.0.1)
#   OTW_HTTP_PORT=5454   (or your chosen port)
$EDITOR network.env
docker compose --env-file .env --env-file network.env up -d
```

`network.env` enthält keine Geheimnisse: Sie hält das Bind-Interface (`127.0.0.1` = nur diese Maschine, `0.0.0.0` = alle Interfaces) und Ports, die von Compose interpoliert werden. LAN + HTTPS braucht mehr als eine Zeile (Zertifikat + DNS-Token), richte diesen Modus also über die Einstellungen oder `./setup.sh` Modus `3` ein. Sobald du die App öffnen kannst, nutze **Einstellungen → Netzwerk**.
