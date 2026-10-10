# Social login

Connectez-vous avec un compte Google, Microsoft, GitHub ou OpenID Connect (Authentik, Keycloak,
Authelia, Zitadel…).

La connexion est verrouillée sur le compte que vous liez, par l'identifiant utilisateur du fournisseur, pas par e-mail. Un autre
compte chez le même fournisseur est refusé. Si l'authentification à deux facteurs est activée, le code est
toujours demandé.

Tous les réglages sont dans **Paramètres → Sécurité → Social login**. Chaque modification demande votre
mot de passe.

## Prérequis {#requirements}

- **URI de redirection** : `<address of OTW>/auth/social`, p. ex. `https://otw.example.com/auth/social`.
  Les Paramètres affichent la valeur exacte pour l'adresse où vous êtes (cliquer pour copier). Elle doit correspondre à
  l'adresse du navigateur exactement : schéma, hôte et port.
- **Google** : `https://` et un nom de domaine. Le `http://` simple et les adresses IP brutes sont refusés,
  sauf `localhost` / `127.0.0.1`.
- **Microsoft** : `https://`, ou `http://localhost`.
- **GitHub** et fournisseurs auto-hébergés : ce qu'ils autorisent.

Une instance jointe en HTTP simple sur une adresse de réseau local doit d'abord passer en `lan_https` ou `web`
pour Google et Microsoft ([Réseau](/fr/config/network)).

## Configuration {#setup}

### 1. Enregistrer OTW chez le fournisseur

::: details Google
1. [Console Google Cloud](https://console.cloud.google.com/) → **Google Auth Platform**.
   À la première utilisation, renseignez le nom de l'application et l'e-mail d'assistance, public **Externe**.
2. **Audience** : tant que l'application est en *Test*, seuls les utilisateurs de test listés peuvent se connecter. Ajoutez votre
   compte Google ici.
3. **Clients → Créer un client**, type **Application Web**.
4. **URI de redirection autorisés** : l'URI de redirection affichée dans les Paramètres. Laissez *Origines JavaScript
   autorisées* vide.
5. Copiez l'ID client et le secret client. Google peut mettre quelques minutes à appliquer une nouvelle
   URI de redirection.
:::

::: details Microsoft
1. [Centre d'administration Microsoft Entra](https://entra.microsoft.com/) → **Inscriptions d'applications → Nouvelle
   inscription**.
2. **Types de comptes pris en charge** : incluez les comptes personnels si vous vous connectez avec l'un d'eux
   (Outlook.com, Hotmail, Xbox).
3. **Authentification → Ajouter une plateforme → Web** : l'URI de redirection affichée dans les Paramètres.
4. **Certificats et secrets → Secrets client → Nouveau secret client**. Copiez la **Valeur**, elle
   n'est affichée qu'une fois. Elle expire (24 mois au plus), voir [Secret expiré](#secret-expired).
5. Copiez l'**ID d'application (client)** depuis **Vue d'ensemble**.
6. Locataire dans OTW : `common` (par défaut, tout compte), `consumers` (comptes personnels uniquement),
   `organizations` (comptes professionnels ou scolaires uniquement), ou l'ID ou le domaine de votre locataire.
:::

::: details GitHub
1. GitHub → **Settings → Developer settings → OAuth Apps → New OAuth App**.
2. **Authorization callback URL** : l'URI de redirection affichée dans les Paramètres.
3. Copiez l'ID client, puis **Generate a new client secret** et copiez-le.
:::

::: details OpenID Connect (auto-hébergé)
1. Créez un client OpenID Connect confidentiel : flux de code d'autorisation, scopes
   `openid email profile`, l'URI de redirection affichée dans les Paramètres.
2. Copiez l'ID client, le secret client et l'**URL de l'émetteur** : l'adresse qui sert
   `/.well-known/openid-configuration`, p. ex. `https://auth.example.com/application/o/otw` sur
   Authentik.
3. L'émetteur doit être en `https://` (`http://` uniquement sur localhost) et doit correspondre exactement au champ `issuer`
   de ce document.
:::

### 2. La saisir dans OTW

Choisissez le fournisseur, collez l'ID client et le secret (et le locataire ou l'URL de l'émetteur), **Enregistrez**.
OTW contacte le fournisseur à l'enregistrement : un mauvais locataire ou émetteur échoue ici, pas à la connexion.

### 3. Lier votre compte

**Lier un compte** vous envoie chez le fournisseur pour choisir le compte. De retour dans les Paramètres, la
carte affiche *Verrouillé sur …*. La page de connexion a maintenant un bouton **Continuer avec …**.

### 4. Générer les codes de récupération {#recovery-codes}

**Codes de récupération → Générer** : dix codes à usage unique, affichés une seule fois. Ils vous connectent quand le
compte du fournisseur est verrouillé, supprimé ou injoignable. Stockez-les en dehors de ce serveur. Une nouvelle série
annule la précédente.

### 5. Désactiver la connexion par mot de passe (optionnel) {#password-off}

Possible une fois un compte lié et des codes de récupération existants. **Connexion par mot de passe → Désactiver** :
le formulaire de connexion refuse les mots de passe, seuls le compte lié et les codes de récupération fonctionnent.

Elle se réactive d'elle-même au déliement, à la suppression, à une connexion par code de récupération, et à une
réinitialisation du mot de passe depuis l'hôte.

## Retour arrière {#rollback}

### Toujours connecté

- **Connexion par mot de passe → Activer** : les mots de passe refonctionnent, la social login reste.
- **Délier** : la social login s'arrête, les mots de passe refonctionnent. Les réglages du fournisseur sont conservés.
- **Supprimer** : réglages du fournisseur et lien supprimés, les mots de passe refonctionnent.

### Compte du fournisseur inutilisable

Page de connexion → **Utiliser un code de récupération**. Vous êtes connecté, la connexion par mot de passe est réactivée, et
Paramètres → Sécurité s'ouvre. Délier ou lier un autre compte là-bas ne demande pas le
mot de passe pendant les cinq minutes suivantes.

### Codes de récupération perdus aussi

Sur l'hôte :

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Cela supprime le lien, réactive la connexion par mot de passe et déconnecte toutes les sessions. Si le mot de passe est
aussi perdu, lancez ensuite `reset-password` ([Mot de passe
oublié](/fr/guide/troubleshooting#forgot-password)). `list-users` affiche le nom d'utilisateur.

### Revenir à la version précédente {#downgrade}

Cette version ajoute la migration `0145_social_login`. Une version précédente refuse de démarrer sur une
base contenant une migration qu'elle ne connaît pas, supprimez-la donc d'abord :

1. Sauvegardez la base ([Sauvegarde et restauration](/fr/guide/backup-restore)).
2. Depuis `deploy/` :

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

3. Installez la version précédente ([Mise à jour](/fr/guide/updating)) : build depuis les sources,
   `git reset --hard <previous release commit>` puis `up -d --build` ; installation par images, le
   `deploy/` et les images de la version précédente.

Les mots de passe fonctionnent sur la version précédente quel que soit l'état de l'interrupteur. Les réglages du fournisseur et les
codes de récupération sont supprimés.

::: warning
Cela ne supprime que la migration 145. Si une version ultérieure avec davantage de migrations est installée,
restaurez plutôt la sauvegarde faite avant cette mise à jour.
:::

L'étape 2 sans l'étape 3 réinitialise la social login : la version actuelle recrée les tables vides
à son prochain démarrage.

## Erreurs {#errors}

| Message | Solution |
|---|---|
| `redirect_uri_mismatch` (Google), `AADSTS50011` (Microsoft) | L'URI de redirection enregistrée diffère de l'adresse du navigateur. Copiez-la à nouveau depuis les Paramètres. |
| *The OAuth client was not found* / `invalid_client` | ID client ou secret erroné, ou secret expiré. |
| *the provider calls itself …* | L'URL de l'émetteur diffère du `issuer` dans le `/.well-known/openid-configuration` du fournisseur. |
| *this … account is not the one linked to this instance* | Un autre compte a été choisi chez le fournisseur. |
| *this sign-in was started in another browser or has expired* | Plus de dix minutes se sont écoulées, ou la connexion s'est terminée dans un autre navigateur. Recommencez. |
| *the ID token has expired* | L'horloge du serveur est fausse. Corrigez l'heure de l'hôte (NTP). |

### Secret expiré {#secret-expired}

La connexion échoue avec `invalid_client` ou un message sur un secret expiré. Créez un nouveau secret
chez le fournisseur, puis **Modifier** dans les Paramètres, collez-le, **Enregistrez**. Le lien est conservé. Verrouillé dehors ?
Connectez-vous d'abord avec un code de récupération.
