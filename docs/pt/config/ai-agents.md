# AI agents (MCP)

O OpenTraderWorld inclui um **servidor MCP** integrado para que AI agents (qualquer cliente compatível com [MCP](https://modelcontextprotocol.io)) possam ler e atualizar seus módulos por um gateway controlado. Um agent pode registrar trades do diário para você, resumir seus feeds de notícias, adicionar tarefas, consultar os resultados dos seus backtests e assim por diante.

**Ele vem desligado por padrão.** Nada escuta agents até você ativá-lo.

::: tip Procurando o assistente de chat do app?
Esta página trata de agents **externos** que se conectam *ao* OpenTraderWorld. Se você quer o assistente de chat integrado que vive dentro do app (traga seu próprio provedor), veja o [módulo Agent](/pt/modules/agent): ele pode *usar* este mesmo gateway para alcançar seus dados.
:::

## Modelo de segurança

Várias camadas, todas precisam passar:

1. **Interruptor global**: o endpoint MCP fica desativado até você ativá-lo em **Configurações → MCP**. Você pode preparar tokens enquanto ele está desligado; toda requisição de agent é rejeitada até a ativação.
2. **Tokens bearer**: um por agent ou caso de uso. Os tokens são armazenados com **hash** e mostrados apenas **uma vez** na criação; tentativas falhas são limitadas. Revogue um token quando quiser.
3. **Permissões de módulo por token**: cada token concede *nenhum acesso*, *leitura*, *leitura + escrita* ou *total (leitura + escrita + exclusão)* **por módulo**. Os agents só descobrem os módulos que você concedeu.
4. **Lista de permissões rígida**: operações de conta, rede, segredos, armazenamento de arquivos e apagar dados **nunca são expostas** aos agents, independentemente das permissões.

::: tip A lista de permissões é deliberada, não automática
Um endpoint só é acessível aos agents porque alguém o adicionou ao catálogo à mão. Um novo módulo, ou uma nova rota em um já existente, fica **invisível para todo agent** até essa entrada existir, então o gateway nunca se amplia por acidente conforme o app cresce. O gerenciamento de personas e skills fica de fora de propósito: nenhum agent, e nenhum conteúdo que um agent leia, pode editar uma persona ou ampliar sua prateleira de skills.
:::

::: tip Versões funcionam como commits
Com o versionamento ligado em **Configurações → Versões**, um agent com acesso a **Editor** ou **Backtest** pode salvar uma versão de um documento ou estratégia depois de atualizá-lo, com uma nota dizendo o que mudou, e listar, ler, restaurar ou excluir versões. Ele pode ligar o versionamento por arquivo ou estratégia, mas não os interruptores globais das Configurações.
:::

::: warning O Automator concede escrita, não armamento
Conceder **Automator** permite que um agent leia seus workflows, crie um, escreva seu grafo e o teste. **Não** permite colocar um em operação: um grafo que um agent salva chega como rascunho que você adota a partir do editor, ele não pode anexar o token de acesso de um workflow, e não pode executar um workflow nem mexer em um agendamento. Um workflow roda sob seu próprio token e não o de quem chama, então escrever um grafo e armá-lo são duas concessões separadas. Veja [a página do módulo](/pt/modules/automator#deixar-um-agent-montar-um-workflow).
:::

## Ative e crie um token

1. Vá em **Configurações → MCP** e ligue.
2. **Novo token**: dê a ele o nome do cliente (por exemplo `My Agent`), defina as permissões por módulo (ou use *Toda leitura* / *Toda leitura+escrita* / *Todo total* como ponto de partida).
3. **Copie o token imediatamente**: ele é mostrado apenas uma vez.

**Permitir acesso externo** é uma caixa separada no mesmo diálogo. Ela não concede nada extra: apenas permite que esse token sustente um vínculo de chat em [Controle externo](/pt/config/external-control), onde uma mensagem do Telegram, Slack ou Discord roda sob esses mesmos níveis por módulo.

O diálogo de criação também mostra um **trecho de configuração pronto para colar**, com uma aba por família de cliente: o endpoint bruto + cabeçalho, um bloco JSON `mcpServers` (Cursor, Cline, Windsurf, VS Code…) e uma linha de comando `claude mcp add`. Os mesmos trechos continuam disponíveis abaixo da tabela de tokens com `<TOKEN>` como placeholder, para configurar uma segunda máquina depois.

## Conecte um cliente

O endpoint fala **MCP sobre Streamable HTTP** em:

```
POST http://<your-host>/api/mcp
Authorization: Bearer <TOKEN>
```

Qualquer cliente compatível funciona. Exemplo para uma configuração MCP:

```json
{
  "mcpServers": {
    "opentraderworld": {
      "type": "http",
      "url": "http://localhost:5454/api/mcp",
      "headers": { "Authorization": "Bearer <TOKEN>" }
    }
  }
}
```

Troque a URL pelo seu domínio se você usa um modo LAN/HTTPS.

::: tip Instalações somente em localhost
Se o app é acessível apenas em `localhost` (o modo de rede padrão), os agents precisam rodar **na mesma máquina**.
:::

## Conecte com OAuth (claude.ai, ChatGPT)

Alguns clientes não conseguem guardar um token fixo: os connectors do claude.ai e do ChatGPT só entram com OAuth. Para eles, ligue o **login OAuth** no final de **Configurações → MCP** (ele pede sua senha) e entregue ao cliente apenas a URL do servidor, `https://<your-domain>/api/mcp`, sem token.

1. O cliente se registra sozinho e abre uma página de consentimento na sua instância (entre primeiro, se necessário).
2. A página mostra o nome do cliente e **para onde sua resposta é enviada**. Escolha os módulos e níveis, depois **Permitir**: sua senha é pedida a cada aprovação.
3. A conexão aparece na tabela de tokens com um selo **OAuth**. Edite suas permissões ou revogue ali como qualquer token; revogar desconecta o cliente.

Os tokens de acesso duram uma hora e são renovados em segundo plano; uma conexão sem uso por 30 dias precisa entrar de novo. Se um token de renovação for repetido por outra pessoa, a conexão é revogada e você é notificado.

::: warning Clientes remotos precisam de HTTPS público
O claude.ai e o ChatGPT se conectam a partir dos próprios servidores, então a instância precisa ser acessível por HTTPS público ([modo Web](/pt/config/network)). Aprove apenas uma página de consentimento que você mesmo abriu, agora há pouco: um link enviado por outra pessoa pode se passar pelo nome de qualquer cliente.
:::

::: tip Atualizou da 0.0.15 ou anterior com `otw update`?
O OAuth precisa de uma nova rota no `deploy/Caddyfile`. Veja [Atualização](/pt/guide/updating#oauth-caddyfile).
:::

## Como os agents enxergam o app

Os agents recebem quatro ferramentas de gateway:

- **`otw_catalog`**: lista os módulos e operações que o token pode chamar. Só aparecem os módulos concedidos. A listagem de um módulo mostra o método, o caminho, os parâmetros de query e os campos de nível superior do corpo de cada operação; `endpoint` (`POST /api/backtest/run`) retorna o schema completo do corpo daquela operação.
- **`otw_read`**: operações de leitura (exigem pelo menos *leitura* no módulo).
- **`otw_compute`**: operações marcadas *(compute)* no catálogo, que respondem uma pergunta e não armazenam nada: um backtest, uma varredura de parâmetros, métricas de risco. Exigem *leitura + escrita* como qualquer POST, mas seu cliente não pedirá que você aprove um cálculo.
- **`otw_write`**: operações de criar e atualizar (exigem *leitura + escrita*); operações de **exclusão** exigem *total* no módulo.

As respostas que carregam texto externo (artigos de feeds, e-mails recebidos) chegam ao agent dentro de um bloco rotulado que o instrui a tratar o conteúdo como dado e a ignorar qualquer instrução escondida nele.

A tabela de tokens nas Configurações mostra a hora do último uso de cada token, para você identificar e revogar os obsoletos. Um token também pode receber uma data de expiração quando você o cria ou edita: depois dessa data ele para de funcionar em todo lugar, inclusive para o agent do app.
