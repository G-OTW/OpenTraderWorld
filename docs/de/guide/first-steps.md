# Erste Schritte nach der Installation

Du hast OpenTraderWorld [installiert](/de/guide/install) und deine Admin-Zugangsdaten. So machst du es zu deinem.

## Anmelden

Öffne die App und melde dich unter `/login` an. Wurde dein Passwort **vom Installationsprogramm erzeugt**, wirst du beim ersten Login aufgefordert, **ein neues zu wählen**, denn das erzeugte Passwort funktioniert nur einmal.

Benutzername oder Passwort kannst du jederzeit unter **Einstellungen → Konto** ändern. Eine Passwortänderung meldet dich von allen Sitzungen ab. Ausgesperrt? Es gibt keine Reset-Mail: Stelle den Zugang über die Shell des Hosts wieder her, siehe [Passwort vergessen](/de/guide/troubleshooting#forgot-password).

Schalte als Nächstes unter **Einstellungen → Sicherheit** die **Zwei-Faktor-Authentifizierung** ein und prüfe, welche Browser angemeldet sind. Tu das, bevor du irgendetwas über diesen Rechner hinaus auf die App zugreifen lässt. Siehe [Kontosicherheit](/de/config/security).

## Standardwerte festlegen

Gehe zu **Einstellungen → Standardwerte** und wähle:

- **Sprache**: gilt sofort für die ganze App (Englisch, Französisch, Deutsch, Spanisch, Italienisch, Portugiesisch, Chinesisch).
- **Standardwährung** und **Zeitzone**: dienen modulübergreifend als Startwerte.

## Module installieren

Öffne **Einstellungen → Module**. Jedes Modul wird mit der App ausgeliefert; ein Modul zu installieren macht es nur im Modulwechsler und im Dashboard verfügbar, es wird nichts heruntergeladen.

- **Installiere** die Module, die du willst. Fang klein an, du kannst jederzeit weitere hinzufügen.
- Manche Module hängen von anderen ab: **Historical Data Visualization**, **Backtest** und **Quant Tools** brauchen alle **Historical Data** (sie arbeiten mit dessen heruntergeladenen Datensätzen).
- **Trennen** blendet ein Modul aus und macht es unzugänglich; seine Daten bleiben erhalten, es sei denn, du setzt zusätzlich den Haken bei *Daten löschen*. Du kannst jederzeit neu installieren.

Nicht sicher, wo du anfangen sollst? Die [Modulübersicht](/de/modules/) zeigt, was jedes Modul tut.

## Sich in der App zurechtfinden

- **Modulwechsler** (oben links): springt zwischen installierten Modulen. Jedes Modul besitzt seinen gesamten Arbeitsbereich: eigene Seitenleiste, Seiten und Inhalte.
- **Dashboard** (Startseite): ein Board aus Kacheln und Widgets deiner Module, mit beliebig vielen Seiten. Siehe [Dashboard & Navigation](/de/modules/dashboard).
- **Suche** (obere Leiste, <kbd>⌘K</kbd> / <kbd>Strg+K</kbd> oder <kbd>/</kbd>): findet Module und Einstellungsbereiche; der Ebenen-Schalter erweitert sie auf deine eigenen Inhalte.
- **Glocke** (obere Leiste): der Benachrichtigungseingang, in dem Erinnerungen und Webhook-Alarme landen.
- **Einstellungen**: Konto, Standardwerte, Darstellung, Netzwerk, Module, Daten, Sicherung, Updates, Protokolle, Konnektoren, Tresor und mehr. Siehe die [Einstellungen im Überblick](/de/config/settings).

## Empfohlene nächste Schritte

1. **Gewöhne dir früh Backups an**: siehe [Sicherung & Wiederherstellung](/de/guide/backup-restore).
2. Wenn andere Geräte auf die App zugreifen sollen, lies [Netzwerk & Fernzugriff](/de/config/network), bevor du etwas änderst, und schalte zuerst die [Zwei-Faktor-Authentifizierung](/de/config/security#totp) ein.
3. Nutzt du externe Marktdatenanbieter? Lege unter **Einstellungen → Daten-Konnektoren** je Konto bei Bedarf einen **[Daten-Konnektor](/de/config/connectors)** an. Die App funktioniert auch ohne, und mehrere Anbieter brauchen keinen Schlüssel.
