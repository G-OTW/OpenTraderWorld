# Instalação

Auto-hospede o OpenTraderWorld na sua própria máquina ou servidor. Leva cerca de 5 minutos.

::: info Somente em contêineres (por enquanto)
O OpenTraderWorld roda como uma stack Docker Compose, a única implantação suportada. Uma instalação nativa é possível, mas não recomendada: o Docker mantém a instalação não intrusiva (tudo vive em contêineres e volumes) e rápida de reconstruir. Veja [Obtenha o Docker](/pt/guide/docker) para entender o porquê e para os passos de instalação por sistema operacional.
:::

## Requisitos

- **Docker** com Docker Compose. Não tem? [Obtenha o Docker](/pt/guide/docker) cobre macOS, Windows e Linux em poucos comandos.
- Linux, macOS ou Windows.
- Uma porta livre (**5454** por padrão; portas **80** + **443** para os modos HTTPS). Você pode alterá-la durante a configuração.

Verifique se o Docker está pronto:

```bash
docker --version
docker compose version
```

## Instalação com um comando (recomendada)

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

O instalador verifica se o Docker está pronto, baixa os arquivos de implantação (apenas o diretório `deploy/`, sem código-fonte, sem toolchain) em `./opentraderworld`, e depois passa para a configuração guiada abaixo, que **baixa as imagens pré-compiladas** do Docker Hub.

As opções vão depois de `bash -s --`:

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash -s -- --dir ~/otw
```

| Opção | Padrão | Notas |
|---|---|---|
| `--dir <path>` | `./opentraderworld` | Diretório de instalação. Recusa um diretório não vazio ou uma instalação existente. |
| `--ref <ref>` | `master` | Branch ou tag a instalar. |
| `--build` | desligado | Clona o código-fonte completo e compila as imagens localmente em vez de baixá-las (requer `git` e a toolchain). |

## A partir de um clone git (alternativa)

```bash
git clone https://github.com/G-OTW/OpenTraderWorld.git
cd OpenTraderWorld/deploy
./setup.sh
```

## A configuração guiada

Os dois caminhos acima executam `deploy/setup.sh`. Ele faz algumas perguntas, gera segredos fortes, escreve a configuração e inicia tudo.

Ele pergunta:

| Pergunta | Padrão | Notas |
|---|---|---|
| **Modo de rede** | `1` (localhost) | `1` somente esta máquina · `2` LAN em HTTP simples · `3` LAN em HTTPS com certificado real · `4` internet pública no seu próprio domínio. Pode ser alterado depois, veja [Rede e acesso remoto](/pt/config/network). |
| **Usuário admin** | `admin` | A conta de administrador é criada para você; uma senha forte é gerada e exibida **uma única vez**. |
| **Porta HTTP** | `5454` | Somente modos 1 e 2; os modos 3 e 4 usam 80 + 443. |
| **Domínio DuckDNS / token / IP da LAN** | nenhum | Somente modo 3. |
| **Domínio público** | nenhum | Somente modo 4, e ele já deve resolver para este servidor. |
| **Nível de log** | `info` | `trace` / `debug` / `info` / `warn` / `error`. |

Os **segredos do banco e da sessão são gerados automaticamente**, então você nunca os digita. Eles são gravados em `deploy/.env` (permissões de arquivo `600`, nunca versionado no git).

No final, deixe o script iniciar a stack: ele espera a API, **cria sua conta de administrador** e exibe a senha gerada **uma única vez**, então copie-a antes de fechar o terminal.

Por padrão, a configuração **baixa as imagens pré-compiladas** do Docker Hub: sem toolchain Rust ou Node, primeira inicialização em poucos minutos. Execute `./setup.sh --build` para compilar os três serviços a partir do código-fonte (desenvolvimento, alterações locais).

::: tip Servidores sem interface
O admin é criado pelo próprio core na primeira inicialização (a partir de `deploy/.env`), então você não precisa de um navegador no servidor. Anote a senha exibida e entre a partir de qualquer máquina que alcance o app.
:::

::: warning Reinstalar sobre dados anteriores
Se existirem volumes Docker de uma instalação anterior, a configuração oferece apagá-los. O padrão é **Não** em todos os casos: apagar (e perder o banco anterior) só acontece com um `y` explícito. Segredos novos sobre um volume de banco antigo não funcionam, então recusar aborta a configuração em vez de iniciar uma stack quebrada.
:::

## Instalação manual (alternativa)

Se você prefere configurar à mão:

```bash
cd deploy
cp .env.example .env
```

Edite o `.env` e defina pelo menos:

- `POSTGRES_PASSWORD`: uma senha forte
- `DATABASE_URL`: deve conter a mesma senha, por exemplo `postgres://otw:YOUR_PASSWORD@postgres:5432/opentraderworld`
- `SESSION_SECRET`: uma string aleatória longa

Depois inicie a stack. Por padrão, isso **baixa as imagens pré-compiladas** do Docker Hub (sem necessidade de toolchain Rust ou Node):

```bash
docker compose -f docker-compose.yml -f docker-compose.images.yml \
  --env-file .env --env-file network.env up -d
```

::: details Compilar a partir do código-fonte
Para desenvolvimento, ou para rodar alterações locais, omita o override de imagens e compile os três serviços você mesmo (requer a toolchain; a compilação do Rust é lenta):

```bash
docker compose --env-file .env --env-file network.env up --build -d
```
:::

## Criar o admin (somente instalações manuais)

Se você usou `./setup.sh` e deixou ele iniciar a stack, **seu admin já existe**, então pule para a próxima seção.

Caso contrário, abra o app no navegador (`http://localhost:5454` por padrão). Na primeira visita, o OpenTraderWorld detecta que ainda não há admin e mostra o **assistente de configuração**: escolha um nome de usuário e uma senha (mín. 8 caracteres), envie, e você chega ao dashboard. As senhas são armazenadas com hash argon2.

::: details Criar o admin pela CLI (sem interface, sem navegador)
Chame o endpoint de primeira execução de dentro da stack: ele funciona independentemente da sua interface de bind ou do modo TLS, e recusa (HTTP 409) se já existir um admin:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T caddy \
  wget -qO- --header=Content-Type:application/json \
  --post-data='{"username":"admin","password":"CHOOSE-A-STRONG-ONE"}' \
  http://core:8080/api/setup
```
:::

## Verificar se está rodando

- App: `http://localhost:5454` (ou a porta/domínio escolhido)
- Health check: `http://localhost:5454/api/health` → `{"status":"ok","service":"otw-core",...}`

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## Operações do dia a dia

Execute estes a partir de `deploy/`:

| Ação | Comando |
|---|---|
| Iniciar | `docker compose up -d` |
| Parar | `docker compose down` |
| Ver logs | `docker compose logs -f` |
| Baixar imagens mais novas | `docker compose -f docker-compose.yml -f docker-compose.images.yml pull && docker compose up -d` |
| Recompilar após mudar o código (build a partir do código-fonte) | `docker compose up --build -d` |
| Parar **e apagar todos os dados** | `docker compose down -v` |

Seus dados vivem em volumes nomeados do Docker e **persistem** entre `up`/`down`. Eles só são apagados com `down -v`.

## Próximos passos

- [Primeiros passos](/pt/guide/first-steps): entrar, definir padrões, instalar módulos.
- [Rede e acesso remoto](/pt/config/network): acesse o app de outros dispositivos, HTTPS na LAN, exposição pública.
- Algo deu errado? Veja [Solução de problemas](/pt/guide/troubleshooting).
