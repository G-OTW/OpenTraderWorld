# Atualização

O OpenTraderWorld avisa quando há uma nova versão disponível em **Configurações → Atualizar app** (ele consulta o GitHub). O app **não consegue se atualizar sozinho por design** (ele roda sem acesso a shell ou Docker, para manter a superfície de ataque pequena), então as atualizações são alguns comandos no host.

## Antes de atualizar

1. **Faça um backup do banco de dados**: veja [Backup e restauração](/pt/guide/backup-restore).
2. Leia rapidamente as notas da versão em busca de mudanças incompatíveis.

## Qual instalação você tem?

Olhe o seu diretório de instalação:

- Apenas `deploy/` dentro, sem `.git`: **instalação por imagens** (o instalador de um comando, ou
  `setup.sh` sem `--build`). Este é o padrão.
- `core/`, `frontend/` e um `.git`: **build a partir do código-fonte** (`install.sh --build`, ou um clone
  mais `./setup.sh --build`).

## Instalação por imagens

Nada é compilado e não há checkout git, então atualize o `deploy/` a partir da versão
(é lá que as novas tags de imagem ficam fixadas), depois baixe e reinicie:

```bash
cd /path/to/opentraderworld
TMP=$(mktemp -d)
curl -fsSL https://codeload.github.com/G-OTW/OpenTraderWorld/tar.gz/master | tar -xz -C "$TMP"
cp -R "$TMP"/*/deploy/. deploy/
rm -rf "$TMP"
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  pull
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d
```

Sua configuração é preservada: `.env`, `network.env` e `dns.env` não fazem parte da
versão, então a cópia nunca os sobrescreve. Todo o resto em `deploy/` é substituído pela
nova versão, e é essa a ideia: edições locais em `docker-compose.yml` ou `Caddyfile`
são perdidas, guarde-as em um patch se precisar delas.

## Build a partir do código-fonte

Atualize o checkout e depois recompile:

```bash
cd /path/to/OpenTraderWorld
git fetch origin
git reset --hard origin/master
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d --build
```

::: warning Use `git reset --hard`, não `git pull`
Cada versão é publicada como um snapshot novo do repositório, então o `git pull` informa
"divergent branches" e falha. O `git reset --hard origin/master` deixa seu checkout
idêntico à nova versão. Seus dados e sua configuração não são tocados: eles vivem em
volumes Docker e em `.env` / `network.env`, que não são rastreados pelo git. Se você editou
arquivos rastreados localmente, guarde-os antes (`git stash`).
:::

Para baixar as imagens publicadas em vez de recompilar a partir do seu checkout, adicione
`-f deploy/docker-compose.images.yml` e use `pull` + `up -d` como na instalação por imagens.

É só isso:

- Os contêineres são recriados com a nova versão e reiniciados.
- **As migrações do banco rodam automaticamente** na primeira inicialização do novo contêiner core.
- Seus dados não são tocados: eles vivem em volumes Docker, independentes das imagens.

O app fica brevemente offline enquanto os contêineres são recriados. Os comandos exatos (com os seus caminhos configurados) também são mostrados em **Configurações → Atualizar app**.

## Pontual: OAuth para AI agents (0.0.16) {#oauth-caddyfile}

O login OAuth para clientes MCP precisa que os documentos de descoberta em `/.well-known/oauth-*`
alcancem o core. O procedimento de instalação por imagens acima e os builds a partir do código-fonte trazem o novo
`deploy/Caddyfile`, assim como as novas instalações. Uma instalação atualizada com `otw update` mantém
o Caddyfile antigo: tudo funciona, exceto o OAuth, até você adicionar este bloco, logo antes da
linha `# Everything else → static frontend`:

```
	handle /.well-known/oauth-* {
		header Content-Security-Policy "default-src 'none'; frame-ancestors 'none'"
		reverse_proxy core:8080
	}
```

Depois recrie o Caddy (um reload não basta: a maioria dos editores substitui o arquivo, e o
contêiner continua com o antigo):

```bash
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d --force-recreate caddy
```

## Pontual: aposentar a senha de bootstrap {#retire-bootstrap-password}

Se o core registrar isto na inicialização, é um recado para você:

```
OTW_ADMIN_PASSWORD is still set but the admin account already exists.
```

O `setup.sh` cria o admin a partir de `deploy/.env` na primeira inicialização e depois esvazia essa linha. Em
uma instalação feita antes disso, o valor continua no arquivo, e no ambiente do
contêiner, onde o `docker inspect` o entrega a qualquer pessoa que alcance o daemon do Docker.
Neste ponto ele não concede nada, é apenas mais uma cópia de uma senha em disco.

```bash
sed -i.bak 's/^OTW_ADMIN_PASSWORD=.*/OTW_ADMIN_PASSWORD=/' deploy/.env
chmod 600 deploy/.env && rm -f deploy/.env.bak
```

Depois recrie o core para que ele também saia do ambiente:

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d core
```

Mantenha `OTW_ADMIN_USER`: uma senha vazia torna todo o bootstrap um no-op, que é o que você
quer, e a linha continua documentando como uma instalação sem interface cria sua primeira conta.
