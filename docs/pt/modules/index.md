# Visão geral dos módulos

O OpenTraderWorld é um conjunto de **módulos**, pacotes de recursos que você liga individualmente em **Configurações → Módulos**. Tudo vem com o app; instalar um módulo o faz aparecer no seletor de módulos (canto superior esquerdo) e no dashboard. Desconecte um módulo para ocultá-lo de novo (seus dados são mantidos, a menos que você também os exclua).

O [dashboard, a busca e as notificações](/pt/modules/dashboard) ficam acima de todos eles e estão sempre lá.

## Dependências

Alguns módulos se apoiam no catálogo de conjuntos de dados do **Historical Data** e precisam dele instalado:

```
Historical Data ──▶ Historical Data Visualization
                ──▶ Backtest
                ──▶ Quant Tools
```

Todo o resto é independente, embora alguns módulos se integrem quando ambos estão instalados (por exemplo, o Tax Calculator pode importar o PnL do Trading Journal; o MyWealth pode importar os holdings do Portfolio Tracker; o Calendar pode exibir ToDos, Goals e Reminders).

Historical Data, Visualization, Watchlists, Fundamentals e o Trading Journal também compartilham uma lista de **[data connectors](/pt/config/connectors)**: uma conta de provedor é criada uma vez e concedida aos módulos que podem usá-la.

## Todos os módulos

### Trading

| Módulo | O que ele faz |
|---|---|
| [Trading Journal](/pt/modules/journal) | Registro de trades com modelos, tabelas de taxas, câmbio multimoeda e estatísticas de desempenho. |
| [Trading Routines](/pt/modules/productivity#routines) | Checklists recorrentes de sessão: preparação pré-mercado, disciplina durante a sessão, revisão pós-mercado. |
| [Mindset](/pt/modules/productivity#mindset) | Check-ins diários de humor e disciplina com tendências. |

### Dados de mercado e análise

| Módulo | O que ele faz |
|---|---|
| [Historical Data](/pt/modules/market-data#histdata) | Baixe histórico OHLCV de vários provedores para conjuntos de dados locais. |
| [Historical Data Visualization](/pt/modules/market-data#histviz) | Um espaço de trabalho de gráficos candle/OHLC/linha/Renko com indicadores, desenhos, comparações e alertas monitorados pelo servidor, ao vivo ou sob demanda, em qualquer instrumento que um connector sirva. |
| [Backtest](/pt/modules/market-data#backtest) | Backtester de estratégias baseadas em regras com dimensionamento, custos e estatísticas completas. |
| [Quant Tools](/pt/modules/market-data#quant) | Risco, estatísticas, volatilidade e regimes de um conjunto de dados; pares, baskets e regressão de fatores; testes de overfitting em backtests salvos; dimensionamento, calculadoras, curvas de futuros e superfícies de volatilidade de opções. |
| [Fundamentals](/pt/modules/fundamentals) | Séries macro, demonstrações de empresas, registros da SEC, transcrições, ETFs, um calendário de mercado e dados alternativos de fontes primárias e dos agregadores que você conecta. |

### Carteiras e dinheiro

| Módulo | O que ele faz |
|---|---|
| [Watchlists](/pt/modules/portfolio#watchlists) | Watchlists de símbolos com preços ao vivo, variações do dia, sparklines e notas. |
| [Portfolio Tracker](/pt/modules/portfolio#portfolios) | Valor ao vivo, ledger de caixa e renda, desempenho frente ao risco assumido, desvio de alocação e stress tests. |
| [MyWealth](/pt/modules/portfolio#wealth) | Patrimônio líquido de tudo o que você possui e deve, com carteiras lidas ao vivo em vez de copiadas. |
| [Managers' Portfolios](/pt/modules/portfolio#mportfolios) | Posições 13F de superinvestidores, navegáveis e com snapshots. |
| [Tax Calculator](/pt/modules/portfolio#taxcalc) | Estimativas de imposto para trading e investimento a partir de modelos por país. |
| [Subscriptions](/pt/modules/portfolio#subscriptions) | Assinaturas recorrentes e visão geral de gastos. |

### Notícias e pesquisa

| Módulo | O que ele faz |
|---|---|
| [News](/pt/modules/news-research#news) | Agregador de notícias RSS e API JSON com dashboards de polling. |
| [Mailbox](/pt/modules/news-research#mailbox) | Newsletters, notícias de mercado e e-mails de brokers lidos da sua própria caixa IMAP, sem rastreadores. |
| [Economic Calendar](/pt/modules/news-research#economics) | Próximos eventos macro. |
| [FinanceDatabase](/pt/modules/news-research#findb) | Busque mais de 300.000 instrumentos localmente; organize favoritos em pastas. |
| [Resources](/pt/modules/news-research#resources) | Biblioteca de favoritos para livros, links e referências. |
| [Community Docs](/pt/modules/news-research#community-docs) | Guias escritos pela comunidade, sincronizados e legíveis offline. |

### Notas e organização

| Módulo | O que ele faz |
|---|---|
| [Editor](/pt/modules/productivity#editor) | Editor de documentos rico com pastas e bancos de dados em tabela/kanban/galeria. |
| [ToDo](/pt/modules/productivity#todos) | Lista de tarefas com prazos e categorias. |
| [Goals](/pt/modules/productivity#goals) | Metas com acompanhamento de métricas e prazos. |
| [Calendar](/pt/modules/productivity#calendar) | Calendário de eventos pessoais; sobrepõe lembretes, tarefas e metas. |
| [RemindMe](/pt/modules/productivity#remindme) | Lembretes com notificações no app e canais de email/Telegram/Slack/Discord. |
| [Time Tracker](/pt/modules/productivity#time) | Cronômetros de projetos com orçamentos e valor por hora. |
| [Prompt Store](/pt/modules/productivity#prompt-store) | Biblioteca pesquisável de prompts de IA reutilizáveis, com tags, avaliações e versões. |
| [Webhooks](/pt/modules/productivity#webhooks) | URLs privadas de entrada que transformam alertas externos em notificações. |
| [Automator](/pt/modules/automator) | Workflows sobre a sua própria API e o mundo externo, à mão ou em agenda. |

### IA

| Módulo | O que ele faz |
|---|---|
| [Agent](/pt/modules/agent) | Assistente de chat com IA integrado (traga seu próprio provedor) que também pode agir sobre seus dados via MCP, com memória, skills e servidores MCP externos. |
