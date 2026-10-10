# Backup e restauração

Tudo vive em um único banco PostgreSQL, então um backup é um `pg_dump`. **Configurações → Backup e restauração** no app mostra esses comandos já preenchidos para a sua implantação. Execute-os no host onde a stack está implantada; eles usam o contêiner Postgres existente, sem precisar de acesso extra.

A seção tem duas abas, cada uma dividida em **Backup** e **Restauração**:

- **Completo**: o banco inteiro, feito no host, para quando a máquina morrer.
- **Parcial**: os módulos que você marcar, em um zip, para mudar de máquina ou manter uma cópia legível.

## Backup completo

Dump simples:

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld > otw-backup-$(date +%F).sql
```

### Criptografe (recomendado)

Um dump contém seus dados em texto claro. Passe-o por `gpg` (ou `age`) para que o arquivo fique criptografado em disco. Será pedida uma senha:

```bash
docker compose --env-file .env --env-file network.env exec -T postgres \
  pg_dump -U otw opentraderworld | gpg -c --cipher-algo AES256 -o otw-backup-$(date +%F).sql.gpg
```

## Notas de segurança

- **Chaves de API e credenciais de provedores** (feeds de notícias, provedores de dados de mercado) já são criptografadas em repouso com `OTW_SECRET_KEY`, então aparecem apenas como texto cifrado no dump.
- Faça backup da **`OTW_SECRET_KEY`** (de `deploy/.env`) **separadamente**, não dentro do mesmo dump, ou esses segredos criptografados não poderão ser restaurados.
- O dump inclui **tokens de sessão** ativos. Trate o arquivo como segredo, ou apague a tabela `sessions` após restaurar e entre novamente.
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

Em um banco novo e vazio (uma stack recém-criada):

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld < otw-backup-2026-07-06.sql
```

A partir de um backup criptografado:

```bash
gpg -d otw-backup-2026-07-06.sql.gpg | \
  docker compose --env-file .env --env-file network.env exec -T postgres \
  psql -U otw opentraderworld
```

Certifique-se de que a stack restaurada use a **mesma `OTW_SECRET_KEY`** de quando o backup foi feito, ou as credenciais de provedores armazenadas ficarão ilegíveis.
