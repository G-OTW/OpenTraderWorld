# Backup e restauração

Tudo vive em um único banco PostgreSQL, então um backup é um `pg_dump`. **Configurações → Backup e restauração** no app mostra os mesmos comandos. Execute-os no host onde a stack está implantada, a partir do diretório de instalação (o que contém `deploy/`); eles usam o contêiner Postgres existente, sem precisar de acesso extra.

A seção tem duas abas, cada uma dividida em **Backup** e **Restauração**:

- **Completo**: o banco inteiro, feito no host, para quando a máquina morrer.
- **Parcial**: os módulos que você marcar, em um zip, para mudar de máquina ou manter uma cópia legível.

## Backup completo

Dump simples:

```bash
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner > otw-backup-$(date +%F).sql
```

### Criptografe (recomendado)

Um dump contém seus dados em texto claro. Passe-o por `gpg` (ou `age`) para que o arquivo fique criptografado em disco. Será pedida uma senha; `GPG_TTY` e `--pinentry-mode loopback` permitem que esse pedido chegue ao seu terminal enquanto o dump alimenta a entrada do gpg:

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner \
  | gpg --symmetric --cipher-algo AES256 --pinentry-mode loopback \
  > otw-backup-$(date +%F).sql.gpg
```

### Backup automático {#automatic-backup}

Quando um script de backup no host informa o app (o [instalador de servidor](#server-installs) configura um), a aba **Completo** abre com um cartão **Backup automático**: há quanto tempo foi o último backup, para onde foi a cópia, se ela foi verificada, o agendamento e o tamanho. Ele vira um aviso quando a última execução falhou, ou após 48 horas sem nenhuma. Sem esse script não há cartão, e os comandos acima são tudo.

## Notas de segurança

- **Chaves de API e credenciais de provedores** (feeds de notícias, provedores de dados de mercado) já são criptografadas em repouso com `OTW_SECRET_KEY`, então aparecem apenas como texto cifrado no dump.
- Faça backup da **`OTW_SECRET_KEY`** (de `deploy/.env`) **separadamente**, não dentro do mesmo dump, ou esses segredos criptografados não poderão ser restaurados.
- O dump inclui **tokens de sessão** ativos. Trate o arquivo como segredo, ou esvazie a tabela `sessions` após restaurar (`DELETE FROM sessions;`) e entre novamente. Não apague a tabela: o login precisa dela.
- Guarde o backup criptografado **fora da máquina** e faça rodízio das cópias mais antigas.

## Parcial (por módulo)

A aba **Parcial** pega os módulos que você marcar e entrega **um arquivo zip**, para mover um diário para outra instância ou manter uma cópia legível. Ela roda com o app no ar, ao contrário do backup completo.

### Backup parcial

1. Abra **Configurações → Backup e restauração → Parcial → Backup**.
2. Marque os módulos que você quer. Cada um mostra o número de linhas e o tamanho, o conjunto marcado é totalizado abaixo da lista, e *O que há na seleção* detalha tabela por tabela. As barras históricas começam desmarcadas: são de longe a maior tabela, e podem ser baixadas de novo do seu provedor.
3. Deixe **Incluir credenciais de provedores armazenadas** desligado, a menos que você saiba por que precisa. Esses valores são criptografados com a `OTW_SECRET_KEY` desta instância e ficam ilegíveis em qualquer outro lugar.
4. Clique em **Baixar dados selecionados**. Você recebe `otw-data-YYYY-MM-DD.zip`.

Dentro do zip: `manifest.json` (o que ele contém, qual versão o escreveu) e um `tables/<name>.jsonl` por tabela, um objeto JSON por linha. Qualquer ferramenta consegue lê-lo.

### Restauração parcial

1. Abra **Configurações → Backup e restauração → Parcial → Restauração** na instância de destino e escolha o arquivo.
2. O arquivo é lido assim que você o escolhe, e nada é gravado: você vê a versão que o escreveu e, por módulo e por tabela, quantas linhas ele traz em comparação com quantas existem agora. Uma tabela muito grande é informada como estimativa, marcada com `~`. Um arquivo que esta instância recusaria (danificado, ou de uma versão mais nova) é recusado neste ponto, antes de você se comprometer com qualquer coisa.
3. Escolha como ele deve se encontrar com os dados já existentes:
   - **Adicionar o que falta** mantém tudo o que existe e adiciona apenas as linhas que ainda não estão lá. Nada é sobrescrito.
   - **Substituir** apaga os dados de todos os módulos do arquivo e depois carrega a versão do arquivo. Pede que você digite `REPLACE`, e salva antes uma cópia dos dados atuais.

   A linha abaixo da escolha transforma essas contagens no que vai acontecer. Substituir é exato: *apaga as N linhas daqui, coloca as M linhas do arquivo no lugar*. Mesclar só pode dar um teto, *adiciona até M linhas*: uma linha cuja chave já existe é ignorada, e só a própria carga sabe quantas são.
4. Clique em **Carregar este arquivo**.

Tudo acontece em uma única transação: se qualquer parte falhar, nada é alterado.

::: warning Um arquivo de uma versão mais nova é recusado
Carregar um pacote escrito por uma versão mais nova é recusado em vez de tentado. Atualize a instância primeiro, depois carregue.
:::

As linhas mantêm seus identificadores originais, e o arquivo inteiro é lido em memória, então uma seleção muito grande (barras históricas, em geral) é recusada com uma mensagem apontando de volta para o backup com `pg_dump` acima. Essa é a ferramenta certa para "tudo, inclusive o que eu nunca olho".

## Restauração completa

Um dump só se restaura em um banco vazio. O core roda suas migrações a cada inicialização, então um banco no qual ele já iniciou (qualquer stack em execução, inclusive uma que o `setup.sh` acabou de criar) entra em conflito com o dump. Pare o core, substitua o banco por um vazio, carregue o dump e inicie o core de novo:

```bash
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q < otw-backup-YYYY-MM-DD.sql
docker compose -f deploy/docker-compose.yml start core
```

A linha `DROP DATABASE` apaga o que a stack contém agora: faça um dump antes, se isso importar. `ON_ERROR_STOP=1` para no primeiro erro em vez de carregar meio dump.

A partir de um backup criptografado:

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
gpg -d otw-backup-YYYY-MM-DD.sql.gpg \
  | docker compose -f deploy/docker-compose.yml exec -T postgres \
    psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q
docker compose -f deploy/docker-compose.yml start core
```

Certifique-se de que a stack restaurada use a **mesma `OTW_SECRET_KEY`** de quando o backup foi feito, ou as credenciais de provedores armazenadas ficarão ilegíveis.

## Instalações em servidor {#server-installs}

Uma máquina configurada com o [instalador de servidor](/pt/guide/hosting/when-it-breaks#installer-options) tem o comando `otw`, que faz tudo isso por você:

| Comando | O que faz |
|---|---|
| `otw backup` | Faz um backup agora em `/var/backups/otw` e verifica se o arquivo está completo. |
| `otw restore` | Lista os backups da máquina, pergunta qual usar e o restaura depois que você digita `RESTORE`. Antes faz um backup completo dos dados atuais, depois reinicia o core. |

O instalador também agenda o `otw backup` todas as noites por volta das 03:20, o que acende o cartão [Backup automático](#automatic-backup). Uma cópia noturna deixa de fora as duas tabelas que podem ser baixadas de novo (o catálogo de instrumentos e as barras históricas); a cópia de domingo é completa. As últimas 10 cópias noturnas e 2 completas são mantidas. Elas ficam na máquina: copie-as para outro lugar para sobreviver a um servidor morto.
