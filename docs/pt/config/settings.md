# Referência de configurações

Tudo o que está na entrada **Configurações** do seletor de módulos, seção por seção.

## Conta

Altere seu nome de usuário ou senha. Sua senha atual é exigida para salvar as alterações, e trocar a senha **encerra todas as suas sessões**.

Uma nova senha deve ter pelo menos 12 caracteres e é recusada se aparecer em listas públicas de vazamentos. Veja [Regras de senha](/pt/config/security#password).

## Segurança

Autenticação em dois fatores, os navegadores atualmente conectados à sua conta e por quanto tempo uma única requisição pode rodar. Detalhado em [Segurança da conta](/pt/config/security).

## Padrões

- **Idioma**: aplica-se a todo o app imediatamente (en, fr, de, es, it, pt, zh).
- **Moeda padrão** e **fuso horário**: os valores iniciais que os módulos usam para novos itens e para exibição.

## Aparência

A **cor de destaque** do app, usada por botões primários, estados ativos, links e destaques de gráficos. Escolha uma amostra predefinida ou qualquer cor no seletor; ela se aplica ao vivo em todo o app e é salva para todas as sessões. *Redefinir* devolve o padrão do tema.

## Rede

Quem pode acessar o app: localhost, LAN, LAN + HTTPS ou público. Detalhado em [Rede e acesso remoto](/pt/config/network).

## Cofre {#vault}

Um só lugar para as chaves de API e segredos que o app usa em seu nome, em vez de colar a mesma chave em cada módulo que precisa dela.

Um **cofre** representa um serviço externo (por exemplo *Binance*) e guarda **chaves** nomeadas: `apikey`, `secretkey` e assim por diante. Crie quantos cofres e chaves precisar; os módulos então **conectam uma chave por referência** por meio de um seletor compartilhado, onde quer que um segredo seja pedido (connectors de provedores, credenciais de feeds…).

- **Valores somente para escrita.** Um segredo é selado ao salvar e nunca mais pode ser visto, apenas substituído ou excluído. Os *nomes* das chaves continuam visíveis. Tudo é criptografado em repouso com a chave mestra do app.
- **Desconecte antes de excluir.** Excluir um cofre ou uma chave que ainda está conectada a um módulo é bloqueado; remova antes a referência lá. Cada cofre mostra quantas conexões o usam.
- **O acompanhamento de requisições** é opcional e **por cofre, não por chave**: todas as chaves de um cofre contam para o mesmo contador. O limite é apenas informativo (observar e exibir); nada nunca é limitado. Ele alimenta a mesma visão da [Taxa da API](#api-rate).

Os segredos de feeds de notícias também aceitam placeholders inline <code v-pre>{{vault.item}}</code>, resolvidos pelo agendador no momento da consulta. Veja [Notícias](/pt/modules/news-research#news).

## Módulos

Instale e desconecte módulos. Tudo vem com o app: instalar apenas torna um módulo disponível no seletor e no dashboard; nada é baixado. Desconectar o oculta e o torna inacessível; marque *excluir também os dados* para apagar também seus dados armazenados (permanente).

## Gerenciar dados

Uso de armazenamento por módulo (tabelas, linhas, tamanho) com o total do banco, e uma ação **Apagar** para excluir permanentemente os dados de um módulo (digite o nome dele para confirmar). Apagar não pode ser desfeito.

## Versões {#versioning}

Dois interruptores: **Arquivos do editor** e **Estratégias**, cada um com o número de versões armazenadas e seu tamanho. Ligar um avisa que cada versão é uma cópia completa e que o banco cresce a cada uma (imagens e vídeos nunca são duplicados). Desligar um pergunta se você quer manter as versões (ocultas até você ligá-lo de novo) ou excluí-las todas. Com uma área ligada, o versionamento é ativado por arquivo ou por estratégia no menu de histórico dela: veja [Editor](/pt/modules/productivity#editor) e [Backtest](/pt/modules/market-data#estrategias-e-indicadores-personalizados).

## Backup e restauração

Duas abas, cada uma com um lado de **Backup** e um de **Restauração**:

- **Completo**: comandos `pg_dump` e `psql` prontos para copiar para a sua implantação, incluindo variantes criptografadas, mais o status do backup automático nas instâncias que têm um.
- **Parcial**: escolha os módulos que quiser, baixe-os em um zip e carregue esse zip de volta aqui ou em outra instância. Os dois lados são contados antes: o que você está levando, por módulo e por tabela, e o que um arquivo que você carrega contém em comparação com o que já existe aqui.

Veja [Backup e restauração](/pt/guide/backup-restore).

## Atualizar app

Mostra a versão atual, consulta o GitHub por uma mais nova e lista os comandos de atualização a executar no host. Veja [Atualização](/pt/guide/updating).

## Registros

O armazenamento de logs do próprio app, com busca por mensagem/alvo. O **nível de captura** define a severidade mínima gravada no armazenamento (vale imediatamente). Níveis mais baixos capturam mais detalhe e usam mais espaço. Você pode limpar os logs armazenados aqui.

## Taxa da API {#api-rate}

Um dashboard de chamadas de saída para provedores externos de dados (dados de mercado, câmbio, cotações, feeds), contadas por dia UTC: contagem de requisições por provedor, erros, respostas de limite de taxa, limites publicados quando conhecidos e uma lista dos acertos recentes de limite de taxa. Ele existe para você ver o quão perto está dos limites do plano gratuito de um provedor.

**Esta página nunca limita nada**: ela apenas observa. O único lugar onde um limite é de fato aplicado é o [limite de requisições do próprio connector](/pt/config/connectors#limites-de-requisicoes) nas buscas sob demanda do gráfico; em todos os outros lugares um limite informa e avisa, e quem diz não continua sendo o provedor.

## Data connectors

A lista compartilhada de contas de provedores de dados de mercado usadas por Historical Data, Visualization, Watchlists e pelo Trading Journal: credenciais, limites de requisições e quais módulos podem usar cada um. Detalhado em [Data connectors](/pt/config/connectors). A mesma tela também está disponível sozinha em **/connectors**, e a partir do botão de connectors dentro de cada módulo de dados.

## Brokers {#brokers}

A lista compartilhada de **contas de brokers somente leitura** usadas pelo Trading Journal, Portfolios, Visualization e pela Tax Calculator: credenciais, configurações por broker e quais módulos podem usar cada uma. Detalhado em [Contas de brokers](/pt/config/brokers). Nada aqui pode enviar, alterar ou cancelar uma ordem.

## Notificações {#notifications}

A lista compartilhada de **canais de notificação**: para onde o app tem permissão de enviar, criados uma vez e reutilizados por todo módulo que notifica.

Um canal é um destino que **você possui**. Cada um guarda um segredo, digitado aqui ou conectado do [Cofre](#vault), selado ao salvar e nunca mais exibido.

| Canal | O que você traz | Segredo | Outros campos |
|---|---|---|---|
| **Email** | seu próprio servidor SMTP | senha | host, porta (587 STARTTLS, 465 TLS), de, para, usuário |
| **Telegram** | um bot do BotFather | token do bot | chat id |
| **Slack** | um Incoming Webhook | a URL do webhook | nenhum |
| **Discord** | um Webhook de canal | a URL do webhook | nenhum |

Os quatro são gratuitos para o host: você traz a conta, o app não traz nada para você se cadastrar. Alguns campos **não secretos** também aceitam um item do cofre, o **chat id** do Telegram por exemplo, para que um canal possa ser configurado sem esse id ficar em texto claro na configuração.

Os módulos que podem receber a concessão de um canal:

| Módulo | O que ele envia |
|---|---|
| **RemindMe** | um lembrete que disparou |
| **Watchlists** | um alerta de preço |
| **Mailbox** | uma conta de e-mail que precisa de atenção |
| **Webhooks** | um payload de entrada redirecionado para um módulo |
| **Historical Data** | uma parada longa, e o fim de um lote de downloads |
| **Visualization** | um alerta de gráfico que disparou |
| **Journal** | o enriquecimento com dados de mercado de um novo trade |
| **Backtest** | uma execução de paper trading, ou seu resumo agrupado |
| **Portfolio Tracker** | concedível antecipadamente; não envia nada hoje |
| **Automator** | o que um bloco `notify` enviar |

- **Concessões, por módulo.** Cada canal nomeia os módulos autorizados a enviar para ele, ou *todos*. A verificação roda no servidor: um módulo que nunca recebeu a concessão de um canal não consegue alcançá-lo, e o segredo daquele canal nem sequer é descriptografado para ele.
- **Um interruptor por canal.** Desativar um canal o silencia em todo lugar sem excluí-lo nem às suas credenciais.
- **Envio de teste** antes de confiar em um.

A mesma tela abre de dentro de cada módulo que notifica, então um canal pode ser adicionado na hora sem sair da página onde você está. Ela está deliberadamente ausente do catálogo do [MCP](#mcp): nenhum agent pode criar um canal ou ampliar uma concessão.

## MCP {#mcp}

Deixe AI agents usarem o app por um gateway controlado. Detalhado em [AI agents (MCP)](/pt/config/ai-agents).

## Controle externo {#external-control}

Controle o app a partir de um canal de chat (Telegram, Slack, Discord). Detalhado em [Controle externo (chat)](/pt/config/external-control).

## Voz {#voice}

Comandos push-to-talk e ditado: o motor de fala, os dois atalhos e seus comandos de voz. Detalhado em [Controle por voz](/pt/config/voice). O microfone só funciona por HTTPS ou em localhost.

## Créditos

As fontes de dados e os projetos upstream que cada módulo pode usar, incluindo provedores que você não configurou.

## Sobre

Versão, links do projeto e botões de compartilhamento.
