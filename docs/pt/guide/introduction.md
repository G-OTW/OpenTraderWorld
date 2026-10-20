# O que é o OpenTraderWorld?

> Site do projeto: **[opentraderworld.com](https://opentraderworld.com)**: tour pelos módulos,
> [demo ao vivo](https://demo.opentraderworld.com), [guias da comunidade](https://opentraderworld.com/docs)
> e [votação do roadmap](https://opentraderworld.com/suggestions).

O OpenTraderWorld é uma **plataforma web auto-hospedada para traders e investidores**. Você a instala uma vez com Docker no seu próprio computador ou servidor, abre no navegador e obtém um espaço de trabalho privado feito de módulos: um diário de trading, dados históricos de mercado com gráficos e backtesting, acompanhamento de carteiras e patrimônio líquido, um agregador de notícias, notas, checklists e mais.

**Gratuito para todos, uso pessoal ou profissional. Código disponível (FSL-1.1-MIT).** A única coisa que você não pode fazer é revendê-lo ou oferecê-lo como serviço pago. O princípio que guia o projeto: *seja lucrativo antes de gastar um centavo.*

## Por que auto-hospedado?

- **Seus dados continuam seus.** Trades, carteiras, notas e entradas do diário ficam em um banco PostgreSQL na sua máquina, não no servidor de outra pessoa.
- **Privado por padrão.** Após a instalação, o app escuta apenas em `localhost`. Expô-lo à sua LAN ou à internet é uma escolha explícita que você faz em [Configurações → Rede](/pt/config/network).
- **Sem assinatura.** As ferramentas principais não custam nada para rodar. Alguns módulos podem usar opcionalmente provedores de dados externos (muitos com planos gratuitos), e você traz suas próprias chaves de API.

## Como funciona

Uma stack `docker compose`, quatro serviços:

| Serviço | Função |
|---|---|
| **core** | Servidor de API em Rust (Axum): toda a lógica de negócio, agendador, tarefas em segundo plano |
| **postgres** | PostgreSQL: o único lugar onde seus dados vivem |
| **frontend** | Aplicação SvelteKit de página única, compilada uma vez na implantação |
| **caddy** | Proxy reverso: serve o app, faz proxy de `/api`, cuida dos certificados HTTPS |

O app é **monousuário**: uma conta de administrador, criada na instalação. Não há modo multi-tenant, compartilhamento nem gestão de usuários para configurar.

O Docker é atualmente a **única implantação suportada**: ele mantém a instalação não intrusiva e rápida de reconstruir ([por quê, e como obter o Docker](/pt/guide/docker)). Uma instalação nativa é possível, mas não recomendada.

## Os módulos

Módulos são pacotes de recursos que você instala ou desconecta em **Configurações → Módulos**. Tudo já vem com o app, e instalar um módulo apenas o ativa. Destaques:

- **[Trading Journal](/pt/modules/journal)**: registre trades com modelos, tabelas de taxas, PnL multimoeda e estatísticas completas de desempenho.
- **[Dados de mercado e backtesting](/pt/modules/market-data)**: baixe histórico OHLCV de vários provedores, plote qualquer instrumento ao vivo ou sob demanda com indicadores, faça backtest de estratégias baseadas em regras e rode análises quant em conjuntos de dados, backtests salvos, curvas de futuros e cadeias de opções.
- **[Carteiras e patrimônio](/pt/modules/portfolio)**: acompanhamento de carteiras ao vivo, histórico de patrimônio líquido, posições 13F de superinvestidores, estimativas de imposto.
- **[Notícias e pesquisa](/pt/modules/news-research)**: painéis de notícias RSS/API, calendário econômico, um catálogo de busca com 300 mil instrumentos.
- **[Notas e organização](/pt/modules/productivity)**: editor de texto rico com bancos de dados, tarefas, metas, calendário, lembretes, rotinas de trading e check-ins de mentalidade.
- **[AI Agent](/pt/modules/agent)**: assistente de chat integrado (traga seu próprio provedor) que pode agir sobre seus dados via MCP, com memória, skills e servidores MCP externos.

Veja a [lista completa de módulos](/pt/modules/).

## Próximos passos

1. [Instale o OpenTraderWorld](/pt/guide/install): cerca de 5 minutos com Docker.
2. [Dê seus primeiros passos](/pt/guide/first-steps): entre, escolha os padrões, instale módulos.
3. [Configure o acesso pela rede](/pt/config/network): se você quiser acessá-lo de outros dispositivos.

Ainda não quer instalar? Experimente a [demo ao vivo](https://demo.opentraderworld.com), uma instância compartilhada
populada com dados, reiniciada a cada 15 minutos. [O que é o modo demo](/pt/guide/demo) e o que ele bloqueia.
