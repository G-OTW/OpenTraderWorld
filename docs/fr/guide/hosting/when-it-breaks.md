# En cas de panne

Les problèmes que les gens rencontrent vraiment, dans l'ordre où ils surviennent le plus souvent.
Pour chacun : ce que vous voyez, pourquoi, et quoi faire.

::: tip La première chose à essayer, toujours
Si l'installation s'est arrêtée, **relancez la même ligne**.
Elle se souvient de ce qui a déjà fonctionné et reprend là où elle s'était arrêtée.
Elle ne pose jamais deux fois la même question.
:::

## Pendant l'installation

### "The name … does not lead anywhere yet"

**Pourquoi :** votre adresse (par exemple `app.example.com`) n'est pas encore reliée à votre machine.

1. Connectez-vous chez l'entreprise qui vous a vendu l'adresse.
2. Ouvrez sa page **DNS** (parfois appelée **Zone** ou **Enregistrements DNS**).
3. Ajoutez un enregistrement avec exactement ce que le message a affiché :

   | Type | Nom | Valeur |
   |---|---|---|
   | `A` | le mot que montre le message (`@` pour l'adresse nue) | les chiffres que montre le message |

4. Enregistrez.
5. Attendez cinq minutes.
6. Relancez la même ligne.

::: details Le champ "Nom" est l'erreur habituelle
Pour `example.com`, saisissez `@` (certains sites veulent le champ vide).
Pour `app.example.com`, saisissez seulement `app`, pas l'adresse complète.
:::

### "The name … does not lead to this machine"

**Pourquoi :** l'adresse pointe ailleurs : une ancienne machine, ou une page de parking de l'entreprise qui l'a vendue.

1. Ouvrez la même page **DNS**.
2. Supprimez tous les autres enregistrements `A` portant ce nom.
3. Gardez uniquement celui dont la valeur est celle affichée par le message.
4. Attendez cinq minutes, puis relancez la même ligne.

Si vous l'avez modifié il y a seulement quelques minutes, répondez **oui** quand l'installateur propose d'attendre.
Il vérifie toutes les 20 secondes pendant 10 minutes au plus.

### "The address https://… is not answering yet"

**Pourquoi :** presque toujours l'une de ces deux raisons.

- Vous avez fait pointer l'adresse vers la machine il y a quelques minutes seulement. **Attendez dix minutes**, puis relancez la même ligne.
- Votre hébergeur a son propre pare-feu devant la machine, et il est fermé.

Pour ouvrir le pare-feu de l'hébergeur :

1. Ouvrez le panneau de contrôle de votre hébergeur.
2. Trouvez les réglages **Pare-feu** ou **Sécurité** de la machine.
3. Autorisez les entrées **TCP 80** et **TCP 443**.
4. Relancez la même ligne.

### "Something on this machine is already answering on port 80 / 443"

**Pourquoi :** votre hébergeur a installé un serveur web sur la machine pour vous. Il occupe la place dont OpenTraderWorld a besoin.

Collez ceci, puis relancez la même ligne :

```bash
systemctl disable --now apache2 nginx caddy 2>/dev/null; true
```

### "This machine has … MB of memory" ou "Only … MB of disk space is free"

**Pourquoi :** la machine est trop petite. OpenTraderWorld a besoin d'environ **2 Go de mémoire** et de **8 Go de disque libre**.

1. Chez votre hébergeur, passez la machine à une offre plus grande.
2. Relancez la même ligne.

### "This installer only knows Ubuntu and Debian"

**Pourquoi :** la machine a été créée avec un autre système.

1. Chez votre hébergeur, **réinstallez** (ou **reconstruisez**) la machine avec **Ubuntu 24.04** ou **Debian 13**.
2. Relancez la même ligne.

### "This needs the machine's administrator rights"

**Pourquoi :** vous êtes connecté avec un compte qui n'a pas le droit d'installer des logiciels.

Relancez la ligne avec `sudo` au milieu, exactement comme le montre le message :

```bash
curl -fsSL https://get.opentraderworld.com/configure_install.sh | sudo bash -s -- --domain app.example.com
```

## Après l'installation

### La page ne s'ouvre plus

1. Connectez-vous à votre machine.
2. Tapez :

   ```bash
   otw status
   ```

3. S'il indique **Nothing is running** ou **is not answering**, tapez :

   ```bash
   otw restart
   ```

4. Attendez une minute, puis rechargez la page.

### Je ne peux plus me connecter à la machine elle-même

**Pourquoi :** l'installateur a durci la façon dont la machine laisse entrer les gens.

- Si vous avez choisi **la clé** : connectez-vous depuis le même ordinateur que celui du jour de l'installation. Les mots de passe sont refusés volontairement.
- Si vous avez choisi **un mot de passe** : connectez-vous avec le nom de compte indiqué sur votre fiche, **pas** `root`. La connexion directe en `root` est refusée volontairement.

Vous avez perdu cet ordinateur ou ce mot de passe ? Utilisez la **console** (parfois appelée **VNC**, **rescue** ou **terminal web**) dans le panneau de contrôle de votre hébergeur. Elle fonctionne même quand l'accès normal est fermé.

### J'ai oublié le mot de passe d'OpenTraderWorld

1. Connectez-vous à votre machine.
2. Tapez (remplacez `admin` par votre nom de connexion si vous l'avez changé) :

   ```bash
   docker exec -it opentraderworld-core-1 /app/otw-core reset-password admin
   ```

3. Connectez-vous à l'application avec le mot de passe affiché. L'application vous demande d'en choisir un nouveau.

Pour revoir votre adresse et votre nom de connexion, tapez `otw card`.

### Le navigateur affiche "Non sécurisé" ou refuse la page

- Tapez l'adresse avec `https://` devant.
- Si elle a démarré il y a quelques minutes après l'installation, attendez dix minutes : le cadenas est encore en cours d'émission.
- Sur une installation domestique (pas un serveur loué), voir [Dépannage](/fr/guide/troubleshooting).

### La machine est pleine

**Pourquoi :** les sauvegardes nocturnes et l'historique de prix téléchargé occupent de la place avec le temps.

1. Tapez `otw status` pour voir l'espace restant.
2. Chez votre hébergeur, donnez un disque plus grand à la machine.
3. Tapez `otw restart`.

## Toujours bloqué ?

1. Connectez-vous à votre machine.
2. Tapez :

   ```bash
   otw report
   ```

3. Il écrit un fichier et affiche où il se trouve. Le fichier ne contient aucun mot de passe.
4. Ouvrez un ticket sur [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues), dites ce que vous faisiez, et joignez ce fichier.
