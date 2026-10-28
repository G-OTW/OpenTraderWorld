# Operar a partir do gráfico

Uma conta de broker pode fazer mais do que ler o seu livro: com o **trading** ativado, o gráfico envia-lhe ordens, move-as e cancela-as a partir da sua linha, e fecha posições a partir da delas. Hoje aceitam ordens três brokers:

- **Interactive Brokers (Gateway)**, através do IB Gateway ou da TWS que você mesmo executa. Esta página.
- **Binance** e **Binance USDⓈ-M Futures**, em contas reais e nos seus ambientes de teste (Spot Demo Mode, a testnet de futuros). Uma conta real aceita ordens assim que o [trading real](#live-trading) estiver ativo nela.
- **Paper trading**, uma conta simulada gerida pela própria aplicação. Veja [Conta de simulação](/pt/config/paper-trading).

Qualquer outro broker responde que não aceita ordens do OpenTraderWorld e continua em [somente leitura](/pt/config/brokers).

::: danger Experimental
Operar a partir do gráfico é **experimental**: por agora funciona com Interactive Brokers, Binance e Binance USDⓈ-M Futures. Foi amplamente testado, em mercados simulados e nos ambientes de teste dos brokers, mas podem subsistir erros. Verifique cada ordem no seu broker, comece pequeno e negocie por sua conta e risco.
:::

::: tip Contas reais
Interactive Brokers, Binance e Binance USDⓈ-M Futures estão classificados **E4**: uma conta real aceita ordens assim que ativar o [trading real](#live-trading) nela, com os seus limites definidos. As contas paper e os ambientes de teste não precisam disso.

Cancelar ordens nunca é bloqueado, seja qual for a conta.
:::

## Configuração

### 1. O Gateway

Execute o IB Gateway (ou a TWS) com sessão iniciada na sua conta paper ou real e, nas definições de API:

- marque **Enable ActiveX and Socket Clients**;
- desmarque **Read-Only API**, ou a própria IB recusa todas as ordens;
- com Docker Engine (Linux), desmarque **Allow connections from localhost only** e adicione `172.28.53.10` aos **Trusted IPs**.

O client id é escolhido pela aplicação (3100 a 3115), por isso várias sessões podem partilhar um Gateway.

### 2. A conta

Em **Definições → Brokers**, **Adicionar conta**, escolha **Interactive Brokers (Gateway)** e preencha:

| Definição | Valor |
|---|---|
| **Gateway host** | `host.docker.internal` quando a aplicação corre em Docker, `127.0.0.1` caso contrário |
| **Gateway port** | `4002` Gateway paper, `4001` Gateway real, `7497` TWS paper, `7496` TWS real |

Não há segredo a fornecer: o Gateway já tem sessão iniciada. **Testar ligação** verifica que responde.

### 3. O interruptor de trading

Cada linha de conta tem um chip **Trading**, ao lado dos chips de módulos. É separado de propósito:

- **Todos os módulos** não o inclui. Ler o seu livro e enviar ordens são duas permissões diferentes.
- **Ativá-lo pede a sua palavra-passe, sempre.** Desativá-lo não pede nada.
- As contas de paper trading são a exceção: sem palavra-passe, porque não há dinheiro real em jogo.

### 4. O gráfico

Um gráfico negoceia quando três condições se juntam:

1. há uma conta escolhida no **Broker book** do gráfico;
2. o trading está ativado para essa conta;
3. os dados do gráfico vêm do mesmo broker: uma conta IBKR negoceia um gráfico alimentado pelo conector Interactive Brokers.

Quando falta uma, o painel de ordens diz qual e como resolver. Os futuros precisam do **mês do contrato**: um futuro contínuo é recusado, por isso abra `MNQZ6` em vez do gráfico contínuo.

## O bloqueio de trading {#trading-lock}

Um segundo fator opcional para as ordens. Com o bloqueio ativo, a primeira ordem depois de iniciar sessão pede o código do seu autenticador, uma vez, e todas as contas de trading ficam abertas nesse navegador até terminar a sessão.

- Protege enviar, alterar e fechar. Cancelar, uma ordem ou todas, nunca pede nada.
- Abrange todas as contas de trading exceto as [contas paper](/pt/config/paper-trading).
- Ative-o em **Definições → Segurança → Bloqueio de trading**, depois de configurar a autenticação de dois fatores. Detalhes, códigos errados e como repô-lo: [Segurança da conta](/pt/config/security#trading-lock).

::: danger Remover o segundo fator com o bloqueio ativo desliga o trading em todo o lado
Remover a autenticação de dois fatores, nas Definições ou com `disable-totp`, desliga o bloqueio. Quando o bloqueio estava ativo, também desliga o trading em **todas** as contas, paper incluídas. Volte a ativá-lo conta a conta depois.
:::

## Enviar ordens

### O ticket

O separador **Ticket** do painel de ordens envia uma ordem, um bracket ou um OCO. **Oferece o que o broker da conta aceita, e nada mais**: um tipo, uma validade ou uma opção que o broker não declara não é mostrado (veja [O que cada broker aceita](#what-each-broker-takes)).

- **Tipos:**
  - **Market**, **Limit**, **Stop**, **Stop limit**.
  - **Trailing stop**: um stop que segue o mercado a uma distância, em percentagem ou em montante de preço, e nunca recua. **Start at**, onde o broker o tem, faz com que espere por um preço antes de começar a seguir.
  - **MIT** (market if touched) e **LIT** (limit if touched): o espelho de um stop, acionado quando o mercado chega ao preço pelo lado do limit (uma compra abaixo do mercado, uma venda acima), e depois uma ordem a mercado ou limit.
  - **OCO**: um limit e um stop do mesmo lado, em que o primeiro executado cancela o outro. Os dois preços têm de ser diferentes.
- **Saídas:** os interruptores **Take profit** e **Stop loss** adicionam a ordem oposta para a mesma quantidade. Ativar um pré-preenche-o a 0,5 % (take profit) ou 0,25 % (stop loss) da entrada. **Exits time in force** escolhe GTC (por omissão) ou Day. Onde o broker aceita ordens reduce-only, as saídas são enviadas reduce-only. Indisponível num OCO, e num broker que não anexa saídas. Na Interactive Brokers a entrada fica retida até as saídas serem aceites, assim o bracket sai inteiro.
- **Validade:** Day, GTC, GTD com data e hora futuras, IOC, FOK (tudo ou nada, de imediato).
- **Preços:** como preço, ou como distância ao bid, ask ou último (à entrada, para as saídas), em ticks quando o tick é conhecido.
- **Tamanho:** em **unidades** (ações, contratos), como **montante** de dinheiro (dividido pelo valor da entrada), ou como o dinheiro em **risco** até ao stop loss (dividido pela distância da entrada ao stop loss, que tem de estar ativado). Um future conta o valor do seu ponto; contratos e ações arredondam por defeito para unidades inteiras.
- **Opções**, cada uma mostrada apenas onde o broker a aceita:
  - **Reduce only**: só reduz a posição, nunca abre nem inverte uma.
  - **Post only**: um limit que apenas fica em repouso no livro; recusado em vez de executado de imediato.
  - **Iceberg**: o tamanho mostrado no livro, o resto oculto.
  - **Trigger on**: o preço que um stop ou gatilho vigia (último, mark, bid/ask, mid).
  - **Close position**: um stop ou MIT que fecha toda a posição no seu gatilho, seja qual for o seu tamanho.
  - **Outside RTH**: também é acionado e executado fora do horário regular.
  - **Adaptive algo**: o broker trabalha uma ordem a mercado ou limit para um melhor preço, paciente, normal ou urgente.

Antes de enviar seja o que for, a aplicação verifica:

- que cada preço e quantidade é maior que zero, e que cada opção se ajusta ao tipo (post-only num limit, um iceberg abaixo do tamanho da ordem, uma percentagem de trailing abaixo de 100);
- que o broker aceita o tipo, a validade e cada opção pedidos: o que não aceita é recusado, com o nome;
- que o preço está no tick do instrumento; caso contrário é recusado com os dois preços válidos mais próximos (`… is not on MNQZ6's 0.25 tick: use … or …`);
- que o instrumento se resolve no broker. **Nunca é adivinhado.**

Se uma perna de um ticket for recusada, todo o ticket é cancelado.

### As ordens que a aplicação acompanha {#followed-orders}

Cada ordem é guardada com o seu próprio id **antes** de ser enviada, e a resposta do broker é guardada com ela. Assim:

- **Um id de ordem é enviado uma só vez por conta, para sempre.** O gráfico dá a cada ordem um id novo; um ticket reenviado (depois de indicar uma margem, por exemplo) sai com ids novos. Um id já usado é recusado, mesmo após um reinício.
- **Não haver resposta não é uma recusa.** Uma ordem que o broker não confirma a tempo (10 segundos para o Gateway) é procurada no broker pelo seu id. Quando ainda não é encontrada, fica marcada como **desconhecida** e **nunca é reenviada**: as ordens novas nesse instrumento esperam que uma procura a resolva. Se continuar por resolver ao fim de 5 minutos, você recebe uma notificação (canal de notificações do gráfico) a pedir que verifique a ordem no broker.
- **Uma ordem que o broker nunca recebeu** (ainda não encontrada depois das procuras) fica à sua espera. O separador Ordens mostra *o broker nunca recebeu esta ordem* com **Enviar de novo**, que a envia com ids novos passando por todas as verificações de um ticket novo, e **Descartar**, que não envia nada. Também recebe uma notificação.
- **Um reinício não esquece nada.** As ordens enviadas mesmo antes de a aplicação parar são procuradas quando ela arranca, e as ordens que a própria aplicação liga (brackets da Binance USDⓈ-M) são retomadas.

Depois a aplicação compara a sua vista com a do broker, uns segundos após o arranque e em ciclo (a cada minuto enquanto tem ordens fora, a cada 10 minutos caso contrário, e de imediato quando um fluxo em direto assinala eventos perdidos). **O broker é quem tem razão**: uma ordem que foi executada ou cancelada enquanto ninguém olhava toma o estado do broker, e uma ordem colocada noutro sítio (TWS, o site da bolsa) é anotada como externa. Uma posição que mudou sem execuções que a expliquem é um **desvio**: fica escrita no registo de auditoria e é notificada.

### O que cada broker aceita {#what-each-broker-takes}

| | Interactive Brokers | Binance spot | Binance USDⓈ-M | Paper |
|---|---|---|---|---|
| **Tipos** | todos | todos | todos | todos |
| **Validade** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK |
| **Saídas, OCO** | ligadas pela IB | listas de ordens (OTO, OTOCO, OCO) | ligadas pela aplicação | ligadas pela aplicação |
| **Trailing stop** | montante ou %, a partir do mercado | %, em passos de 0,01 %, preço de início | % (0,1 a 20), preço de início | montante ou %, preço de início |
| **Reduce only** | | | sim | sim |
| **Post only** | | sim (limit maker) | sim (GTX) | sim |
| **Iceberg** | sim, unidades inteiras | sim, limits GTC | | |
| **Trigger on** | último, bid/ask, mid | | último, mark | |
| **Close position** | | | sim | |
| **Outside RTH** | sim | | | |
| **Adaptive algo** | sim | | | |

- **Binance spot** retém as saídas de uma entrada **limit** (OTO, OTOCO) e coloca as de uma entrada **market** depois de esta ser executada; uma entrada stop não aceita saídas. A metade limit de um OCO sai como limit maker. Uma saída de venda é pela quantidade comprada: uma comissão cobrada na própria moeda deixa menos, e a Binance recusa a saída.
- **Binance USDⓈ-M** não anexa nada por si: a aplicação envia as saídas como ordens reduce-only junto da sua entrada, e liga-as, tal como os pares OCO, lendo-as a cada 5 segundos. Após um reinício, a aplicação retoma as ligações a partir das ordens que guardou. O modo hedge é seguido: cada ordem indica o seu lado long ou short, e um símbolo mantido nos dois sentidos fecha-se um lado de cada vez.
- Os trailing stops da **Interactive Brokers** seguem desde o momento em que são colocados (sem preço de início) e mostram um iceberg em unidades inteiras.

### Binance

- **Chave API.** As ordens precisam de uma chave com o trading ativado («Enable Spot & Margin Trading» para spot, «Enable Futures» para USDⓈ-M) e os levantamentos desativados. Numa conta real, antes da primeira ordem de uma sessão, a aplicação lê os direitos da chave e recusa enviar quando o trading está desativado, os levantamentos estão ativados ou a chave não está restringida a IPs de confiança. Depois faz um ensaio da ordem na rota de teste da Binance.
- **Verificações antes de enviar.** Cada ordem é sujeita aos filtros do símbolo tal como a Binance os publica: tick, passo e limites de tamanho, tamanho das ordens a mercado, valor mínimo e máximo da ordem, a banda de preço em torno do preço de referência atual (mark price nos futuros) e o limite de ordens abertas por símbolo. Uma ordem recusada indica o filtro e os valores mais próximos que passariam. Nada é alterado por você.
- **Ritmo das ordens.** Quando o limite de ordens da conta seria excedido, a ordem é recusada com o número de segundos a esperar. Nunca fica em fila.
- **Alterar um stop de futuros.** A Binance não consegue alterar uma ordem condicional no local. A aplicação coloca primeiro o stop alterado e depois cancela o antigo, de modo que há sempre um stop ativo. A ordem recebe um id novo (`….r1`).
- **Se a aplicação desaparecer (USDⓈ-M).** Enquanto tem ordens ativas num símbolo de futuros, a aplicação mantém armada a contagem decrescente de cancelamento automático da Binance (120 s, renovada a cada 30 s). Se deixar de a renovar, a Binance cancela todas as ordens abertas nesse símbolo, incluindo as colocadas noutro sítio.
- **Posições.** As posições de futuros mostram a alavancagem e o modo de margem (cross ou isolated) definidos na Binance. A aplicação apenas os lê.
- **Separador Conta (spot).** O dinheiro é o saldo em USDT; o disponível é a sua parte livre. O equity é o valor que a própria Binance dá à carteira spot em USDT, apenas em real (o Spot Demo Mode não o publica).

### Interactive Brokers

- Um tamanho que o contrato não aceita (abaixo do seu mínimo, ou fora do seu passo de tamanho, por exemplo 0,5 de um future) é recusado antes de enviar seja o que for, com os tamanhos válidos.
- Um cancelamento que chega depois de a ordem ter sido executada não é um erro: a ordem aparece como executada.
- Enquanto a IB informar que perdeu os seus servidores (1100), as ordens novas e as alterações são recusadas; os cancelamentos continuam a passar.
- Numa conta real, a primeira ordem de cada ligação ao Gateway é enviada primeiro à IB como what-if. Se a IB a recusar, ou disser que a conta não a suporta, o ticket é recusado com o motivo da IB.

O ticket acopla-se em baixo, em cima, à esquerda ou à direita do gráfico, ou entre linhas ou colunas de um layout. Arraste a sua borda interior para o redimensionar (as setas também funcionam, Shift para passos maiores); o tamanho é guardado por posição.

### A barra rápida

A **barra rápida de Compra/Venda** (botão da barra de ferramentas) mostra Vender, uma quantidade e Comprar com o bid e o ask. **Um clique envia de imediato uma ordem a mercado, sem confirmação.** Arraste-a para onde quiser; encaixa na borda mais próxima.

### O `+` da escala de preços

Passe o rato pela escala de preços e clique em **+**: o menu propõe uma compra ou uma venda a esse preço, como limit ou stop consoante o lado do mercado em que está. É enviada de imediato, Day, ou abre-se no ticket com **Abrir no ticket…**.

## No gráfico

- **As ordens ativas** são desenhadas como linhas. Arraste a etiqueta de uma linha para mover a ordem (encaixa no tick); um movimento recusado repõe a linha. O seu **✕** cancela a ordem e as suas saídas.
- **Duplo clique** numa linha de ordem, ou o separador Ordens, abre-a no ticket para alterar preços, quantidade ou validade. O lado e o tipo não mudam. As ordens colocadas na TWS também aparecem e editam-se da mesma forma se forem market, limit, stop ou stop-limit.
- **As posições** são desenhadas no seu preço médio, com um **✕** para fechar a mercado. Fechar envia uma ordem a mercado pela quantidade que o broker detém agora e depois **cancela as saídas da posição**: as ordens ativas no seu instrumento, do lado do fecho, que o broker liga a uma entrada ou marca como reduce-only. As outras ordens no instrumento ficam.
- **Proteger com saídas** (o escudo no separador Posições) abre o ticket no gráfico da posição com um OCO do outro lado pela quantidade detida: um take profit e um stop loss, um cancelando o outro, reduce-only onde o broker o tem. A posição tem de estar aberta num gráfico dessa conta.
- **Brackets:** passar sobre uma ordem de um bracket desenha as suas ligações, tracejado verde para o take profit e vermelho para o stop loss.
- **Execuções:** com **Marcadores de entrada e saída** ativado, as execuções da conta no instrumento são desenhadas como marcadores no seu preço e na sua barra (uma compra aponta para cima, uma venda para baixo), dos últimos 30 dias no máximo. Vem desativado por omissão.

**Definições do gráfico → Trading** define como são desenhadas:

| | Opções |
|---|---|
| **Posição** | Resultado na linha (nenhum, percentagem, montante), Linha (largura total, curta, só etiqueta na escala de preços), mostrar o lado, mostrar a quantidade |
| **Ordem** | Linha, mostrar o lado, mostrar a quantidade |
| **Execuções** | Marcadores de entrada e saída, cores do Marcador de compra e do Marcador de venda |

## O painel de ordens

Ao lado de **Ticket**, filtrado por broker:

| Separador | Mostra |
|---|---|
| **Posições** | símbolo, lado, quantidade, preço médio, preço de mercado, P&L, hora de abertura e comissões de entrada (das execuções dos últimos 7 dias), com botão de fecho |
| **Ordens** | tipo, quantidade, limit, stop e estado, com editar e cancelar. O seu **Cancelar tudo** cancela as ordens listadas neste navegador |
| **Conta** | os valores que o broker indica, ver abaixo |
| **Registos** | as últimas 200 chamadas enviadas ao broker, do registo de auditoria |

### Valores da conta

**Equity, Caixa, Resultado aberto, Realizado, Margem inicial, Margem de manutenção, Disponível, Liquidez excedente, Poder de compra, Valor das posições**, na moeda base da conta, uma linha por conta que o Gateway alcança. Para a Interactive Brokers são os da IB (`NetLiquidation`, `TotalCashValue`, `UnrealizedPnL`, o `RealizedPnL` do dia, `InitMarginReq`, `MaintMarginReq`, `AvailableFunds`, `ExcessLiquidity`, `BuyingPower`, `GrossPositionValue`). Um valor que o broker não indica não é mostrado: a IB não dá um total de comissões, por isso essa linha não aparece.

### Atualizações em direto

As ordens, execuções e posições chegam como o broker as comunica, por um único fluxo para todas as contas de trading da página: as mensagens próprias do Gateway na Interactive Brokers, o user data stream da Binance na Binance (a API WebSocket no spot, a listen key da conta no USDⓈ-M, ambas com a chave já configurada da conta). As posições de futuros em modo hedge são relidas por inteiro em vez de atualizadas no local. O livro também é relido a cada 30 segundos, e um segundo depois de uma posição se mover. Se o fluxo cair, a página passa a ler o livro a cada 4 segundos. Um Gateway que se desliga é religado na chamada seguinte; entretanto as ordens ativas continuam na IB.

## Limites de risco e corta-circuito {#risk-limits}

Os seus próprios limites, verificados antes de qualquer ordem sair, em cada conta de trading (paper incluída). Cada um fica desativado até o definir, em **Definições → Brokers → Limites de risco** da conta:

| Limite | Recusa |
|---|---|
| **Maior ordem (tamanho)** | uma ordem maior do que este número de unidades |
| **Maior valor de ordem (moeda da conta)** | uma ordem que valha mais do que isto, na moeda da conta (tamanho × preço × valor do ponto de um future) |
| **Maior posição (tamanho)** | uma ordem que deixaria mais do que este número de unidades detidas no instrumento |
| **Maior valor de posição (moeda da conta)** | uma ordem que deixaria uma posição que valha mais do que isto, na moeda da conta (tamanho detido × preço × valor do ponto de um future) |
| **Faixa de preço (%)** | um limit ou stop a mais do que esta percentagem do último preço do gráfico |
| **Ordens por minuto** | mais ordens do que este número no último minuto |
| **Ordens abertas ao mesmo tempo** | mais ordens ativas do que este número |
| **Limite de perda por período (moeda da conta)** | qualquer ordem assim que a conta perdeu isto desde o início do período (hora, dia ou semana, ver abaixo): a conta é suspensa |
| **Janela de duplicata (segundos)** | a mesma ordem (lado, tipo, tamanho, preços) enviada de novo dentro destes segundos, até confirmar |

- Uma violação é recusada com o limite e o valor. **A ordem nunca é redimensionada para caber.**
- **Reduzir risco passa sempre**: uma ordem reduce-only, um fecho ou uma ordem que apenas reduz o que é detido atravessa todos os limites e todas as suspensões.
- Alterar um limite pede a sua palavra-passe.

### Suspensões

Uma suspensão recusa novas ordens até terminar; os cancelamentos e as ordens que reduzem uma posição continuam a sair, e as ordens ativas não podem ser alteradas entretanto (cancele-as).

- **Corta-circuito**: o botão em **Definições → Brokers** e no painel de ordens do gráfico cancela todas as ordens ativas de **todas** as contas de trading (incluindo as colocadas noutro sítio, cada cancelamento no registo de auditoria) e depois suspende-as todas. Pede uma confirmação, nunca uma palavra-passe.
- **Limite de perda**: uma conta que atinge o seu limite de perda suspende-se sozinha (verificado antes de cada ordem e no ciclo de reconciliação), e recebe uma notificação.
- **Desvio de posição**: quando uma posição se move sem execuções que o expliquem (uma ordem que a aplicação não viu, uma execução corrigida), a conta suspende-se até a **reconhecer**, e recebe uma notificação. Verifique primeiro a conta no broker; reconhecer pede sempre a sua palavra-passe, seja qual for a política abaixo.

Em **Definições → Brokers → Risco e corta-circuito** escolhe como termina uma suspensão, para todas as contas:

- **após**: o início da hora, do dia ou da semana seguinte (segunda-feira), no **Fuso horário** da política; ou **só quando levantada**. O fuso é UTC até guardar um, e o formulário começa no fuso do seu navegador;
- **levantar mais cedo com a minha palavra-passe**: uma suspensão pode ser levantada antes do fim a partir do mesmo local ou do banner do gráfico. Uma suspensão que só termina quando levantada levanta-se sempre com a palavra-passe.

Guardar a política pede a sua palavra-passe.

O período de perda segue a mesma escolha (um dia quando as suspensões só terminam quando levantadas). A perda mede-se a partir do equity da conta no início do período, tal como o broker o indica; um depósito ou levantamento durante o período conta. Um broker que não indica equity (Binance Spot Demo Mode) não pode ficar sujeito a um limite de perda: as suas ordens são recusadas enquanto houver um definido.

## Trading real {#live-trading}

O circuito de ordens de cada broker tem um nível, mostrado na conta em **Definições → Brokers**: E2 (testado num mercado simulado), E3 (testado, durante dias, no ambiente de teste do próprio broker), E4 (passaram um ensaio em real e uma ordem real do tamanho mínimo). Abaixo de E4, uma conta que opera com dinheiro real é recusada pelo próprio conector.

Em E4, uma conta real continua sem aceitar ordens até ativar **Trading real** nela:

- é preciso o interruptor de trading ligado, e o **limite de tamanho ou valor por ordem**, o **limite de tamanho ou valor por posição** e o **limite de perda** da conta definidos;
- o [bloqueio de trading](#trading-lock) é opcional. Ativo, ativar o trading real e a primeira ordem de cada sessão pedem o código do seu autenticador. Desativado, a conta mostra um aviso e as suas ordens saem sem código;
- a conta mostra o nível, se o broker cancela as suas ordens quando a aplicação para (Binance USDⓈ-M: sim; spot e Interactive Brokers: não, as ordens continuam ativas) e que limites estão definidos;
- desativá-lo não pede nada; desligar o trading também o desativa, e tem de ser ativado de novo.

Os ambientes de teste, as contas demo e as contas paper nunca precisam dele.

## Registo de auditoria e métricas de ordens {#audit-log}

Cada envio, alteração, cancelamento, cancelamento global e fecho é escrito num **registo de auditoria só de acrescentar**: hora, conta, broker, ação, pedido, resposta ou erro do broker e latência. A base de dados recusa alterar ou apagar uma linha. As chamadas recusadas antes de chegarem ao broker porque o trading está desativado ou o bloqueio fechado não são registadas; qualquer outra recusa é. O que a [reconciliação](#followed-orders) encontra (uma execução ou cancelamento perdido, uma ordem externa, uma ordem resolvida após uma resposta perdida, um desvio de posição) também é registado, como **reconcile**, e não conta nas métricas de ordens.

Em **Definições → Brokers**, cada conta de trading mostra as suas **métricas de ordens** (chamadas, enviadas, recusadas, latência p50 / p99, última ordem) e um botão **Cancelar todas as ordens**. Ao contrário do [corta-circuito](#suspensoes), abrange apenas essa conta e não suspende nada:

- pede confirmação e depois cancela **todas as ordens ativas** da conta no broker, incluindo as colocadas fora do OpenTraderWorld. Na Interactive Brokers são todas as ordens das contas do Gateway, incluindo as colocadas à mão na TWS;
- indica quantas foram canceladas. Na Interactive Brokers espera primeiro até 10 segundos até não restar nenhuma ativa;
- funciona com o trading desligado e com o bloqueio fechado.

## Limites

- Na Interactive Brokers, as ordens vão para a conta por omissão do Gateway.
- Não se muda o lado nem o tipo de uma ordem ativa: cancele e envie uma nova.
- As ordens são na moeda do contrato, sem conversão.
- Os agentes de IA ([MCP](/pt/config/ai-agents)) e os workflows do Automator não podem enviar ordens.
- As contas de broker, e portanto o trading, estão desativadas no [modo demo](/pt/guide/demo).
