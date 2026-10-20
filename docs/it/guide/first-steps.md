# Primi passi

Hai [installato](/it/guide/install) OpenTraderWorld e hai le credenziali admin. Ecco come renderlo tuo.

## Accedere

Apri l'app e accedi su `/login`. Se la password è stata **generata dall'installer**, ti viene chiesto di **sceglierne una nuova al primo accesso**, poiché la password generata funziona una volta sola.

Puoi cambiare nome utente o password in qualsiasi momento in **Impostazioni → Account**. Cambiare la password ti disconnette da tutte le sessioni. Bloccato fuori? Non c'è email di reset: recupera dalla shell dell'host, vedi [Password dimenticata](/it/guide/troubleshooting#forgot-password).

Poi, in **Impostazioni → Sicurezza**: attiva l'**autenticazione a due fattori** e controlla quali browser sono connessi. Fallo prima di far raggiungere l'app da qualsiasi cosa oltre questa macchina. Vedi [Sicurezza dell'account](/it/config/security).

## Imposta i valori predefiniti

Vai in **Impostazioni → Predefiniti** e scegli:

- **Lingua**: si applica subito a tutta l'app (inglese, francese, tedesco, spagnolo, italiano, portoghese, cinese).
- **Valuta predefinita** e **fuso orario**: usati come valori iniziali in tutti i moduli.

## Installa i tuoi moduli

Apri **Impostazioni → Moduli**. Ogni modulo è incluso nell'app; installarne uno lo rende solo disponibile nel selettore dei moduli e nella dashboard, e non viene scaricato nulla.

- **Installa** i moduli che vuoi. Parti con pochi, puoi aggiungerne altri in qualsiasi momento.
- Alcuni moduli dipendono da altri: **Historical Data Visualization**, **Backtest** e **Quant Tools** richiedono tutti **Historical Data** (lavorano sui suoi dataset scaricati).
- **Scollega** nasconde un modulo e lo rende inaccessibile; i suoi dati vengono mantenuti a meno che tu non spunti anche *elimina i dati*. Puoi reinstallarlo in qualsiasi momento.

Non sai da dove iniziare? Vedi la [panoramica dei moduli](/it/modules/) per cosa fa ciascuno.

## Orientarsi nell'app

- **Selettore dei moduli** (in alto a sinistra): passa tra i moduli installati. Ogni modulo possiede tutta la sua area di lavoro: barra laterale, pagine e contenuti propri.
- **Dashboard** (home): una bacheca di tessere e widget dei tuoi moduli, con quante pagine vuoi. Vedi [Dashboard e navigazione](/it/modules/dashboard).
- **Ricerca** (barra in alto, <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, oppure <kbd>/</kbd>): trova moduli e sezioni delle impostazioni; l'interruttore dei livelli la estende ai tuoi contenuti.
- **Campanella** (barra in alto): la casella delle notifiche, dove arrivano promemoria e avvisi dei webhook.
- **Impostazioni**: account, predefiniti, aspetto, rete, moduli, dati, backup, aggiornamenti, log, connector, vault e altro. Vedi il [riferimento delle impostazioni](/it/config/settings).

## Prossimi passi consigliati

1. **Prendi presto l'abitudine del backup**: vedi [Backup e ripristino](/it/guide/backup-restore).
2. Se altri dispositivi devono raggiungere l'app, leggi [Rete e accesso remoto](/it/config/network) prima di cambiare qualsiasi cosa, e attiva prima l'[autenticazione a due fattori](/it/config/security#totp).
3. Usi provider di dati di mercato esterni? Crea un **[connector dati](/it/config/connectors)** per account in **Impostazioni → Data connector** quando ti servono. L'app funziona benissimo anche senza, e diversi provider non richiedono chiave.
