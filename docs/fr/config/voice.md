# Commande vocale

Pilotez OpenTraderWorld à la voix, ou dictez dans n'importe quel champ de texte. **Push-to-talk uniquement** : le microphone s'ouvre quand vous appuyez et se ferme quand vous relâchez. Rien n'écoute entre les deux.

**C'est désactivé par défaut.** Activez-le dans **Paramètres → Parole et raccourci**.

## Le microphone exige HTTPS ou localhost {#https}

Les navigateurs ne donnent à une page web le microphone (et leur reconnaissance vocale intégrée) que sur une **origine sécurisée** : une page servie en **HTTPS**, ou ouverte sur **localhost**. C'est une règle des navigateurs, pas d'OpenTraderWorld, et aucun réglage de l'application ne peut la lever.

| Comment vous ouvrez OTW | La voix fonctionne ? |
|---|---|
| Sur la machine qui l'exécute, `http://127.0.0.1:5454` ou `http://localhost:5454` | Oui |
| Mode [LAN + HTTPS](/fr/config/network#lan-https) ou [Public](/fr/config/network#public) | Oui |
| Mode [Réseau local (LAN)](/fr/config/network) en HTTP simple, depuis un autre appareil | **Non**, le navigateur masque le microphone |

Pour utiliser la voix depuis un téléphone ou un autre ordinateur de votre réseau, passez **Paramètres → Réseau** sur **LAN + HTTPS**. En HTTP simple, la page des réglages vocaux affiche un avertissement et le bouton du microphone explique pourquoi il ne peut pas démarrer.

## Deux raccourcis, deux modes

| | Par défaut | Ce qui arrive à ce que vous dites |
|---|---|---|
| **Commande** | `Alt+V` (`⌥V` sur Mac) | Devient un plan d'actions, même quand un champ de texte a le focus. |
| **Dictée** | `Alt+Shift+V` (`⌥⇧V`) | Est saisi, mot pour mot, dans le champ de texte qui a le focus. Jamais lu comme une commande. |

Maintenez le raccourci pendant que vous parlez, relâchez pour terminer, `Esc` pour annuler. Le **microphone de la barre du haut** exécute toujours des commandes : cliquez pour démarrer, cliquez de nouveau pour arrêter, ou maintenez-le comme un talkie-walkie.

Les deux raccourcis peuvent être changés dans **Paramètres → Parole et raccourci**. Chacun exige `Ctrl`, `Alt` ou `⌘` (ou une touche de fonction `F1` à `F12`), pour ne jamais gêner la frappe normale, et les deux doivent différer.

## Moteur de reconnaissance vocale

Le moteur transforme votre enregistrement en texte. Choisissez-en un dans **Paramètres → Parole et raccourci**.

| Moteur | Configuration | Où va l'audio |
|---|---|---|
| **Ce navigateur** | aucune | Le service vocal de l'éditeur du navigateur (Google pour Chrome, Microsoft pour Edge, Apple pour Safari). Indisponible dans Firefox. |
| **Whisper auto-hébergé** | le service fourni, voir [Exemple : Whisper auto-hébergé](#whisper-example) | Reste sur votre machine |
| **Serveur whisper.cpp** | votre propre serveur, son point d'entrée `/inference` | Reste sur votre serveur |
| **OpenAI / Groq** | une clé API (collée ou branchée depuis le [Coffre](/fr/config/settings#vault)) | Envoyé à ce fournisseur |
| **Autre** | tout serveur exposant l'API OpenAI `/audio/transcriptions` | Ce serveur |

Avec un moteur serveur, le navigateur enregistre, convertit l'audio en petit fichier WAV et OTW le relaie au moteur. **Tester** envoie une demi-seconde de silence pour vérifier l'URL, la clé et le modèle. Les clés sont chiffrées au repos et jamais renvoyées au navigateur.

### Exemple : Whisper auto-hébergé, de zéro {#whisper-example}

Le service Whisper fourni est optionnel et n'est pas démarré par un simple `up`. Les étapes 1 à 3 se font une seule fois : le modèle est conservé dans le volume `whisper-cache` entre les redémarrages.

**1. Démarrez le service**, depuis la racine du dépôt :

```bash
docker compose -f deploy/docker-compose.yml --env-file deploy/.env --env-file deploy/network.env --profile voice up -d whisper
docker logs -f opentraderworld-whisper-1
```

Attendez `Uvicorn running on http://0.0.0.0:8000`, puis `Ctrl+C`. Le port n'est joignable que par OTW à l'intérieur de Docker, pas depuis votre navigateur, et c'est voulu.

**2. Téléchargez le modèle** (environ 500 Mo, quelques minutes) :

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen(urllib.request.Request('http://localhost:8000/v1/models/Systran/faster-whisper-small',method='POST'),timeout=1800).read())"
```

**3. Vérifiez qu'il est installé** : la réponse doit lister `Systran/faster-whisper-small`.

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen('http://localhost:8000/v1/models').read())"
```

**4. Branchez-le dans OTW.** Ouvrez l'application sur `http://localhost:5454` (ou en HTTPS, voir [ci-dessus](#https)), puis **Paramètres → Parole et raccourci** :

1. **Ajouter un moteur**, préréglage **Whisper auto-hébergé** : il renseigne `http://whisper:8000/v1` et `Systran/faster-whisper-small`.
2. **Enregistrer et tester** : le moteur affiche **Fonctionne · … ms**.
3. Sélectionnez le moteur, puis passez l'interrupteur du haut sur **Activé**. Réglez éventuellement la **Langue**.

**5. Essayez** : maintenez `⌥V` / `Alt+V`, dites *"ouvre journal"*, relâchez, appuyez sur `Entrée`. Pour la dictée, cliquez dans un champ de texte et maintenez `⌥⇧V` / `Alt+Shift+V`.

La première transcription après un redémarrage est plus lente, le temps que le modèle se charge en mémoire.

## Commandes vocales

**Paramètres → Commandes vocales** : une commande est une **phrase** (plus d'autres façons de la dire) et une liste ordonnée d'**étapes** :

- **Ouvrir une page** : un module, le dashboard ou les Paramètres.
- **Demander à l'agent** : un prompt envoyé à l'assistant flottant, qui agit avec ses propres outils.
- **Lancer un workflow** : démarre un workflow [Automator](/fr/modules/automator).
- **Thème**, **Masquer les chiffres** : comme les boutons de la barre du haut.
- **Parler** : lit une phrase à voix haute.

Une phrase ne se déclenche que lorsqu'elle est dite **seule**, jamais comme un mot dans une phrase : dicter "une tortue bleue" ne lance pas votre commande *tortue*.

### Intégré, sans configuration

- **"open &lt;page&gt;"** (*"ouvre &lt;page&gt;"*, *"öffne &lt;Seite&gt;"*, *"abre &lt;página&gt;"*, *"apri &lt;pagina&gt;"*, *"打开 &lt;页面&gt;"*) ouvre n'importe quel module installé, le dashboard ou les Paramètres. Un nom qui correspond à plusieurs pages est refusé avec la liste, jamais deviné.
- Avec **Confier le reste à l'agent** activé, tout ce qu'aucune commande ne reconnaît part vers l'assistant comme une seule demande.

### Enchaînement

Dites plusieurs choses d'un coup, reliées par *et* ou *puis* (*and*, *then* en anglais, etc., selon le réglage de **Langue**). Une virgule dans la transcription sépare aussi :

> "tortue, puis ouvre paramètres et compare AAPL et MSFT"

lance votre commande *tortue*, ouvre les Paramètres, puis demande à l'agent de "compare AAPL et MSFT". Les morceaux restants côte à côte sont gardés ensemble, l'agent reçoit donc la demande entière.

## Confirmation

Chaque plan est **affiché avant d'être exécuté** : ce qui a été entendu, chaque étape et son origine. `Entrée` l'exécute, `Esc` l'annule. Les étapes s'exécutent dans l'ordre et le plan s'arrête au premier échec, en nommant l'étape.

Une commande peut être marquée **Exécuter sans confirmation**. C'est **désactivé par défaut**, et cela ne s'applique que lorsque cette phrase est dite seule : enchaînée avec autre chose, le plan est tout de même affiché d'abord. Les étapes de l'agent gardent la confirmation propre à l'assistant pour chaque écriture.

## Bon à savoir

- L'assistant est masqué sur la page **Agent**, donc un plan avec une étape d'agent ne peut pas s'y exécuter.
- **Lire le résultat à voix haute** utilise les voix propres au navigateur : aucune configuration, aucun audio ne quitte le navigateur.
- La démo publique montre les pages vocales en lecture seule : elle n'enregistre pas les réglages et n'envoie pas d'audio.
- **Votre propre reverse proxy** devant OTW ne doit pas bloquer le microphone : son en-tête `Permissions-Policy` doit contenir `microphone=(self)`. Avec `microphone=()`, le navigateur refuse immédiatement, sans demander.
