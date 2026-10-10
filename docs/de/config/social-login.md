# Social Login

Melde dich mit einem Google-, Microsoft-, GitHub- oder OpenID-Connect-Konto an (Authentik, Keycloak,
Authelia, Zitadel…).

Die Anmeldung ist an das Konto gebunden, das du verknüpfst, über die Benutzer-ID des Anbieters, nicht über die E-Mail. Ein anderes
Konto beim selben Anbieter wird abgelehnt. Ist die Zwei-Faktor-Authentifizierung eingeschaltet, wird der Code
trotzdem abgefragt.

Alle Einstellungen findest du unter **Einstellungen → Sicherheit → Social Login**. Jede Änderung fragt nach deinem
Passwort.

## Voraussetzungen {#requirements}

- **Redirect-URI**: `<address of OTW>/auth/social`, z. B. `https://otw.example.com/auth/social`.
  Die Einstellungen zeigen den genauen Wert für die Adresse, auf der du bist (zum Kopieren klicken). Er muss exakt zur
  Browser-Adresse passen: Schema, Host und Port.
- **Google**: `https://` und ein Domainname. Unverschlüsseltes `http://` und reine IP-Adressen werden abgelehnt,
  außer `localhost` / `127.0.0.1`.
- **Microsoft**: `https://` oder `http://localhost`.
- **GitHub** und selbst gehostete Anbieter: was immer sie erlauben.

Eine Instanz, die über unverschlüsseltes HTTP auf einer LAN-Adresse erreicht wird, muss für Google und Microsoft zuerst auf `lan_https` oder `web`
umstellen ([Netzwerk](/de/config/network)).

## Einrichtung {#setup}

### 1. OTW beim Anbieter registrieren

::: details Google
1. [Google Cloud Console](https://console.cloud.google.com/) → **Google Auth Platform**.
   Fülle bei der ersten Nutzung App-Name und Support-E-Mail aus, Zielgruppe **Extern**.
2. **Zielgruppe**: Solange die App im Status *Testing* ist, können sich nur aufgeführte Testnutzer anmelden. Füge dort dein
   Google-Konto hinzu.
3. **Clients → Client erstellen**, Typ **Webanwendung**.
4. **Autorisierte Weiterleitungs-URIs**: die Redirect-URI aus den Einstellungen. Lass *Autorisierte JavaScript-
   Quellen* leer.
5. Kopiere die Client-ID und das Client-Secret. Google kann ein paar Minuten brauchen, um eine neue
   Redirect-URI zu übernehmen.
:::

::: details Microsoft
1. [Microsoft Entra Admin Center](https://entra.microsoft.com/) → **App-Registrierungen → Neue
   Registrierung**.
2. **Unterstützte Kontotypen**: Schließe persönliche Konten ein, wenn du dich mit einem anmeldest
   (Outlook.com, Hotmail, Xbox).
3. **Authentifizierung → Plattform hinzufügen → Web**: die Redirect-URI aus den Einstellungen.
4. **Zertifikate & Geheimnisse → Clientgeheimnisse → Neues Clientgeheimnis**. Kopiere den **Wert**, er
   wird nur einmal angezeigt. Er läuft ab (höchstens 24 Monate), siehe [Secret abgelaufen](#secret-expired).
5. Kopiere die **Anwendungs-ID (Client)** aus der **Übersicht**.
6. Tenant in OTW: `common` (Standard, jedes Konto), `consumers` (nur persönlich),
   `organizations` (nur Arbeit oder Schule) oder deine Tenant-ID bzw. Domain.
:::

::: details GitHub
1. GitHub → **Settings → Developer settings → OAuth Apps → New OAuth App**.
2. **Authorization callback URL**: die Redirect-URI aus den Einstellungen.
3. Kopiere die Client-ID, wähle dann **Generate a new client secret** und kopiere es.
:::

::: details OpenID Connect (selbst gehostet)
1. Erstelle einen vertraulichen OpenID-Connect-Client: Authorization-Code-Flow, Scopes
   `openid email profile`, die Redirect-URI aus den Einstellungen.
2. Kopiere die Client-ID, das Client-Secret und die **Issuer-URL**: die Adresse, die
   `/.well-known/openid-configuration` ausliefert, z. B. `https://auth.example.com/application/o/otw` bei
   Authentik.
3. Der Issuer muss `https://` sein (`http://` nur auf localhost) und exakt zum Feld `issuer`
   dieses Dokuments passen.
:::

### 2. In OTW eintragen

Wähle den Anbieter, füge Client-ID und Secret ein (und den Tenant oder die Issuer-URL), **Speichern**.
OTW kontaktiert den Anbieter beim Speichern: Ein falscher Tenant oder Issuer scheitert hier, nicht bei der Anmeldung.

### 3. Konto verknüpfen

**Konto verknüpfen** schickt dich zum Anbieter, um das Konto zu wählen. Zurück in den Einstellungen zeigt die
Karte *Gebunden an …*. Die Anmeldeseite hat nun eine Schaltfläche **Weiter mit …**.

### 4. Wiederherstellungscodes erzeugen {#recovery-codes}

**Wiederherstellungscodes → Erzeugen**: zehn Einmalcodes, einmalig angezeigt. Sie melden dich an, wenn das
Anbieterkonto gesperrt, gelöscht oder nicht erreichbar ist. Bewahre sie außerhalb dieses Servers auf. Ein neuer Satz
macht den vorherigen ungültig.

### 5. Passwort-Anmeldung ausschalten (optional) {#password-off}

Möglich, sobald ein Konto verknüpft ist und Wiederherstellungscodes existieren. **Passwort-Anmeldung → Ausschalten**:
Das Anmeldeformular lehnt Passwörter ab, nur das verknüpfte Konto und die Wiederherstellungscodes funktionieren.

Sie schaltet sich von selbst wieder ein beim Trennen, beim Entfernen, bei einer Anmeldung per Wiederherstellungscode und bei einem
Passwort-Reset auf dem Host.

## Rollback {#rollback}

### Noch angemeldet

- **Passwort-Anmeldung → Einschalten**: Passwörter funktionieren wieder, Social Login bleibt.
- **Trennen**: Social Login endet, Passwörter funktionieren wieder. Die Anbieter-Einstellungen bleiben erhalten.
- **Entfernen**: Anbieter-Einstellungen und Verknüpfung werden gelöscht, Passwörter funktionieren wieder.

### Anbieterkonto nicht nutzbar

Anmeldeseite → **Wiederherstellungscode verwenden**. Du bist angemeldet, die Passwort-Anmeldung ist wieder an, und
Einstellungen → Sicherheit öffnet sich. Das Trennen oder Verknüpfen eines anderen Kontos dort fragt in den
nächsten fünf Minuten nicht nach dem Passwort.

### Auch die Wiederherstellungscodes verloren

Auf dem Host:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Entfernt die Verknüpfung, schaltet die Passwort-Anmeldung ein und meldet jede Sitzung ab. Ist auch das Passwort
verloren, führe als Nächstes `reset-password` aus ([Passwort
vergessen](/de/guide/troubleshooting#forgot-password)). `list-users` gibt den Benutzernamen aus.

### Zurück zur vorherigen Version {#downgrade}

Dieses Release fügt die Migration `0145_social_login` hinzu. Eine frühere Version weigert sich zu starten, wenn die
Datenbank eine ihr unbekannte Migration enthält, entferne sie also zuerst:

1. Sichere die Datenbank ([Sicherung & Wiederherstellung](/de/guide/backup-restore)).
2. Aus `deploy/`:

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

3. Installiere das vorherige Release ([Aktualisieren](/de/guide/updating)): Build aus dem Quellcode,
   `git reset --hard <previous release commit>` dann `up -d --build`; Image-Installation, das
   `deploy/` und die Images des vorherigen Releases.

Passwörter funktionieren in der vorherigen Version, unabhängig von der Stellung des Schalters. Anbieter-Einstellungen und
Wiederherstellungscodes werden gelöscht.

::: warning
Dies entfernt nur die Migration 145. Ist ein späteres Release mit weiteren Migrationen installiert,
stelle stattdessen das Backup wieder her, das vor diesem Update erstellt wurde.
:::

Schritt 2 ohne Schritt 3 setzt Social Login zurück: Die aktuelle Version erstellt die Tabellen beim nächsten Start leer neu.

## Fehler {#errors}

| Meldung | Lösung |
|---|---|
| `redirect_uri_mismatch` (Google), `AADSTS50011` (Microsoft) | Die registrierte Redirect-URI weicht von der Browser-Adresse ab. Kopiere sie erneut aus den Einstellungen. |
| *The OAuth client was not found* / `invalid_client` | Falsche Client-ID oder falsches Secret, oder das Secret ist abgelaufen. |
| *the provider calls itself …* | Die Issuer-URL weicht vom `issuer` in der `/.well-known/openid-configuration` des Anbieters ab. |
| *this … account is not the one linked to this instance* | Beim Anbieter wurde ein anderes Konto gewählt. |
| *this sign-in was started in another browser or has expired* | Es sind mehr als zehn Minuten vergangen, oder die Anmeldung endete in einem anderen Browser. Starte neu. |
| *the ID token has expired* | Die Uhr des Servers geht falsch. Korrigiere die Zeit des Hosts (NTP). |

### Secret abgelaufen {#secret-expired}

Die Anmeldung scheitert mit `invalid_client` oder einer Meldung über ein abgelaufenes Secret. Erstelle beim Anbieter ein neues Secret,
wähle dann in den Einstellungen **Bearbeiten**, füge es ein und **Speichern**. Die Verknüpfung bleibt erhalten. Ausgesperrt?
Melde dich zuerst mit einem Wiederherstellungscode an.
