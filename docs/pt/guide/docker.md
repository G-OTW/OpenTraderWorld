# Obtenha o Docker

O OpenTraderWorld é distribuído como um conjunto de contêineres Docker, e essa é atualmente a **única forma suportada de executá-lo**. Uma instalação nativa (rodando o core em Rust, o PostgreSQL e o frontend diretamente no host) é possível se você sabe o que está fazendo, mas **não é recomendada nem documentada**. O Docker é priorizado de propósito:

- **Não intrusivo**: nada é instalado no seu sistema além do próprio Docker. O app, o banco e o proxy vivem em contêineres; seus dados vivem em volumes nomeados. Remover tudo é `docker compose down -v` mais apagar a pasta.
- **Idêntico em todo lugar**: a mesma stack roda sem alterações no macOS, Linux e Windows.
- **Rápido de atualizar**: atualize o repositório e baixe as novas imagens (veja [Atualização](/pt/guide/updating)); um contêiner quebrado é recriado em segundos sem tocar nos seus dados.

Se você já tem o Docker, vá direto para a [Instalação](/pt/guide/install).

## macOS

Instale o **Docker Desktop**:

- Baixe em [docker.com](https://www.docker.com/products/docker-desktop/) (escolha Apple Silicon ou Intel), abra o `.dmg` e arraste o Docker para Aplicativos, ou com Homebrew:

  ```bash
  brew install --cask docker
  ```

- Abra o **Docker** uma vez a partir de Aplicativos e deixe-o terminar de iniciar (o ícone da baleia na barra de menus para de animar).

O Docker Compose está incluído.

## Windows

Instale o **Docker Desktop** com o backend WSL 2:

1. Requisitos: Windows 10/11 64 bits com **WSL 2**, ativado em um PowerShell de administrador se necessário: `wsl --install`, depois reinicie.
2. Instale o Docker Desktop a partir de [docker.com](https://www.docker.com/products/docker-desktop/), ou:

   ```powershell
   winget install Docker.DockerDesktop
   ```

3. Abra o Docker Desktop e mantenha a configuração padrão *Use WSL 2*.

Execute os comandos do OpenTraderWorld em qualquer terminal (PowerShell ou um shell WSL). O Docker Compose está incluído.

## Linux

Em um desktop ou servidor sem interface, instale o **Docker Engine** (o Desktop não é necessário). O script de conveniência funciona em todas as principais distribuições:

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER   # run docker without sudo
newgrp docker                    # or log out and back in
sudo systemctl enable --now docker
```

Prefere os pacotes da sua distribuição? Veja as [instruções oficiais por distribuição](https://docs.docker.com/engine/install/). As instalações recentes do Engine incluem o plugin Compose.

## Verificar

```bash
docker --version
docker compose version
docker run --rm hello-world
```

Os três funcionam → você está pronto.

## Implantar o OpenTraderWorld

Um comando, depois siga as perguntas:

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

O passo a passo completo (o que significam as perguntas, opções, alternativa manual, verificação do resultado) está na página de [Instalação](/pt/guide/install).

::: info Imagens pré-compiladas
A instalação **baixa imagens pré-compiladas** do Docker Hub: sem build, sem toolchain Rust/Node. A compilação a partir do código-fonte continua disponível para desenvolvimento (`install.sh --build`, ou `./setup.sh --build` a partir de um clone).
:::
