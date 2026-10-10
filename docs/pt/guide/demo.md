# Modo demo

Existe um sandbox público em **[demo.opentraderworld.com](https://demo.opentraderworld.com)**: o app real, populado com dados de exemplo, compartilhado por todos e reiniciado a cada **15 minutos**. Nada para instalar, nada para se cadastrar; você chega já conectado como `demo`.

Esta página explica o que é esse modo, para você saber o que está vendo, e para que você possa rodar um por conta própria se quiser mostrar o app a alguém.

## O que é

O modo demo é uma postura que o backend adota quando inicia com `OTW_DEMO=1`. **Ele nunca é ativado implicitamente**: uma instalação normal não é afetada por nada do que segue.

- **O banco de dados é reiniciado a cada quarto de hora.** A semente é restaurada a partir de um template, então tudo o que você alterar some às :00, :15, :30 ou :45. O banner dentro do app faz a contagem regressiva até o próximo.
- **Você é conectado automaticamente** como a conta `demo`. Não há senha para adivinhar, nem conta para criar.
- **Todos compartilham um único banco.** Os trades, notas e conversas de outros visitantes ficam visíveis, e os seus ficam visíveis para eles. Não digite nada que você não publicaria.

## O que é bloqueado

O portão é **negar por padrão**: uma requisição precisa corresponder a uma lista de permissões explícita ou é recusada com `demo_disabled`. Uma rota em que ninguém pensou fica fechada, não aberta, a mesma regra que o [catálogo MCP](/pt/config/ai-agents) segue.

Em linhas gerais:

| Bloqueado | Somente leitura | Completo |
|---|---|---|
| Configuração inicial, logout, conta e senha, rede, backup, atualização, apagar dados, instalar/desconectar módulos, **o cofre**, **o Automator**, webhooks de entrada, o próprio endpoint MCP, canais de notificação, instalação do FinanceDatabase, downloads de provedores | Data connectors, feeds, arquivos, carteiras de gestores, configurações e tokens do MCP, taxa da API, conjuntos de dados armazenados, provedores do agent, memórias e skills | Journal, backtest, quant, carteiras, calendário, tarefas, metas, editor e bancos de dados, prompts, recursos, assinaturas, taxcalc, tempo, dashboard, busca, chat do agent |

Então você pode registrar um trade, rodar um backtest e conversar com o assistente; você não pode mudar o modo de rede, gerar um token, extrair o banco, nem fazer a máquina buscar dados de um provedor com cobrança por uso em seu nome.

Dois desses merecem uma palavra, já que o módulo é visível mas não faz nada:

- **O cofre** é fechado por completo. É o único armazenamento cuja finalidade inteira é guardar credenciais, e este banco é compartilhado e público.
- **O Automator** é fechado pela regra de negar por padrão, e não por uma linha própria: um workflow alcança tudo o que seu token concede e pode chamar qualquer URL, exatamente o que um sandbox público não deve oferecer. Abrir o módulo mostra a interface; toda requisição por trás dela responde `demo_disabled`.

As mensagens de chat também são limitadas a 2000 caracteres.

## Cotas por visitante

Os endpoints caros custam dinheiro de verdade, então cada um tem **dois** orçamentos: uma fatia por visitante e um teto global por cima. Só por IP não limitaria o gasto; só global permitiria que um visitante com script trancasse todos os outros para fora.

| | Por visitante | Em toda a demo | Janela |
|---|---|---|---|
| **Execuções do agent** | 3 | 8 | 10 minutos |
| **Execuções do agent** | 10 | 40 | 24 horas |
| **Backtests e varreduras** | 10 | 30 | 10 minutos |

O assistente roda com uma **chave compartilhada fixada em um modelo gratuito**, resolvida na inicialização; a memória de longo prazo e os servidores MCP externos ficam desligados.

## Rodando o seu

```bash
OTW_DEMO=1        # in the core service's environment
otw-core --seed-demo   # once, against a scratch database
```

A semente é pública: vem no repositório, não contém segredos e é ignorada se já existir um usuário `demo`. A reinicialização é uma restauração `CREATE DATABASE … TEMPLATE` conduzida a partir do host, não pelo app.

::: warning Não aponte o modo demo para os seus dados
A semente grava em qualquer `DATABASE_URL` que você indicar, e a reinicialização restaura por cima. Use um banco descartável.
:::
