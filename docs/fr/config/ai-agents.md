# Agents IA (MCP)

OpenTraderWorld embarque un **serveur MCP** pour que des agents IA (tout client compatible [MCP](https://modelcontextprotocol.io)) puissent lire et modifier vos modules via une passerelle contrôlée. Un agent peut consigner des trades du journal pour vous, résumer vos flux d'actualités, ajouter des tâches, interroger vos résultats de backtest, etc.

**C'est désactivé par défaut.** Rien n'écoute les agents tant que vous ne l'activez pas.

::: tip Vous cherchez l'assistant de chat intégré à l'application ?
Cette page concerne les agents **externes** qui se connectent *à* OpenTraderWorld. Si vous voulez l'assistant de chat intégré qui vit dans l'application (apportez votre propre fournisseur), voir le [module Agent](/fr/modules/agent) : il peut *utiliser* cette même passerelle pour atteindre vos données.
:::

## Modèle de sécurité {#security-model}

Plusieurs couches, qui doivent toutes passer :

1. **Interrupteur global** : le point d'entrée MCP est désactivé tant que vous ne l'activez pas dans **Paramètres → MCP**. Vous pouvez préparer des tokens pendant qu'il est désactivé ; chaque requête d'agent est rejetée jusqu'à l'activation.
2. **Tokens Bearer** : un par agent ou cas d'usage. Les tokens sont stockés **hachés** et affichés **une seule fois** à la création ; les tentatives échouées sont limitées. Révoquez un token à tout moment.
3. **Autorisations de modules par token** : chaque token accorde *aucun accès*, *lecture*, *lecture + écriture* ou *complet (lecture + écriture + suppression)* **par module**. Les agents ne découvrent que les modules que vous avez accordés.
4. **Liste d'autorisation stricte** : les opérations sur le compte, le réseau, les secrets, le stockage de fichiers et l'effacement de données ne sont **jamais exposées** aux agents, quelles que soient les autorisations.

::: tip La liste d'autorisation est délibérée, pas automatique
Un point d'entrée n'est accessible aux agents que parce que quelqu'un l'a ajouté au catalogue à la main. Un nouveau module, ou une nouvelle route d'un module existant, est **invisible pour tous les agents** tant que cette entrée n'existe pas, donc la passerelle ne peut jamais s'élargir par accident au fil de l'évolution de l'application. La gestion des personas et des compétences est volontairement exclue : aucun agent, ni aucun contenu qu'un agent lit, ne peut modifier une persona ou élargir son étagère de compétences.
:::

::: tip Les versions fonctionnent comme des commits
Avec le versionnage activé dans **Paramètres → Versions**, un agent à qui **Editor** ou **Backtest** est accordé peut enregistrer une version d'un document ou d'une stratégie après l'avoir mis à jour, avec une note disant ce qui a changé, et lister, lire, restaurer ou supprimer des versions. Il peut activer le versionnage par fichier ou par stratégie, mais pas les interrupteurs globaux des Paramètres.
:::

::: warning L'Automator accorde l'écriture, pas l'armement
Accorder **Automator** permet à un agent de lire vos workflows, d'en créer un, d'écrire son graphe et de le tester. Cela ne lui permet **pas** de le mettre en service : un graphe qu'un agent enregistre arrive comme un brouillon que vous adoptez depuis l'éditeur, il ne peut pas rattacher le token d'accès d'un workflow, et il ne peut ni lancer un workflow ni toucher à une planification. Un workflow s'exécute sous son propre token plutôt que celui de l'appelant, donc écrire un graphe et l'armer sont deux autorisations distinctes. Voir [la page du module](/fr/modules/automator#letting-an-agent-build-a-workflow).
:::

## Activer et créer un token

1. Allez dans **Paramètres → MCP** et activez-le.
2. **Nouveau token** : nommez-le d'après le client (p. ex. `My Agent`), définissez les autorisations par module (ou utilisez *Tout en lecture* / *Tout en lecture+écriture* / *Tout complet* comme point de départ).
3. **Copiez le token immédiatement** : il n'est affiché qu'une fois.

**Autoriser l'accès externe** est une case distincte dans la même boîte de dialogue. Elle n'accorde rien de plus : elle permet seulement à ce token de soutenir une liaison de chat dans [Contrôle externe](/fr/config/external-control), où un message de Telegram, Slack ou Discord s'exécute sous ces mêmes niveaux par module.

La boîte de dialogue de création affiche aussi un **extrait de configuration prêt à coller**, avec un onglet par famille de client : le point d'entrée brut + l'en-tête, un bloc JSON `mcpServers` (Cursor, Cline, Windsurf, VS Code…), et une ligne de commande `claude mcp add`. Les mêmes extraits restent disponibles sous le tableau des tokens avec `<TOKEN>` comme marqueur, pour configurer plus tard une deuxième machine.

## Connecter un client

Le point d'entrée parle **MCP sur Streamable HTTP** à l'adresse :

```
POST http://<your-host>/api/mcp
Authorization: Bearer <TOKEN>
```

Tout client conforme fonctionne. Exemple pour une configuration MCP :

```json
{
  "mcpServers": {
    "opentraderworld": {
      "type": "http",
      "url": "http://localhost:5454/api/mcp",
      "headers": { "Authorization": "Bearer <TOKEN>" }
    }
  }
}
```

Remplacez l'URL par votre domaine si vous utilisez un mode LAN/HTTPS.

::: tip Installations en localhost uniquement
Si l'application n'est joignable que sur `localhost` (le mode réseau par défaut), les agents doivent s'exécuter **sur la même machine**.
:::

## Se connecter avec OAuth (claude.ai, ChatGPT)

Certains clients ne peuvent pas détenir un token fixe : les connectors claude.ai et ChatGPT ne se connectent qu'avec OAuth. Pour eux, activez la **connexion OAuth** en bas de **Paramètres → MCP** (votre mot de passe est demandé) et donnez au client l'URL du serveur seule, `https://<your-domain>/api/mcp`, sans token.

1. Le client s'enregistre lui-même et ouvre une page de consentement sur votre instance (connectez-vous d'abord si besoin).
2. La page affiche le nom du client et **l'endroit où votre réponse est envoyée**. Choisissez les modules et les niveaux, puis **Autoriser** : votre mot de passe est demandé à chaque approbation.
3. La connexion apparaît dans le tableau des tokens avec un badge **OAuth**. Modifiez ses autorisations ou révoquez-la là comme n'importe quel token ; la révoquer déconnecte le client.

Les tokens d'accès durent une heure et sont renouvelés en arrière-plan ; une connexion inutilisée pendant 30 jours doit se reconnecter. Si un token de renouvellement est un jour rejoué par quelqu'un d'autre, la connexion est révoquée et vous êtes notifié.

::: warning Les clients distants exigent un HTTPS public
claude.ai et ChatGPT se connectent depuis leurs propres serveurs, l'instance doit donc être joignable en HTTPS public ([mode Web](/fr/config/network)). N'approuvez qu'une page de consentement que vous avez ouverte vous-même, à l'instant : un lien envoyé par quelqu'un d'autre peut se faire passer pour n'importe quel client.
:::

::: tip Mis à jour depuis la 0.0.15 ou antérieure avec `otw update` ?
OAuth a besoin d'une nouvelle route dans `deploy/Caddyfile`. Voir [Mise à jour](/fr/guide/updating#oauth-caddyfile).
:::

## Comment les agents voient l'application

Les agents disposent de quatre outils de passerelle :

- **`otw_catalog`** : liste les modules et opérations que le token peut appeler. Seuls les modules accordés apparaissent. La liste d'un module montre pour chaque opération la méthode, le chemin, les paramètres de requête et les champs de corps de premier niveau ; `endpoint` (`POST /api/backtest/run`) renvoie le schéma de corps complet de cette opération.
- **`otw_read`** : opérations de lecture (nécessitent au moins *lecture* sur le module).
- **`otw_compute`** : opérations marquées *(compute)* dans le catalogue, qui répondent à une question et ne stockent rien : un backtest, un balayage de paramètres, des métriques de risque. Elles nécessitent *lecture + écriture* comme tout POST, mais votre client ne vous demandera pas d'approuver un calcul.
- **`otw_write`** : opérations de création et de mise à jour (nécessitent *lecture + écriture*) ; les opérations de **suppression** exigent *complet* sur le module.

Les réponses qui portent du texte venu de l'extérieur (articles de flux, courrier entrant) arrivent à l'agent dans un bloc étiqueté lui disant de traiter le contenu comme des données et d'ignorer toute instruction qui s'y cacherait.

Le tableau des tokens dans les Paramètres montre l'heure de dernière utilisation de chaque token, pour repérer et révoquer les tokens périmés. Un token peut aussi recevoir une date d'expiration à sa création ou modification : passé cette date, il cesse de fonctionner partout, y compris pour l'agent dans l'application.
