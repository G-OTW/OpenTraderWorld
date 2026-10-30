# Instalação

Auto-hospede o OpenTraderWorld na sua própria máquina ou servidor. Leva cerca de 5 minutos.

::: info Somente em contêineres (por enquanto)
O OpenTraderWorld roda como uma stack Docker Compose, a única implantação suportada. Uma instalação nativa é possível, mas não recomendada: o Docker mantém a instalação não intrusiva (tudo vive em contêineres e volumes) e rápida de reconstruir. Veja [Obtenha o Docker](/pt/guide/docker) para entender o porquê e para os passos de instalação por sistema operacional.
:::

## Requisitos

- **Docker** com Docker Compose. Não tem? [Obtenha o Docker](/pt/guide/docker) cobre macOS, Windows e Linux em poucos comandos.
- Linux, macOS ou Windows.
- Uma porta livre (**5454** por padrão; portas **80** + **443** para os modos HTTPS). Você pode alterá-la durante a configuração. Os modos localhost e LAN em HTTP simples também usam a porta **8443** em `127.0.0.1`.

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
| `--yes` | desligado | Repassado ao `setup.sh`: aceita todos os padrões quando não há terminal para perguntar. |
| `--wipe-volumes` | desligado | Repassado ao `setup.sh`: remove os volumes de uma instalação anterior sem perguntar. **Isso apaga os dados dessa instalação.** |

`OTW_DIR` e `OTW_REF` definem os mesmos valores que `--dir` e `--ref` (uma opção vence a sua variável), e `OTW_REPO` o repositório GitHub (padrão `G-OTW/OpenTraderWorld`).

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

Ele também aceita estas opções:

| Opção | Notas |
|---|---|
| `--build` | Compila as imagens a partir do código-fonte em vez de baixá-las (veja abaixo). |
| `--yes` | Aceita todos os padrões quando não há terminal para perguntar. |
| `--wipe-volumes` | Remove os volumes de uma instalação anterior sem perguntar. **Isso apaga os dados dessa instalação.** |
| `--non-interactive` | Igual a `OTW_NONINTERACTIVE=1`: lê todas as respostas do ambiente, nunca de um terminal. Uma variável não definida usa o padrão. |

As variáveis do modo não interativo, uma por pergunta: `NET_CHOICE` (`1` a `4`), `OTW_HTTP_PORT`, `ADMIN_USER`, `OTW_ADMIN_PASSWORD` (gerada se não definida), `OTW_LOG`, `DUCK_DOMAIN`, `DUCK_TOKEN` e `LAN_IP` (modo 3), `OTW_DOMAIN` (modo 4), e `OVERWRITE`, `CLEAN_VOLS`, `WIPE_VOL`, `START_NOW` (`y` ou `N`) para as confirmações.

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
- `OTW_SECRET_KEY`: outra string aleatória longa, a chave que criptografa as credenciais armazenadas. Mantenha-a estável: alterá-la depois as torna ilegíveis.

O Compose também precisa dos dois arquivos de rede que o `setup.sh` teria escrito. Para o modo localhost na porta 5454:

```bash
cat > network.env <<'EOF'
OTW_BIND=127.0.0.1
OTW_HTTPS_BIND=127.0.0.1
OTW_HTTP_PORT=5454
OTW_HTTPS_PORT=8443
OTW_DOMAIN=:80
EOF
printf 'OTW_ACME_GLOBAL=\n' > dns.env
```

Os dois são reescritos depois a partir de **Configurações → Rede**, veja [Rede e acesso remoto](/pt/config/network).

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

Caso contrário, abra o app no navegador (`http://localhost:5454` por padrão). Na primeira visita, o OpenTraderWorld detecta que ainda não há admin e mostra o **assistente de configuração**: escolha um nome de usuário e uma senha, envie, e você chega ao dashboard. A senha precisa de pelo menos 12 caracteres, e um caractere repetido, uma sequência contada, uma senha que contenha o nome de usuário ou que esteja numa lista de vazamentos conhecidos é recusada. As senhas são armazenadas com hash argon2.

Nos modos HTTPS (LAN + HTTPS, público), o assistente também pede um **token de configuração**, para que ninguém que apenas encontre o endereço possa reivindicar a conta primeiro. O core o exibe no seu log na inicialização: `docker compose logs core`.

::: details Criar o admin pela CLI (sem interface, sem navegador)
Chame o endpoint de primeira execução de dentro da stack: ele funciona qualquer que seja a sua interface de bind, e recusa (HTTP 409) se já existir um admin. Nos modos HTTPS, adicione o token de configuração do log do core como mais um cabeçalho, `--header=X-OTW-Setup-Token:<token>`, ou ele responde 403 `setup_token_required`:

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
- Health check: `http://localhost:5454/api/health` → `{"status":"ok"}` (com sessão iniciada, também retorna a versão, o tempo de atividade e o estado de cada serviço)

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## Operações do dia a dia

Execute estes a partir de `deploy/`. Todo comando recebe os dois arquivos de ambiente, e uma instalação por imagens também recebe o override de imagens, então comece pela linha que corresponde à sua instalação:

```bash
# image install (the default)
docker compose -f docker-compose.yml -f docker-compose.images.yml --env-file .env --env-file network.env
# source build
docker compose --env-file .env --env-file network.env
```

| Ação | Acrescente |
|---|---|
| Iniciar | `up -d` |
| Parar | `down` |
| Ver logs | `logs -f` |
| Recompilar após mudar o código (build a partir do código-fonte) | `up --build -d` |
| Parar **e apagar todos os dados** | `down -v` |

Para passar a uma versão mais nova, siga [Atualização](/pt/guide/updating): as tags das imagens são fixadas em `docker-compose.images.yml`, então um simples `pull` baixa a versão que você já roda.

Seus dados vivem em volumes nomeados do Docker e **persistem** entre `up`/`down`. Eles só são apagados com `down -v`.

## Próximos passos

- [Primeiros passos](/pt/guide/first-steps): entrar, definir padrões, instalar módulos.
- [Rede e acesso remoto](/pt/config/network): acesse o app de outros dispositivos, HTTPS na LAN, exposição pública.
- Algo deu errado? Veja [Solução de problemas](/pt/guide/troubleshooting).
