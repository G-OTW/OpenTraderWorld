# Actualités et recherche

## News {#news}

Un agrégateur d'actualités auto-hébergé. Construisez des **dashboards** (p. ex. *Crypto*, *Macro*), ajoutez-y des **sources**, et laissez le planificateur les interroger en arrière-plan.

### Sources

- **RSS / Atom** : collez l'URL d'un flux, c'est fait.
- **API (JSON)** pour tout ce qui n'a pas de RSS : définissez le point d'entrée, la méthode, les en-têtes et les paramètres de requête, puis mappez les chemins JSON vers les champs d'un élément (tableau d'éléments, titre, URL, date, résumé, identifiant unique pour le dédoublonnage). Les clés API vont dans des **secrets** propres à chaque flux, stockés chiffrés et référencés par <code v-pre>{{secret:NAME}}</code> dans les en-têtes ou paramètres, et ne sont plus jamais affichés.

L'URL, un en-tête ou un paramètre d'un flux peut aussi porter un marqueur <code v-pre>{{vault.item}}</code> pointant vers le [Coffre](/fr/config/settings#vault) partagé. Le planificateur le résout au moment de l'interrogation, donc une clé est réutilisée entre flux et modules sans jamais être stockée dans la configuration du flux.

Chaque source a son propre **intervalle d'interrogation** ; les sources en double sont détectées pour que le même flux ne soit pas récupéré deux fois entre dashboards. Démarrez/arrêtez l'interrogation par dashboard, ou actualisez une source à la demande.

### Lecture

Filtrez les éléments par recherche, source, type et plage de dates ; vue compacte ou complète ; actualisation automatique optionnelle toutes les 60 secondes avec une bannière "{n} mises à jour, cliquez pour charger". Un widget d'actualités peut aussi se placer sur la page d'accueil de votre dashboard.

## Mailbox {#mailbox}

Vos newsletters, mails d'actualités de marché et mails de broker, lus depuis **votre propre boîte mail** : rien ne transite par un tiers.

### Connecter une boîte mail

Choisissez votre fournisseur (Fastmail, Gmail, iCloud, Zoho, mailbox.org, Posteo, Migadu, Proton Bridge ou tout autre serveur IMAP) et les réglages du serveur sont pré-remplis ; vous fournissez un **mot de passe d'application**, qui est stocké dans le [Coffre](/fr/config/settings#vault) partagé et nulle part ailleurs.

**Outlook.com / Microsoft 365** n'acceptent plus de mot de passe pour IMAP, ils se connectent donc avec OAuth : un onglet s'ouvre chez Microsoft, vous approuvez l'accès, et il revient directement dans cette application (code d'autorisation + PKCE, aucun secret n'est stocké nulle part). Cela exige une inscription d'application ponctuelle et gratuite, la vôtre : Entra ID → Inscriptions d'applications → nouvelle inscription, puis Authentification → *Applications mobiles et de bureau* avec l'URI de redirection que montre le formulaire (`http://localhost:5454/mailbox/oauth` sur une installation locale par défaut), flux de clients publics autorisés, et autorisations d'API → déléguée `IMAP.AccessAsUser.All`. Collez l'ID d'application (client) dans le formulaire. La connexion qui en résulte est chiffrée dans le coffre et renouvelée automatiquement à chaque récupération.

Microsoft n'accepte qu'une URI de redirection en `https://…`, ou en `http://` sur localhost, et son portail refuse une URI `http` saisie en `127.0.0.1`, donc ouvrez l'application sur `http://localhost:5454` (le port est ignoré pour faire correspondre une redirection localhost) ou placez-la derrière HTTPS dans [Paramètres → Réseau](/fr/config/settings#network). Si la vôtre est une adresse LAN en HTTP simple, la connexion se rabat sur un code que vous tapez sur `microsoft.com/devicelogin` ; cela fonctionne toujours pour les comptes Outlook.com personnels, mais les tenants Microsoft 365 bloquent désormais par défaut la connexion par code d'appareil.

Ce renouvellement est la seule chose à savoir sur la maintenance : Microsoft abandonne une connexion après **90 jours sans utilisation**, donc une boîte mail que vous avez mise en pause pendant des mois demandera à être reconnectée. L'application avertit après 60 jours d'inactivité et, si la connexion est révoquée (changement de mot de passe, réinitialisation MFA, politique d'administrateur), la boîte mail affiche **Connexion requise** avec un bouton Reconnecter au lieu d'échouer silencieusement.

L'accès est strictement en **lecture seule** : le dossier est ouvert en lecture seule, et rien n'est jamais marqué, déplacé ou supprimé sur votre serveur. Connectez plusieurs boîtes mail si vous en avez plus d'une.

> Envisagez une **adresse dédiée** pour les newsletters. Votre courrier personnel reste alors entièrement hors de l'application, le mot de passe d'application est révocable en un clic, et le jour où un expéditeur divulgue sa liste vous savez exactement lequel.

### Ce qui est conservé

Le courrier de liste de diffusion (tout ce qui porte `List-Unsubscribe`, `List-Id` ou `Precedence: bulk`) est conservé automatiquement. Tout le reste est seulement *enregistré comme un expéditeur en attente de votre décision*, et aucun contenu n'est stocké tant que vous ne le classez pas. C'est ainsi que les relevés d'un broker entrent : un clic sur le nouvel expéditeur, classé comme **Broker**.

Les expéditeurs sont classés en quatre catégories (**News**, **Newsletter**, **Broker**, **Other**), modifiables à tout moment, et l'écran de lecture a un interrupteur en un clic par catégorie ainsi qu'un filtre par boîte mail quand vous en avez plusieurs.

### Lecture

Les messages sont assainis à l'arrivée (scripts, styles, formulaires et frames retirés) et affichés dans un cadre en bac à sable. **Les images distantes restent bloquées** jusqu'à ce que vous les demandiez, donc le pixel espion d'une newsletter ne se déclenche jamais et l'expéditeur ne peut pas savoir que vous l'avez ouverte. Les pièces jointes (relevés de broker, PDF) sont téléchargeables depuis le message.

Par message : étoile, marquer non lu, archiver, **Me le rappeler** (ce soir / demain / ce week-end, directement dans [RemindMe](/fr/modules/productivity#remindme)) et **Se désabonner**, envoyé pour vous quand l'expéditeur gère le désabonnement en un clic, ouvert dans un onglet sinon.

### Le Store

L'onglet **Store** est votre propre liste de newsletters : une carte par publication avec un nom, un lien, une courte description et un sujet (état d'esprit, finance, trading, géopolitique, économie, autre), groupées par domaine et ouvrables en un clic. Il est autonome, utile même sans boîte mail connectée.

## Economic Calendar {#economics}

Les événements macro à venir (décisions de banques centrales, publications de CPI, données d'emploi) dans une vue calendrier, pour savoir ce qui vous attend avant votre séance. Un clic ajoute un rappel pour un événement.

## FinanceDatabase {#findb}

Un catalogue consultable de **plus de 300 000 instruments** : actions, ETF, fonds, indices, devises et cryptomonnaies.

À la première utilisation, vous **installez le catalogue** (un téléchargement unique d'environ 15 Mo, importé en arrière-plan). Ensuite il vit en local et **les recherches ne touchent jamais le réseau**. Recherchez par symbole ou nom, filtrez par type d'actif et attributs, et mettez des instruments en **favoris**, organisés en dossiers avec des notes (p. ex. un dossier *Watchlist*).

Le catalogue a son propre cycle de publication, distinct de celui de l'application : l'en-tête montre quel **instantané** est installé et un bouton **Vérifier les mises à jour** demande à l'éditeur s'il en existe un plus récent. La mise à jour réimporte le catalogue sur place ; vos favoris survivent et se relient aux nouvelles lignes.

Le catalogue est construit à partir du projet open source [FinanceDatabase](https://github.com/JerBouma/FinanceDatabase) de Jeroen Bouma, un jeu de données d'instruments financiers maintenu par la communauté.

## Resources {#resources}

Une bibliothèque de favoris pour livres de trading, articles, vidéos et outils : nom, lien optionnel, description, organisés en catégories. Simple volontairement.

Trois affichages : **cartes**, **liste**, et une **galerie** avec une miniature par favori. Une miniature est téléversée, collée comme URL, ou récupérée en un clic depuis l'aperçu social du lien lui-même ; un favori qui n'en a pas reçoit une tuile d'initiales plutôt qu'un trou dans la grille.

## Community Docs {#community-docs}

Des guides écrits par la communauté, synchronisés depuis la [bibliothèque sur opentraderworld.com](https://opentraderworld.com/docs) et **lisibles hors ligne** dans l'application. Parcourez par catégorie, recherchez, et mettez des favoris en étoile.

Les docs s'affichent en **cartes ou en liste**, à votre choix, et une carte de catégorie prévisualise les docs qu'elle contient pour que vous sachiez ce qu'il y a dedans avant de l'ouvrir.

Vous pouvez contribuer : écrivez un document dans l'[Editor](/fr/modules/productivity#editor) et utilisez **Soumettre pour publication**. Il part dans une file de revue et apparaît dans la bibliothèque de tous une fois approuvé.
