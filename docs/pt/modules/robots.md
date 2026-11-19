# Bots de trading {#trading-robots}

Um **bot de trading** é uma estratégia do Backtest rodando para a frente. Ele congela as configurações da estratégia, roda a cada fechamento do timeframe da estratégia num conjunto de instrumentos e simula as suas execuções ou envia as suas ordens para uma conta de broker. Por cima da estratégia, cada bot tem o seu próprio dinheiro (uma **política de capital**), **exclusões**, **proteções** que reagem aos seus próprios resultados e uma **prioridade**.

Não há segundo motor: toda execução lê a janela com o backtest comum e age só sobre o candle que acabou de fechar. Um bot de sinal leva as posições que tomou tal como as tomou; o que a janela mostra em candles anteriores é histórico, nunca registrado nem enviado.

A estratégia dá os sinais, o seu stop e o seu alvo, e os seus tipos de ordem. O bot dá o dinheiro e o calendário: a sua [política de capital](#capital-policy) e as suas [exclusões](#exclusions). O instrumento (valor do ponto, tick de preço, passo de tamanho, mínimo) é o da bolsa. O capital, o tamanho, os filtros de dia e de sessão e os custos da estratégia não são lidos.

O módulo precisa do **Backtest** instalado. Ele fica em **/robots**, como **Bots de trading** no seletor de módulos.

## A frota

A página da frota lista todos os bots: cartões no celular, uma tabela ordenável no desktop. Ela se atualiza a cada 10 segundos enquanto a página está visível, e as contagens regressivas andam a cada segundo.

- **Números principais**: **Bots** (com quantos estão ativos), **Capital** (dividido por bot), **Hoje** (o P&L de hoje e os eventos não lidos), **Abertas** (posições abertas, e quantos bots têm uma).
- **P&L da frota**: a curva de patrimônio somada dos bots, com **P&L**, **Realizado** e **Não realizado**. Filtre-a por ambiente (**Todos os ambientes**, Simulado, Papel, Testnet, Real) e por uma data **De** e **Até**. Sem data final, o último ponto é marcado a mercado nas posições abertas.
- **Classificação**: os bots ordenados pelo P&L dos trades fechados **Hoje**, em **1S**, **1M** ou **Tudo**, com o retorno sobre o capital inicial.
- **Exposição por tipo de ativo**: o que as posições abertas comprometem ao último preço, repartido por tipo de ativo (cripto, futuros, forex, ações...), com quantos bots há em cada um.
- **Filtros de estado**: um clique num chip mostra os bots nesse estado (**Ativo**, **Retido por uma proteção**, **Parado**, **Desarmado, arme-o de novo**, **Degradado**, **Bloqueado**, **Corretora inacessível**, **Erro**), com uma contagem em cada um. Um segundo clique o limpa. O seletor de conta restringe a **Simulado** ou a uma conta.
- **Precisa de atenção**: os bots com um erro, uma proteção que os retém ou eventos não lidos, cada um com o motivo.
- **Próximos ticks**: os oito bots com execução mais próxima, com os seus instrumentos e uma contagem regressiva.
- **Marcar N como lidos** limpa os eventos não lidos de todos os bots. **Novo bot** abre o assistente.

## Criar um bot {#create-a-robot}

**Novo bot** na página da frota, ou **Executar como bot** no Backtest: num resultado (a execução exatamente como rodou) ou numa estratégia salva na biblioteca. De qualquer forma o assistente abre, uma etapa de cada vez:

1. **Estratégia**: o nome do bot e uma estratégia salva para congelar (ou a execução de onde ele veio). **Criar uma estratégia** abre o Backtest. Uma edição posterior da estratégia não muda nada no bot até você atualizá-lo (veja [Definições](#settings)).
2. **Instrumentos**: **Adicionar um instrumento** abre o seletor de dados do gráfico, que busca na pesquisa de símbolos de cada provedor. Nenhum histórico armazenado é necessário: cada execução busca os candles que a estratégia lê. Até 8 instrumentos, num mesmo **Timeframe** que todos compartilham. Cada instrumento é lido pela fonte de dados de onde foi escolhido, que as **Definições do bot** podem mudar depois. Onde o provedor transmite um preço ao vivo, stops, alvos e limits disparam nele; nos outros casos são verificados no fechamento do candle. As configurações do bot dizem qual se aplica a cada instrumento.
3. **Conta**: **Simular** (nada é enviado a lugar nenhum) ou **Conta de corretora** (veja [Numa conta de broker](#on-a-broker-account)).
4. **Capital**: a [política de capital](#capital-policy).
5. **Proteções**: as [proteções](#guards). Um bot novo começa com o conjunto padrão.
6. **Agenda e notificações**: o bot roda a cada fechamento dos seus candles e decide sobre o candle que acabou de fechar. A etapa guarda as suas [**Exclusões**](#exclusions), um **Relatório de estado** opcional (**Enviar um relatório de estado**, depois quando ele sai: a cada N minutos, diariamente, semanalmente, mensalmente ou uma vez, num fuso horário), **Velas móveis** (quanto histórico cada execução alimenta ao motor, 5000 por padrão), **Prioridade**, **Notificar** (**Cada execução**, **Apenas numa nova operação**, **Nunca**) e **Canais**.
7. **Revisão**, depois **Criar o bot**.

O servidor verifica tudo de novo quando o bot é criado. Uma recusa é mostrada como foi dita, e nomeia o que mudar.

O bot começa de imediato. A janela **A iniciar** *nome do bot* acompanha a sua primeira execução enquanto ele baixa o seu histórico e aprende o seu book; **Continuar em segundo plano** a fecha e abre a página do bot enquanto a execução continua. Quando ele está pronto, um bot simulado roda a cada fechamento a partir daí, e um bot de broker oferece **Armar** na mesma janela (veja [Numa conta de broker](#on-a-broker-account)). **Abrir o bot** abre a sua página.

Os canais são os canais de notificação concedidos ao Trading Bots; o botão de canais da etapa os adiciona ou gerencia sem sair do assistente. Sem nenhum canal escolhido, o bot envia para todos os canais concedidos ao Trading Bots; sem nenhum concedido, nada é enviado e todo evento continua registrado no log do bot. **Notificar** **Nunca** não envia nada. O relatório de estado dá o estado do bot, os seus números desde o último relatório e o que ele tem; ele não é enviado enquanto o bot está numa exclusão e não tem nada. As **Definições do bot** também guardam alertas de entradas e saídas por instrumento e a sua própria redação para as mensagens de entrada e saída, e para o resumo quando um relatório de estado está ligado.

### Exclusões {#exclusions}

As exclusões são períodos em que o bot não toma nenhuma nova entrada. As saídas continuam geridas dentro deles. Elas são lidas no fuso horário do bot, e qualquer combinação funciona:

- **Dias inteiros**: os dias da semana escolhidos.
- **Intervalos de horas**: de uma hora a outra, o fim excluído; um intervalo que termina antes de começar passa da meia-noite.
- **Intervalos de datas**: de uma data a outra, os dois dias incluídos.

Até 20 intervalos de horas e 20 intervalos de datas. Com **Sair do mercado**, o bot fecha as suas posições quando um período começa, e volta a operar quando ele termina. Entrar e sair de um período fica no log, e vai para os canais do bot quando ele tem uma posição.

## A página de um bot {#a-robots-page}

O cabeçalho mantém o estado do bot à vista: o nome, a estratégia e a sua versão, selos para o estado, o ambiente de um bot de broker (**Papel**, **Testnet**, **Real**), a proteção (em pausa, parado, ou a escala, como x0.5) e, numa conta de broker, **Armado** ou **Desarmado**. Abaixo dele: a próxima avaliação como contagem regressiva, a última avaliação e a última sincronização.

- **Executar agora** roda o bot na hora. **Parar estratégia** pausa a sua agenda; não fecha posições nem limpa ordens. **Iniciar estratégia** a retoma.
- **Insights** lê os trades fechados do bot, veja [Insights](/pt/modules/journal#insights).
- Enquanto uma proteção retém o bot, um aviso diz por quê, com **Retomar**.

A página se atualiza a cada poucos segundos; as posições abertas seguem o preço ao vivo onde o provedor transmite um. Oito abas:

| Aba | Mostra |
|---|---|
| **Visão geral** | **Capital**, **P&L realizado**, **P&L não realizado**, **Hoje** (P&L e ordens enviadas), **Taxa de acerto**, **Fator de lucro**, **Drawdown máx.**, **Expectativa**; a curva de patrimônio; o último preço de cada instrumento (ao vivo, candle armazenado, ou desatualizado); **P&L por instrumento** e **P&L por direção**; o calendário de P&L; um gráfico no timeframe da estratégia com entradas e saídas. Um bot de broker também mostra o seu [painel de execução](#on-a-broker-account) |
| **Posições** | instrumento, lado, quantidade no tamanho do bot, preço médio, preço (ao vivo ou armazenado), não realizado, stop, alvo e idade |
| **Pendentes** | as ordens de um bot simulado à espera da próxima abertura; as ordens de um bot de broker em andamento no broker |
| **Operações** | os trades fechados, 25 por página, com a escala da proteção onde ela se aplicou. **Não tomados** conta as entradas que o bot recusou e lista as 10 mais recentes, cada uma com o motivo |
| **Estatísticas** | as estatísticas de trades, a curva de desempenho e, para estratégias de sinal, um gráfico de dispersão 3D e as sequências |
| **Proteções** | as regras como frases, o que elas retêm agora, e a linha do tempo dos 100 últimos disparos, restaurações e retomadas, da mais recente para a mais antiga |
| **Registo** | os 200 últimos eventos: entradas, saídas, recusas, ações de proteção, erros |
| **Definições** | a estratégia, a conta, a agenda, a prioridade, o capital e as proteções, com **Definições do bot**, a atualização para a versão da estratégia e **Eliminar este bot** |

O book de um bot começa com a sua primeira execução: só o que acontece a partir do candle que acabou de fechar é registrado. O que a janela mostra antes dele é histórico, nunca no seu book, nas suas estatísticas ou nas suas proteções.

### Definições {#settings}

**Definições do bot** edita o nome, a fonte de dados de cada instrumento (**Predefinida do fornecedor**, ou um connector concedido ao Trading Bots), as exclusões, o relatório de estado, a janela, a prioridade, o capital, as proteções e as notificações. A estratégia, os instrumentos e a conta são congelados: outra estratégia ou outra conta é um novo bot. A nota **Seguida em frente** lista os instrumentos e os tipos de ordem de entrada e saída da estratégia: os seus sinais, stop e alvo rodam como foram congelados, enquanto o seu capital, tamanho, calendário e custos não são lidos.

**Passar para a versão atual da estratégia** (mostrado quando o bot veio de uma estratégia salva) toma a estratégia salva tal como está agora. O book do bot é limpo e reaprendido na próxima execução, e um bot armado é desarmado.

Excluir um bot exclui o seu book, a sua linha do tempo de proteções e o seu log.

## Política de capital {#capital-policy}

O dinheiro do bot dimensiona cada decisão da estratégia: a estratégia roda sobre o capital do bot, não sobre o seu. Todo campo é opcional:

| Campo | Significado |
|---|---|
| **Alocação** | o capital com que o bot opera. Vazio: o capital e o tamanho da estratégia |
| **Moeda** | a moeda da conta, para ler os números |
| **Parte por posição (%)** | uma posição vale esta parte do capital do bot. Precisa de uma alocação |
| **Alavancagem** | o bot dimensiona com ela, até 125. Vazio: 1 |
| **Entradas por posição** | quantas entradas uma posição pode tomar, até 50. Vazio: 1. Um bot de broker aceita só 1 |
| **Máx. posições abertas** | uma entrada além disso não é tomada |
| **Máx. ordens por dia** | entradas contadas por dia UTC. As saídas nunca são retidas |

Cada entrada é então multiplicada pela escala de uma proteção e arredondada para baixo ao passo de tamanho da bolsa (nunca para cima). Numa conta de broker, a **Posição máxima (unidades)** definida na criação recusa uma entrada maior em vez de cortá-la.

**Uma entrada recusada nomeia a sua correção**, na aba Operações e no log:

> the bot already holds 3 position(s), its maximum of 3: the entry on BTCUSDT was not taken. Raise the maximum of positions

As outras se leem da mesma forma: um tamanho abaixo do passo da bolsa (aumente a alocação, a parte por posição ou a alavancagem) ou abaixo do seu mínimo (aumente a alocação, a parte por posição ou o tamanho da estratégia), o máximo de ordens do dia, um tamanho acima da posição máxima.

## Proteções {#guards}

As proteções são regras sobre os **próprios trades fechados** do bot, no tamanho do bot. Até 10 por bot. As saídas são sempre geridas, seja o que for que uma proteção retenha.

Cada proteção une uma condição a uma ação, qualquer uma com qualquer uma.

| Quando | Então |
|---|---|
| **perdas consecutivas**, **ganhos consecutivos** (uma contagem) | **pausa** por dias ou horas (de um minuto a 365 dias) |
| **drawdown %**, **desde o pico** ou **desde o início** | **parar até retomar** |
| **perda % em**, **valor de perda em**, **perdas em**, **ganho % em** uma janela: **hoje**, **esta semana**, **este mês**, **desde o início** (no fuso horário do bot) | **reduzir o tamanho**: **Dividir por** mais de 1 e no máximo 10, **Voltar ao tamanho total** **após operações**, **após ganhos**, **após dias** ou **à mão** |

O conjunto padrão oferecido a um bot novo: pausa de um dia após 3 perdas consecutivas, e metade do tamanho após um drawdown de 10% desde o pico, de volta ao tamanho total após 5 trades.

- Um bot em pausa ou parado não toma nenhuma nova entrada e continua gerindo as suas posições. Um bot reduzido dimensiona as suas próximas entradas para baixo.
- Uma pausa termina sozinha. Uma parada espera por **Retomar**. Uma redução volta ao tamanho total pela sua regra de restauração, num só passo.
- Uma regra de contagem conta de novo a partir do seu último disparo: uma pausa após três perdas dispara de novo após mais três. Uma regra de nível dispara quando o nível é cruzado, e de novo depois que ele deixou de ser atendido ou a sua janela virou.
- Várias ao mesmo tempo: uma parada vence uma pausa, uma pausa vence uma redução, e uma redução disparada com elas se aplica quando as entradas recomeçam. Uma redução sobre uma redução se multiplica, nunca abaixo de x0.1.
- Os percentuais são do capital inicial do bot (a alocação, senão o capital inicial da estratégia). Um drawdown desde o pico é do capital no pico; uma perda ou ganho em hoje, esta semana ou este mês é do capital no início desse período.
- **Retomar** limpa tudo o que uma proteção retém. Editar as regras mantém o que está retido agora.

Todo disparo e toda restauração estão na aba **Proteções** e no log, e vão para os canais do bot.

## Prioridade e limites de taxa

Cada bot tem uma **Prioridade** de 0 a 1000, 100 por padrão. **O menor roda primeiro** quando vários bots vencem ao mesmo tempo ou compartilham um broker. Os bots que vencem no mesmo fechamento rodam lado a lado, até 8 de uma vez.

Os bots numa conta de broker são sincronizados a cada 15 segundos, broker por broker, em ordem de prioridade:

- uma passagem envia no máximo 10 ordens para um broker, bot por bot em ordem de prioridade. Saídas e fechamentos nunca são retidos por esse limite; uma entrada além dele espera a próxima passagem, e o bot diz isso;
- quando um broker recusa pedidos pelo seu limite de taxa, **só os bots desse broker esperam**: até o horário que ele indicou (no máximo uma hora), ou por uma passagem quando ele não indicou nenhum. Cada bot mostra uma nota que nomeia a espera e o que mudar (menos bots ou instrumentos na conta, ou um limite maior no seu plano). Os bots de outros brokers seguem em frente.

## Numa conta de broker {#on-a-broker-account}

Um bot pode enviar as suas ordens para qualquer conta com o [trading](/pt/config/trading) ligado e concedida ao **Trading Bots** em [Configurações → Brokers](/pt/config/brokers), incluindo uma conta de [paper trading](/pt/config/paper-trading). O selo de ambiente diz para onde elas vão: **Papel** (o broker paper do app), **Testnet** (uma conta de teste ou demo de uma bolsa), **Real** (dinheiro real).

O motor decide a cada fechamento e a conta executa. Na criação:

- só uma estratégia de sinal executa num broker, com uma entrada por posição (**Entradas por posição** 1);
- cada instrumento é procurado na conta, que mostra o símbolo e o passo de tamanho que encontrou. Um instrumento que a conta não consegue nomear é recusado: o bot nunca adivinha;
- **Saídas**: **No broker** (o stop e o alvo ficam no broker, anexados à entrada, onde o broker consegue mantê-los) ou **Vigiadas** (verificadas no preço ao vivo e fechadas a mercado quando cruzadas);
- vários bots podem visar um mesmo instrumento numa mesma conta, mas só um de cada vez está armado, para que dois bots nunca operem uma posição um contra o outro.

Um bot de broker é **criado desarmado**: nada é enviado até você o **Armar**, na janela **A iniciar** *nome do bot* quando a sua primeira execução termina, ou depois a partir da sua página. Armar exige o **trading em segundo plano** assinado na conta (**Assinar o trading em segundo plano** abre os termos para assinar), e o código do seu autenticador quando o [bloqueio de trading](/pt/config/trading#trading-lock) está ligado e a conta não é paper. A partir do fechamento seguinte, as entradas vão a mercado e as saídas como a estratégia decide, sem ninguém olhando. Toda ordem passa pelos [limites de risco](/pt/config/trading#risk-limits) da conta e fica no [registo de auditoria](/pt/config/trading#audit-log).

Armar lê a conta primeiro: é recusado quando o broker não pode ser alcançado, ou quando a conta tem menos do que o dinheiro do bot (a sua alocação, senão o capital da estratégia). Cada bot é verificado por si, então vários bots podem compartilhar uma conta; uma entrada que a conta não pode pagar depois não é enviada, e o log diz o que ela precisava e o que a conta tinha.

O **painel de execução** na Visão geral mostra o nível da conta, os seus limites, o book do motor ao lado do da conta, e o preço simulado de cada trade ao lado do preço do broker. O seu estado:

| Estado | Significa |
|---|---|
| **Desarmado** | nada é enviado até você armar o bot |
| **Em sincronia** | a conta tem o que o bot espera |
| **Degradado** | uma entrada foi executada só em parte. O bot não acrescenta exposição e continua gerindo as suas saídas até você zerar ou adotar |
| **Parado, arme-o de novo** | uma posição foi aberta à mão no instrumento do bot enquanto ele estava zerado, ou a posição do bot foi fechada, aumentada ou reduzida à mão. O bot foi desarmado e as suas entradas em espera foram canceladas; as saídas de uma posição fechada também são canceladas. Arme-o de novo para continuar |
| **Envio em pausa** | uma proteção retém as novas entradas, ou o trading está suspenso na conta: só o que tira risco é enviado |
| **Verificar no broker** | uma ordem ficou sem resposta: nada mais é enviado nesse instrumento até isso ser resolvido |
| **Broker inacessível** | o broker não respondeu a três sincronizações seguidas (45 s). Nada é enviado até ele responder, e a conta é lida de novo antes de qualquer envio |
| **Erro da conta** | a conta não pode ser usada (as suas credenciais ou a sua concessão): nada é enviado até que possa |

O bot lê a conta a cada sincronização (a cada 15 s, e logo após um reinício) antes de enviar qualquer coisa. Qualquer mudança que você faça à mão na posição do bot o para até você armá-lo de novo. Um stop ou um alvo que você move à mão no broker fica onde você o pôs: o bot não o move mais. Cancele à mão o seu stop e o seu alvo e a próxima sincronização os coloca de novo, uma vez; depois disso o bot os vigia no preço ao vivo. Cancelar qualquer outra das suas ordens pendentes não muda nada para ele. A verificação de posição da própria conta deixa o instrumento de um bot para o bot: os trades de um bot nunca suspendem a conta.

**Posições** e **Pendentes** mostram só o que está no broker. Uma entrada que não saiu (recusada por um limite, uma proteção, as regras de tamanho da bolsa, os fundos da conta ou o broker) é uma linha no **Registo**, com o motivo exato ao passar o mouse. Os preços e o P&L de um trade fechado são os que as suas ordens executaram no broker, taxas incluídas, quando a sua saída foi executada; até lá vale o valor simulado. Um stop ou um alvo executado no broker, Parar e zerar, ou um fechamento que você faz no broker fecha o trade do bot nessa execução, mesmo que os candles do bot nunca tenham chegado ao nível; uma entrada que nunca saiu não é tomada. Um candle que o provedor corrige depois do seu fechamento não muda nada: o bot faz o que decidiu nesse fechamento, e uma reversão é uma mensagem.

- **Desarmar** para o envio. As ordens que já estão no broker ficam lá.
- **Parar e zerar** desarma, cancela as ordens em andamento do bot e fecha o que ele tem a mercado.
- **Adotar a diferença** deixa para você o que a conta tem fora do bot, para que um bot degradado volte a operar.

Armar, desarmar, zerar e adotar são sempre feitos por você a partir da página, nunca por um agent ou um workflow.

## Sessões paper

As sessões paper do Backtest agora são bots, com o seu book, eventos, conta e concessões de canais. Um antigo link de sessão paper abre a página do bot. **Executar como bot** substitui **Paper trade** no Backtest.

## Bom saber

- Os [agentes de IA (MCP)](/pt/config/ai-agents) podem ler bots (a lista, um bot, o seu dashboard, as suas operações, os seus eventos de proteção, o seu painel de execução, a frota, a caixa de entrada e as variáveis de mensagem). Criar, editar, executar ou armar um continua no app.
- Um bot lê os candles e o preço ao vivo de cada instrumento pela fonte de dados escolhida para ele, senão por um [data connector](/pt/config/connectors) concedido ao Trading Bots, senão por um concedido ao Backtest, senão pelo primeiro connector do provedor.
- No [modo demo](/pt/guide/demo) o módulo é somente leitura: dois bots de exemplo numa conta paper de demonstração, um reduzido pela sua proteção e outro em pausa, nenhum armado nem rodando.
- Como um bot simulado executa no preço ao vivo, e as opções que ele compartilha com o backtest: [Backtest](/pt/modules/market-data#backtest).
