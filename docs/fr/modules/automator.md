# Automator

*Des workflows qui pilotent votre propre application.* Un workflow est une liste d'étapes : appeler un point d'entrée de votre API OpenTraderWorld, appeler n'importe quelle URL extérieure, poser une question à votre fournisseur d'IA, remodeler la réponse, vous notifier. Lancez-le à la main, ou sur planification.

Usages typiques : un brief pré-marché du lundi qui lit les événements économiques de la semaine et vos watchlists et pousse un seul message Telegram ; un export nocturne du journal vers un service externe ; un webhook de prix redistribué en notification ; un résumé hebdomadaire rédigé par l'assistant à partir de vos propres chiffres.

Le module vit à **/automator** avec quatre sections : **Workflows**, **Schedules**, **Agenda** (un calendrier des exécutions à venir), **Runs**.

## La grille

L'éditeur est une grille, pas un canevas : aucun fil à tracer.

- **Les étapes s'exécutent de haut en bas, les tâches d'une étape de gauche à droite.** Tout s'exécute l'un après l'autre. Une étape regroupe les tâches appartenant au même moment du workflow, elle ne les démarre pas ensemble.
- Faites glisser un bloc de la palette vers un **intervalle** : l'intervalle entre deux étapes ouvre une nouvelle étape, l'intervalle entre deux tâches le dépose dans cette étape. Chaque cible de dépôt valide est mise en évidence avant que vous relâchiez.
- Une tâche ne peut lire que ce qui s'est exécuté **avant** elle. Déplacez une carte et ses références sont revérifiées à l'enregistrement suivant.
- **Enregistrement automatique** 1,2 s après chaque modification, **Ctrl/Cmd+Z** pour annuler. Un bloc encore en cours de remplissage est conservé comme **brouillon** : il ne s'exécute jamais et aucune planification ne le prend tant qu'il ne valide pas.

## Blocs

| Bloc | Ce qu'il fait |
|---|---|
| **App call** | Un point d'entrée de votre propre API, exécuté en interne. La palette liste les points d'entrée qu'un workflow peut atteindre ; cliquez sur l'un et le bloc arrive déjà pointé dessus, avec la forme de corps attendue à portée de main. |
| **Web call** | N'importe quelle URL extérieure à l'application : méthode, requête, en-têtes, corps JSON/texte/formulaire, réponse lue en JSON, texte, CSV ou binaire, avec une limite de taille de réponse. |
| **AI step** | Un tour de modèle, sans conversation autour. Choisissez une persona et un prompt stocké ou écrivez les instructions ; demandez un **objet JSON** quand le bloc suivant doit lire des champs plutôt que de la prose. Il appelle votre fournisseur et consomme des tokens à chaque exécution. |
| **Transform** | Remodèle ce qui précède : `pick` une valeur, `set` un objet, `format` une chaîne, `csv_parse` un CSV, `join` une liste en une ligne. |
| **Notify** | Une notification dans l'application et vos [canaux de notification](/fr/config/settings#notifications) (e-mail, Telegram, Slack, Discord). Rien de sélectionné signifie tous les canaux activés accordés à l'Automator. |
| **Wait** | Met l'exécution en pause. **Stop** répond toujours pendant l'attente. |

## Passer des données entre les blocs

Chaque bloc a un **id**, affiché en haut de son éditeur. Un bloc ultérieur lit son résultat avec une expression :

```
{{steps.http1.output}}                     the whole answer
{{steps.http1.output.items.0.name}}        one field of it
{{steps.http1.status}}                     ok | simulated | failed | skipped
{{steps.http1.error}}                      the failure message, empty on success
{{run.started_at}} {{run.trigger}} {{workflow.name}} {{input.key}}
```

Les filtres s'enchaînent après un pipe : `json`, `upper`, `lower`, `trim`, `round:2`, `date:"DD/MM/YYYY"`, `default:"n/a"`. Seul `default` sauve une valeur absente.

Ce n'est **pas un langage** : pas d'arithmétique, pas de code. Chaque référence est vérifiée **à l'enregistrement du graphe**, par rapport aux blocs qui précèdent réellement, donc un lien cassé est refusé dans l'éditeur plutôt qu'à trois heures du matin.

## Secrets

Un mot de passe ou une clé API a sa place dans le [Coffre](/fr/config/settings#vault), jamais saisi dans un champ. Référencez-le avec <code v-pre>{{vault.myvault.mykey}}</code> dans un en-tête, une valeur de requête ou un corps de requête, les seuls endroits où il est accepté (jamais dans une URL), et la valeur résolue est effacée de l'historique des exécutions. Une valeur de requête atteint quand même les logs du site que vous appelez, préférez donc un en-tête.

## Permissions

- **App call exige un token d'accès.** Choisissez-en un dans les **Paramètres** du workflow ; les tokens se créent dans [Paramètres → Agents IA](/fr/config/ai-agents). Pas de token, pas d'appel interne du tout, et un workflow ne peut atteindre que ce que son token accorde, dans la même liste d'autorisation que celle qu'utilise la passerelle MCP.
- **Web call refuse votre propre réseau.** Les adresses de boucle locale, privées, CGNAT et link-local sont rejetées sauf si le bloc autorise explicitement les cibles internes. L'hôte est d'abord résolu et la connexion est épinglée sur l'adresse vérifiée, et chaque saut de redirection est revérifié.
- **Notify exige une autorisation.** Un canal ne s'offre ici qu'une fois accordé à l'Automator dans Paramètres → Notifications.

## L'essayer, puis l'exécuter

- **Test run** effectue les lectures et rapporte ce qu'une écriture ou un envoi *aurait* fait, donc rien ne quitte l'application. Un bloc qui lit une valeur que seule une écriture simulée aurait pu produire est lui-même rapporté comme **simulated**, plutôt que de faire échouer un test qu'une vraie exécution réussirait.
- **Test this block** exécute un seul bloc, mêmes règles.
- **Run now** le fait pour de vrai. **Stop** est vérifié entre les blocs et pendant une attente ; une exécution qui dépasse la **limite d'exécution** du workflow est close comme un timeout.

## Exécutions

Chaque exécution conserve **une ligne par bloc** : ce qui a été envoyé, ce qui est revenu, le statut et la durée, donc un workflow qui a échoué à 3 h du matin nomme le bloc et montre la charge. Une sortie de plus de 256 Ko est mise de côté et récupérée à la demande. L'historique est réduit aux 50 dernières exécutions par workflow (10 pour les test runs).

## Planifications

Toutes les N minutes, quotidien, certains jours de la semaine, mensuel, ou une fois à un moment donné, chacune avec son propre **fuseau horaire IANA**, donc une règle définie en Europe/Paris suit Paris et non le serveur.

- **Pas de chevauchement** : si une exécution est encore en cours quand l'occurrence suivante arrive à échéance, cette occurrence est **abandonnée**, pas mise en file.
- **Rattrapage** (optionnel) : si l'application était arrêtée à l'heure prévue, exécute une fois au démarrage.
- L'heure d'été est résolue, pas ignorée : une heure locale sautée s'exécute à la fin du trou, une heure doublée s'exécute une fois. Une règle mensuelle définie au-delà de la fin d'un mois court s'exécute son dernier jour.
- Une planification peut être **épinglée à une version** du workflow. Enregistrer un nouveau graphe demande si les planifications épinglées doivent le suivre ; l'enregistrement automatique n'en redirige jamais une de lui-même.
- **Pause / reprise** depuis la carte du workflow ou la liste des Schedules. Un workflow désactivé n'est jamais démarré par une planification, bien que le lancer à la main fonctionne toujours.

::: warning Une écriture planifiée écrit
Un App call pointé sur un point d'entrée qui modifie vos données le fera à chaque exécution, sans surveillance. L'éditeur signale ces points d'entrée ; testez le workflow avant de le planifier.
:::

## Laisser un agent construire un workflow {#letting-an-agent-build-a-workflow}

Composer un graphe est la chose la plus difficile que ce module vous demande, et c'est exactement le
genre de travail dans lequel un [agent IA](/fr/config/ai-agents) excelle. Donc un agent détenant un token avec la
permission **Automator** peut lire vos workflows, en créer un, écrire son graphe et le tester.

Ce qu'il ne peut pas faire, c'est le mettre en service. La séparation est voulue :

- Un graphe qu'un agent enregistre arrive comme un **brouillon**, jamais comme le graphe qui s'exécute. Ouvrez le
  workflow, lisez ce qu'il a écrit, et Enregistrez pour l'adopter. Tant que vous ne le faites pas, rien ne change : un
  workflow déjà planifié continue d'exécuter la version que vous avez enregistrée.
- Un agent ne peut pas rattacher le **token d'accès** d'un workflow. Cette enveloppe est ce qui transforme un graphe
  en permissions, vous l'accordez donc à la main, après avoir lu le graphe qu'elle va exécuter.
- Un agent ne peut pas **exécuter** un workflow, restaurer une révision, en supprimer un, ni toucher à une planification.
- Ses **test runs** sont scellés : les notifications et les appels externes sont forcés à l'arrêt, et sans
  enveloppe rattachée un App call échoue en mode fermé. Un test vérifie que le graphe s'exécute, que ses
  expressions se résolvent et que ses transformations font ce qu'elles prétendent, sans rien atteindre. Un
  workflow qui porte déjà un token est refusé : celui-là, vous le testez vous-même.

La raison de cette ligne est qu'un workflow s'exécute sous **son propre** token, pas celui de l'appelant.
Un agent capable à la fois d'écrire un graphe et de le démarrer hériterait de tout ce que ce token accorde,
quoi que disent ses propres permissions. Écrire et armer sont deux autorisations, et une seule des deux
est à vous de déléguer.

L'Automator est désactivé en [mode démo](/fr/guide/demo).
