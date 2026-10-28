# Installer Docker

OpenTraderWorld est livré sous forme de conteneurs Docker, et c'est actuellement la **seule façon prise en charge de l'exécuter**. Une installation native (exécuter directement sur l'hôte le core Rust, PostgreSQL et le frontend) est possible si vous savez ce que vous faites, mais elle n'est **ni recommandée ni documentée**. Docker est privilégié volontairement :

- **Non intrusif** : rien n'est installé sur votre système à part Docker lui-même. L'application, la base de données et le proxy vivent dans des conteneurs ; vos données vivent dans des volumes nommés. Tout supprimer revient à `docker compose down -v` puis à effacer le dossier.
- **Identique partout** : la même pile tourne sans changement sur macOS, Linux et Windows.
- **Rapide à mettre à jour** : actualisez le dépôt et récupérez les nouvelles images (voir [Mise à jour](/fr/guide/updating)) ; un conteneur défaillant est recréé en quelques secondes sans toucher à vos données.

Si vous avez déjà Docker, passez directement à [Installation](/fr/guide/install).

## macOS

Installez **Docker Desktop** :

- Téléchargez-le depuis [docker.com](https://www.docker.com/products/docker-desktop/) (choisissez Apple Silicon ou Intel), ouvrez le `.dmg` et glissez Docker dans Applications, ou avec Homebrew :

  ```bash
  brew install --cask docker
  ```

- Lancez **Docker** une fois depuis Applications et laissez-le finir de démarrer (l'icône de baleine dans la barre de menus cesse de s'animer).

Docker Compose est inclus.

## Windows

Installez **Docker Desktop** avec le backend WSL 2 :

1. Prérequis : Windows 10/11 64 bits avec **WSL 2**, activé si besoin depuis un PowerShell administrateur : `wsl --install`, puis redémarrez.
2. Installez Docker Desktop depuis [docker.com](https://www.docker.com/products/docker-desktop/), ou :

   ```powershell
   winget install Docker.DockerDesktop
   ```

3. Lancez Docker Desktop et gardez le réglage par défaut *Use WSL 2*.

Exécutez les commandes d'OpenTraderWorld depuis n'importe quel terminal (PowerShell ou un shell WSL). Docker Compose est inclus.

## Linux

Sur un poste de travail ou un serveur sans écran, installez **Docker Engine** (Desktop inutile). Le script pratique fonctionne sur toutes les grandes distributions :

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER   # run docker without sudo
newgrp docker                    # or log out and back in
sudo systemctl enable --now docker
```

Vous préférez les paquets de votre distribution ? Consultez les [instructions officielles par distribution](https://docs.docker.com/engine/install/). Les installations récentes d'Engine incluent le plugin Compose.

## Vérifier

```bash
docker --version
docker compose version
docker run --rm hello-world
```

Les trois réussissent → vous êtes prêt.

## Déployer OpenTraderWorld

Une seule commande, puis suivez les invites :

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

Le déroulé complet (sens des invites, options, alternative manuelle, vérification du résultat) se trouve sur la page [Installation](/fr/guide/install).

::: info Images préconstruites
L'installation **récupère des images préconstruites** depuis Docker Hub : aucune compilation, aucune chaîne d'outils Rust/Node. La compilation depuis les sources reste disponible pour le développement (`install.sh --build`, ou `./setup.sh --build` depuis un clone).
:::
