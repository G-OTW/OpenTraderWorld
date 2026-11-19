# Operar a partir do gráfico

Uma conta de broker pode fazer mais do que ler o seu livro: com o **trading** ativado, o gráfico envia-lhe ordens, move-as e cancela-as a partir da sua linha, e fecha posições a partir da delas. Hoje aceitam ordens estes brokers:

- **Interactive Brokers (Gateway)**, através do IB Gateway ou da TWS que você mesmo executa. Esta página.
- **Binance** e **Binance USDⓈ-M Futures**, em contas reais e nos seus ambientes de teste (Spot Demo Mode, a testnet de futuros). Uma conta real aceita ordens assim que o [trading real](#live-trading) estiver ativo nela.
- **Bybit** (book spot ou linear), em contas reais e na testnet da Bybit. Classificado **E2D**: construído a partir da documentação da Bybit e nunca testado contra a Bybit; uma conta real aceita ordens assim que o [trading real](#live-trading) estiver ativo nela com a aceitação.
- **Bybit EU** (spot), em contas reais e na testnet da Bybit EU. Classificado **E2D** como a Bybit, com a mesma aceitação para uma conta real.
- **OKX** (spot, swaps perpétuos e futuros com vencimento), em contas reais e no trading demo da OKX. Classificado **E2D**, com a mesma aceitação para uma conta real.
- **Bitget** (spot e futuros USDT-M), em contas reais e no trading demo da Bitget. Classificado **E2D**.
- **Gate.io** (spot e perpétuos USDT), em contas reais e na testnet da Gate.io. Classificado **E2D**.
- **KuCoin** (spot) e **KuCoin Futures**, só em contas reais: a KuCoin não tem ambiente de teste. Classificado **E2D**.
- **Kraken** (spot), só em contas reais: a Kraken não tem ambiente de teste spot. Classificado **E2D**.
- **Kraken Futures** (contratos multicolateral, PF_ e FF_), em contas reais e no demo da Kraken Futures. Classificado **E2D**, com a mesma aceitação para uma conta real.
- **Coinbase Advanced Trade** (spot), só em contas reais: a Coinbase não tem conta de teste. Classificado **E2D**.
- **MEXC** (spot e perpétuos USDT), só em contas reais: a MEXC não tem ambiente de teste. Classificado **E2D**.
- **Bitfinex** (carteira exchange spot e perpétuos), só em contas reais: a Bitfinex não tem host de teste. Classificado **E2D**.
- **Deribit** (futuros e perpétuos: contratos lineares USDC e contratos inversos BTC e ETH), em contas reais e em test.deribit.com. Classificado **E2D**, com a mesma aceitação para uma conta real.
- **Paper trading**, uma conta simulada gerida pela própria aplicação. Veja [Conta de simulação](/pt/config/paper-trading).

Qualquer outro broker responde que não aceita ordens do OpenTraderWorld e continua em [somente leitura](/pt/config/brokers).

::: danger Experimental
Operar a partir do gráfico é **experimental**: funciona com Interactive Brokers, Binance, Binance USDⓈ-M Futures, Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex e Deribit, e com o paper trading. A Interactive Brokers e as duas contas Binance foram amplamente testadas, em mercados simulados e nos ambientes de teste dos brokers. Os outros foram feitos a partir da documentação de cada broker e ainda não foram experimentados numa conta real. Podem subsistir erros em qualquer um deles. Verifique cada ordem no seu broker, comece pequeno e negocie por sua conta e risco.
:::

::: tip Contas reais
Interactive Brokers, Binance e Binance USDⓈ-M Futures estão classificados **E4**: uma conta real aceita ordens assim que ativar o [trading real](#live-trading) nela, com os seus limites definidos. As contas paper e os ambientes de teste não precisam disso.

Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex e Deribit estão classificados **E2D**: uma conta testnet ou demo aceita ordens de imediato; uma conta real precisa do trading real ativo com a aceitação de que o broker nunca foi testado contra a plataforma (ver [Trading real](#live-trading)).

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

| | Interactive Brokers | Binance spot | Binance USDⓈ-M | Paper | Bybit | Bybit EU | OKX | Bitget | Gate.io | KuCoin | Kraken | Kraken Futures | Coinbase | MEXC | Bitfinex | Deribit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **Tipos** | todos | todos | todos | todos | spot: market, limit; linear: market, limit, stop, stop limit | market, limit | mercado, limite | spot: mercado, limite, stop; futuros: mercado, limite, stop, stop limite | mercado, limite | spot: mercado, limite; futuros: mercado, limite, stop, stop limite | mercado, limite, stop, stop limite, MIT, LIT | mercado, limite, stop, stop limite, MIT, LIT | mercado, limite, stop limite, LIT | mercado, limite | mercado, limite, stop, stop limite | mercado, limite, stop, stop limite, MIT, LIT |
| **Validade** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK (uma ordem market vai IOC: FOK recusado) | GTC, IOC, FOK (uma ordem market vai IOC: FOK recusado) | GTC, IOC, FOK (FOK recusado numa ordem a mercado) | GTC, IOC, FOK (FOK recusado numa ordem a mercado; um stop é GTC) | GTC, IOC, FOK (uma ordem a mercado funciona IOC; futuros: FOK recusado numa ordem a mercado) | GTC, IOC, FOK (FOK só num limite) | GTC, GTD, IOC, FOK (FOK só num limite; IOC num limite ou numa ordem a mercado) | GTC, IOC, FOK (uma ordem a mercado funciona IOC: FOK recusado; stops, MIT e LIT GTC) | GTC, GTD, IOC, FOK (uma ordem a mercado funciona IOC: FOK e GTD recusados; um stop limite é GTC ou GTD) | GTC, IOC, FOK (FOK só num limite) | GTC, GTD, IOC, FOK (IOC e FOK só num limite; um stop é GTC ou GTD) | GTC, Day, IOC, FOK (Day recusado numa ordem a mercado) |
| **Saídas, OCO** | ligadas pela IB | listas de ordens (OTO, OTOCO, OCO) | ligadas pela aplicação | ligadas pela aplicação | linear: take profit e stop loss sobre toda a posição, a mercado | | | futuros: take profit e stop loss de uma posição, movidos ou retirados no painel de ordens | | | | | | | | |
| **Trailing stop** | montante ou %, a partir do mercado | %, em passos de 0,01 %, preço de início | % (0,1 a 20), preço de início | montante ou %, preço de início | | | | | | | | | | | | |
| **Reduce only** | | | sim | sim | linear | | swaps e futuros | futuros | futuros | futuros | | sim | | futuros | perpétuos | sim |
| **Post only** | | sim (limit maker) | sim (GTX) | sim | | | sim | sim | | sim | sim | sim | sim | sim | sim | sim |
| **Iceberg** | sim, unidades inteiras | sim, limits GTC | | | | | | | | | | | | | | |
| **Trigger on** | último, bid/ask, mid | | último, mark | | último (stops linear) | | | spot: último; futuros: último, mark | | último (stops de futuros) | último | último, mark, index | último | | | último, mark, index (indicado em cada stop) |
| **Close position** | | | sim | | | | | | | | | | | | | |
| **Outside RTH** | sim | | | | | | | | | | | | | | | |
| **Adaptive algo** | sim | | | | | | | | | | | | | | | |

- **Binance spot** retém as saídas de uma entrada **limit** (OTO, OTOCO) e coloca as de uma entrada **market** depois de esta ser executada; uma entrada stop não aceita saídas. A metade limit de um OCO sai como limit maker. Uma saída de venda é pela quantidade comprada: uma comissão cobrada na própria moeda deixa menos, e a Binance recusa a saída.
- **Binance USDⓈ-M** não anexa nada por si: a aplicação envia as saídas como ordens reduce-only junto da sua entrada, e liga-as, tal como os pares OCO, lendo-as a cada 5 segundos. Após um reinício, a aplicação retoma as ligações a partir das ordens que guardou. O modo hedge é seguido: cada ordem indica o seu lado long ou short, e um símbolo mantido nos dois sentidos fecha-se um lado de cada vez.
- Os trailing stops da **Interactive Brokers** seguem desde o momento em que são colocados (sem preço de início) e mostram um iceberg em unidades inteiras.

### Bybit

- **Cada ordem vai para o book do seu instrumento.** Uma conta Bybit mostra as suas ordens e posições de cada book que lê. Uma ordem vai para o book do instrumento do gráfico: aquele em que o conector de dados Bybit do gráfico está definido (o seu **Market**: spot ou linear), ou senão o único book que lista o símbolo. Um símbolo listado por ambos os books (`BTCUSDT`, par spot e perpétuo) num gráfico com dados de outra fonte é recusado com essa correção. O ticket oferece o que esse book aceita.
- **Testnet.** Defina o **Site** da conta como `testnet` (uma chave de testnet.bybit.com): as suas ordens não movimentam dinheiro e não precisam de trading real.
- **Verificações antes do envio.** Tick, passo de tamanho, tamanho mínimo e máximo (limit e market à parte) e valor mínimo de ordem, da lista de instrumentos da Bybit. Uma ordem recusada nomeia a regra e os valores mais próximos; nada é alterado. Um instrumento que não negoceia agora (pré-lançamento, em entrega, fechado) não aceita novas ordens; os cancelamentos continuam a sair.
- **Modo de posição.** O modo one-way ou hedge é lido da sua conta Bybit (uma vez, e depois mantido atualizado pelo fluxo da Bybit) e nunca alterado. Cada ordem linear nomeia a posição sobre a qual atua.
- **Stops de proteção.** Um stop linear reduce-only sai com o *close on trigger* da Bybit: reduz a posição mesmo quando falta margem, e a Bybit pode então cancelar ou reduzir outras ordens nesse contrato.
- **Antes da primeira ordem real de uma sessão** (linear), a ordem passa pela verificação prévia da Bybit, que responde a margem que ocuparia; uma recusa aí não envia nada. Uma primeira ordem condicional é recusada: envie primeiro uma ordem market ou limit simples.
- **Atualizações em direto.** As ordens seguem o fluxo privado da Bybit; as posições são relidas quando mudam. Se o fluxo não estiver disponível (uma chave RSA, um site regional sem fluxo), a conta é lida por ciclos.
- **Se a aplicação parar**, a Bybit mantém as suas ordens ativas: nada as cancela.
- **Take profit e stop loss (linear).** O take profit e o stop loss de um ticket saem com a entrada numa só chamada e cobrem toda a posição (modo *Full* da Bybit): cada um sai como ordem a mercado quando o seu preço é negociado. A Bybit cria-os quando a entrada é executada; aparecem no painel de ordens como ordens take profit e stop loss ligadas à entrada enquanto ela trabalha. Movê-los ou retirá-los altera-os na posição. Um take profit limit sai a mercado, não como limit.
- **Ainda não oferecido:** take profit e stop loss em spot ou sobre parte da posição, OCO, alterar uma ordem no lugar, trailing stops, post only.

### Bybit EU

- **Só spot**, pelo mesmo circuito de ordens que a Bybit spot: ordens market e limit, as mesmas verificações antes do envio, uma ordem market dimensionada na moeda base.
- **Testnet.** Defina o **Site** da conta como `eu-testnet` (uma chave de testnet.bybit.eu): as suas ordens não movimentam dinheiro e não precisam de trading real.
- **Atualizações em direto.** A Bybit não documenta nenhum fluxo privado para a Bybit EU: a conta é lida por ciclos.
- **A sua chave.** O guia da Bybit diz que a API do site EU serve aplicações de terceiros para utilizadores API broker. Uma chave pessoal pode então ser recusada: a ordem mostra a recusa da Bybit com o seu código, e nada mais é enviado.

### OKX

- **Books.** Spot, swaps perpétuos e futuros com vencimento. Cada ordem vai para o book do seu instrumento: `BTC-USDT` é spot, `BTC-USDT-SWAP` um swap, `BTC-USDT-251226` um futuro com vencimento.
- **Região.** Defina a **Região** da conta como aquela onde foi aberta: `global` (okx.com, por omissão), `eea` (my.okx.com) ou `us` (app.okx.com, Estados Unidos e Austrália). Uma chave só funciona na sua região.
- **Trading demo.** Defina **Ambiente** como `demo` com uma chave de trading demo: as suas ordens não movem dinheiro e não precisam do trading real.
- **Tamanhos.** Swaps e futuros negociam-se em contratos: um tamanho que não é um número inteiro de contratos é recusado, indicando os tamanhos mais próximos.
- **Alterar uma ordem** no lugar: o seu preço e o seu tamanho.
- **O modo de posição** (net ou long/short) é lido da sua conta e nunca alterado. A alavancagem é mostrada, nunca definida. As ordens spot usam só as suas próprias moedas, nunca margem.
- **Não oferecido:** ordens stop e de gatilho, take profit e stop loss, trailing stops, OCO, iceberg, opções, contratos inversos.

### Bitget

- **Books.** Spot e futuros USDT-M. Uma ordem vai para o book definido no conector de dados Bitget do gráfico (o seu **Mercado**: `spot`, ou `usdt-futures`, também escrito `futures`), ou senão para o único book que lista o símbolo. Um símbolo que ambos os books listam, num gráfico cujos dados vêm de outro lado, é recusado com essa correção.
- **Trading demo.** Defina **Negociação demo** como `yes` com uma chave demo da Bitget: as suas ordens não movem dinheiro e não precisam do trading real.
- **Os stops** seguem como ordens de gatilho da Bitget, sobre o último preço (futuros: último ou mark). Um stop spot executa a mercado.
- **Uma compra spot a mercado** é um montante a gastar na moeda de cotação: a aplicação lê o melhor ask mesmo antes de enviar e envia esse montante, por isso o tamanho executado pode diferir um pouco. A resposta di-lo.
- **Todas as ordens da conta** aparecem no painel de ordens, ordens de gatilho incluídas, e cancelam-se a partir dele.
- **Take profit e stop loss de uma posição (futuros).** O que está definido numa posição aparece no painel de ordens; movê-lo ou retirá-lo aí altera-o na posição.
- **O modo de posição e a margem** são lidos da sua conta e nunca alterados. A alavancagem é mostrada, nunca definida.
- **Não oferecido:** alterar uma ordem no lugar, um take profit e stop loss enviados com a entrada, trailing stops, stop limite spot, OCO.

### Gate.io

- **Books.** Spot e os perpétuos USDT, escolhidos como na Bitget: o **Mercado** do conector de dados do gráfico (`spot`, ou `futures`, também escrito `usdt-futures`), ou senão o único book que lista o símbolo.
- **Testnet.** Defina o **Site** da conta como `testnet` com uma chave de testnet: as suas ordens e leituras vão para a testnet da Gate.io, não movem dinheiro e não precisam do trading real.
- **Uma compra spot a mercado** é um montante a gastar na moeda de cotação, avaliado ao melhor ask lido mesmo antes de enviar e arredondado para baixo à precisão do par. O tamanho executado pode diferir um pouco; a resposta di-lo.
- **Os tamanhos de futuros** são contratos inteiros: um tamanho que não o é é recusado, indicando os tamanhos mais próximos.
- **Alterar uma ordem** no lugar: em spot o seu preço ou o seu tamanho (um de cada vez), em futuros o seu preço e o seu tamanho.
- **Não oferecido:** stops, take profit e stop loss, trailing stops, post only, OCO, iceberg.

### KuCoin e KuCoin Futures

- **Sem ambiente de teste.** A KuCoin não tem: todas as contas são reais e precisam do trading real ativo com a aceitação. Antes da primeira ordem de uma sessão, a aplicação envia-a ao teste de ordens da KuCoin, que a verifica sem a colocar.
- **Spot:** ordens a mercado e limite, com tamanho na moeda base.
- **Futuros:** mercado, limite, stop e stop limite sobre o último preço, reduce only, post only. Os tamanhos são contratos inteiros. Os stops aparecem no painel de ordens e cancelam-se a partir dele.
- **O modo de posição e de margem** é lido da sua conta e nunca alterado. A alavancagem é mostrada, nunca definida.
- **Não oferecido:** alterar uma ordem no lugar, um take profit e stop loss enviados com a entrada, stops spot, OCO, futuros com margem em moeda, trading com margem.

### Kraken

- **Sem ambiente de teste.** A Kraken não tem para o spot: todas as contas são reais e precisam do trading real ativo com a aceitação. Antes da primeira ordem de uma sessão, a aplicação envia-a à chamada de validação da Kraken, que a verifica sem a colocar.
- **A sua chave.** Dê-lhe Query Funds, Query Open Orders & Trades, Query Closed Orders & Trades, Create & Modify Orders, Cancel/Close Orders e Access WebSocket API, nunca Withdraw Funds. Dê à aplicação uma chave só sua: a Kraken recusa uma chamada cujo nonce não seja superior ao último da chave, por isso uma chave que outro programa também use falha ao acaso.
- **Tamanhos e comissões.** As ordens têm o tamanho na moeda base; as comissões são cobradas na moeda de cotação.
- **Alterar uma ordem** no lugar: o seu preço, o gatilho e o tamanho. Cancelar tudo é uma só chamada.
- **Não oferecido:** margem, uma ordem de fecho ligada à entrada, trailing stops, iceberg, gatilhos sobre o preço index, ações tokenizadas.

### Kraken Futures

- **Contratos.** As ordens vão para os contratos multicolateral (PF_, FF_), com tamanho na moeda. Os contratos inversos (PI_, FI_) aparecem com as suas ordens e cancelam-se no painel de ordens, mas não aceitam novas ordens.
- **Demo.** Defina o **Ambiente** da conta como `demo` com uma chave do demo da Kraken Futures: as suas ordens não movem dinheiro e não precisam do trading real.
- **A sua chave.** General API: Full Access para enviar ordens; uma chave Read Only continua a ler. Nunca conceda a Withdrawal API.
- **Alterar uma ordem** no lugar: o seu tamanho, o limit e o gatilho.
- **Todas as ordens da conta** aparecem no painel de ordens, ordens de gatilho incluídas, e cancelar tudo abrange-as.
- **O modo de posição** é one-way. A alavancagem é mostrada, nunca definida.
- **Atualizações em direto.** Não se usa nenhum fluxo privado: a conta é lida por ciclos. A comissão de uma execução é mostrada sem a sua moeda, que a Kraken não indica.
- **Não oferecido:** trailing stops, um take profit e stop loss enviados com a entrada, OCO, opções, ordens em contratos inversos.

### Coinbase

- **Sem conta de teste.** A Coinbase não tem: todas as contas são reais e precisam do trading real ativo com a aceitação. Antes da primeira ordem de uma sessão, a aplicação envia-a à pré-visualização de ordens da Coinbase.
- **Só spot**, com tamanho na moeda base. Um stop limite dispara sobre o último preço.
- **Verificações antes de enviar.** Tick, passo de tamanho, tamanho mínimo e máximo, a partir da lista de produtos da Coinbase. Um produto em modo só limit ou só post only aceita apenas essas ordens.
- **Alterar uma ordem** no lugar: o preço e o tamanho de um limit.
- **Todas as ordens da conta** aparecem no painel de ordens, incluindo as ordens colocadas na app da Coinbase.
- **A sua chave.** Precisa da permissão Trade para enviar ordens; uma chave View-only continua a ler.
- **Atualizações em direto.** As ordens seguem o fluxo de utilizador da Coinbase; as execuções são lidas por ciclos. Uma execução que a Coinbase ajustou (estorno, correção) fica de fora da importação e é indicada no registo do servidor; as outras execuções são mantidas.
- **Se a aplicação desaparecer**, a Coinbase mantém as suas ordens a funcionar.
- **Não oferecido:** stop market, take profit e stop loss, trailing stops, OCO, futuros e perpétuos.

### MEXC

- **Sem ambiente de teste.** A MEXC não tem: todas as contas são reais e precisam do trading real ativo com a aceitação. Antes da primeira ordem spot de uma sessão, a aplicação envia-a ao teste de ordens da MEXC.
- **Books.** Spot e os perpétuos USDT, escolhidos como na Bitget: o **Mercado** do conector de dados do gráfico, ou senão o único book que lista o símbolo.
- **Uma compra spot a mercado** é um montante a gastar na moeda de cotação, avaliado ao melhor ask lido mesmo antes de enviar e arredondado para baixo à precisão do par.
- **Os tamanhos de futuros** são contratos inteiros. As novas ordens só saem em modo hedge: numa conta one-way são recusadas antes de enviar qualquer coisa (passe a conta para modo hedge na MEXC), enquanto uma posição continua a fechar-se. O modo de margem e a alavancagem são lidos, nunca definidos.
- **Todas as ordens da conta** aparecem no painel de ordens, incluindo planos de futuros, take profit e stop loss, e ordens trailing colocadas noutro lado, e cancelar tudo abrange-as.
- **A sua chave.** Trading spot e colocação de ordens de futuros, e cada par spot adicionado aos pares de trading da chave; a colocação de ordens de futuros exige KYC.
- **Não oferecido:** colocar stops, take profit e stop loss ou trailing stops, ordens chase, alterar uma ordem no lugar, OCO, futuros com margem em moeda.

### Bitfinex

- **Sem host de teste.** A Bitfinex não tem: todas as contas são reais, sub-contas paper incluídas, e precisam do trading real ativo com a aceitação.
- **Books.** Os pares spot da carteira exchange e os perpétuos (`BTCF0:USTF0`). Uma ordem vai para o book definido no conector de dados Bitfinex do gráfico (o seu **Mercado**: spot, ou derivatives, também escrito perp ou futures), ou senão para o único book que lista o símbolo. O trading com margem não é oferecido.
- **A alavancagem** é a da posição mantida, enviada só quando é um número inteiro de 1 a 100; sem posição, a Bitfinex aplica a sua predefinição (10). A aplicação nunca a escolhe.
- **Os preços** têm no máximo 5 algarismos significativos: outro preço é recusado com os dois mais próximos.
- **Alterar uma ordem** move só os seus preços: para mudar o tamanho, cancele-a e coloque uma nova.
- **A sua chave.** Acesso de escrita a Orders para enviar; uma chave só de leitura continua a ler. Dê à aplicação uma chave só sua (a Bitfinex verifica um nonce crescente por chave).

### Deribit

- **Books.** Futuros e perpétuos: contratos lineares USDC com tamanho na moeda, contratos inversos BTC e ETH com tamanho em USD. Num contrato inverso o ticket aceita o tamanho na moeda ou em USD: um tamanho em USD é arredondado para baixo ao passo do contrato (10 USD em BTC), um tamanho na moeda é convertido ao último preço e depois arredondado para baixo. Posições e ordens em contratos inversos aparecem em USD. Os seus limites de risco avaliam uma ordem dada em USD por esse montante.
- **Ambiente de teste.** Defina o **Ambiente** da conta como `test` com uma chave de test.deribit.com: as suas ordens não movem dinheiro e não precisam do trading real.
- **Os stops** precisam do seu gatilho (último, mark ou index): não há predefinição.
- **Post only** é recusado em vez de ser movido quando seria executado de imediato.
- **Alterar uma ordem** no lugar: o seu preço, o gatilho e o tamanho. Um stop que disparou é seguido até à ordem em que se tornou.
- **Todas as ordens da conta** aparecem no painel de ordens, gatilhos incluídos, e cancelar tudo abrange-as.
- **Uma posição por instrumento.** A alavancagem é mostrada, nunca definida.
- **Não oferecido:** opções, spot, um take profit e stop loss enviados com a entrada, OCO, trailing stops, iceberg.

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
| **Limite de perda por período** | qualquer ordem assim que a conta perdeu isto desde o início do período (hora, dia ou semana, ver abaixo): a conta é suspensa. A sua unidade é um **Montante** na moeda da conta ou uma **% do patrimônio** com que o período começou |
| **Janela de duplicata (segundos)** | a mesma ordem (lado, tipo, tamanho, preços) enviada de novo dentro destes segundos, até confirmar |

- Uma violação é recusada com o limite e o valor. **A ordem nunca é redimensionada para caber.**
- **Reduzir risco passa sempre**: uma ordem reduce-only, um fecho ou uma ordem que apenas reduz o que é detido atravessa todos os limites e todas as suspensões.
- Alterar um limite pede a sua palavra-passe.

### Suspensões

Uma suspensão recusa novas ordens até terminar; os cancelamentos e as ordens que reduzem uma posição continuam a sair, e as ordens ativas não podem ser alteradas entretanto (cancele-as).

- **Corta-circuito**: cancela todas as ordens ativas de **todas** as contas de trading (incluindo as colocadas noutro sítio, cada cancelamento no registo de auditoria) e depois suspende-as todas. Em **Definições → Brokers** pede uma confirmação. No gráfico fica na barra de ferramentas ao lado de **Congelar**, enquanto uma conta opera, e funciona da mesma forma: um clique arma-o durante 3 segundos, um segundo clique dispara-o. Nunca uma palavra-passe.
- **Congelar** (barra de ferramentas do gráfico): bloqueia qualquer nova entrada enviada a partir dos gráficos, em todas as contas ligadas (paper incluída), durante 10 minutos, e cancela as ordens de entrada ativas. Fechar, reduzir, os stops e os alvos continuam a funcionar. Um clique arma-o, um segundo inicia-o; nada o termina mais cedo.
- **Limite de perda**: uma conta que atinge o seu limite de perda suspende-se sozinha (verificado antes de cada ordem e no ciclo de reconciliação), e recebe uma notificação.
- **Desvio de posição**: quando uma posição se move sem execuções que o expliquem (uma ordem que a aplicação não viu, uma execução corrigida), a conta suspende-se até a **reconhecer**, e recebe uma notificação. Verifique primeiro a conta no broker; reconhecer pede sempre a sua palavra-passe, seja qual for a política abaixo.

Em **Definições → Brokers → Risco e corta-circuito** escolhe como termina uma suspensão, para todas as contas:

- **após**: o início da hora, do dia ou da semana seguinte (segunda-feira), no **Fuso horário** da política; ou **só quando levantada**. O fuso é UTC até guardar um, e o formulário começa no fuso do seu navegador;
- **levantar mais cedo com a minha palavra-passe**: uma suspensão pode ser levantada antes do fim a partir do mesmo local ou do banner do gráfico. Uma suspensão que só termina quando levantada levanta-se sempre com a palavra-passe.

Guardar a política pede a sua palavra-passe.

O período de perda segue a mesma escolha (um dia quando as suspensões só terminam quando levantadas). A perda mede-se a partir do equity da conta no início do período, tal como o broker o indica; um depósito ou levantamento durante o período conta. Um broker que não indica equity (Binance Spot Demo Mode) não pode ficar sujeito a um limite de perda: as suas ordens são recusadas enquanto houver um definido.

## Trading real {#live-trading}

O circuito de ordens de cada broker tem um nível, mostrado na conta em **Definições → Brokers**: E2 (testado num mercado simulado), E3 (testado, durante dias, no ambiente de teste do próprio broker), E4 (passaram um ensaio em real e uma ordem real do tamanho mínimo). Abaixo de E4, uma conta que opera com dinheiro real é recusada pelo próprio conector.

| Nível | Significa | Ordens reais |
|---|---|---|
| **E2** | testado contra uma plataforma simulada | recusadas |
| **E2D** | E2, construído só a partir da documentação da plataforma, nunca testado contra ela | numa conta cujo titular o aceitou, com os limites definidos; a verificação própria da plataforma corre antes da primeira ordem de cada sessão onde existe |
| **E3** | testado no ambiente de teste do broker, durante dias | recusadas |
| **E4** | um teste em seco real e uma ordem real do tamanho mínimo passaram | assim que o trading real estiver ativo, com os limites definidos |

**E2D (Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex, Deribit).** Ativar o trading real pede que marque a aceitação de que o circuito de ordens deste broker foi construído a partir da documentação da plataforma e nunca foi testado contra ela; fica guardada com a data e mostrada na conta. Exigem-se os mesmos limites que em E4. Uma resposta que o broker não entende, uma ordem cujo resultado fica desconhecido ou uma posição que se move sem execuções são tratadas como em qualquer broker: registadas, notificadas e reconciliadas com o broker.

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

## Trade no alerta {#trade-on-alert}

Um alerta do gráfico pode enviar uma ordem quando dispara, sem ninguém a olhar, mesmo com a aplicação fechada. Configure-o na secção **Operação** do alerta (*Enviar uma ordem para uma conta de broker quando este alerta disparar*); o alerta em si está descrito em [Charts → Alertas](/pt/modules/market-data#alerts).

- **A conta** tem de ter o trading ligado, estar concedida ao Charts e ter o [trading em segundo plano](/pt/config/brokers#background-trading), assinado uma vez em **Definições → Brokers**. Uma conta real também precisa do [trading real](#live-trading) ligado com os seus limites definidos.
- **Quando dispara**: **Entrar** envia o ticket que descreve (lado, tipo, tamanho em **Unidades**, **Montante** ou **Risco**, preços como distância ao preço que dispara, take profit e stop loss); **Sair** cancela as ordens ativas no instrumento e fecha a sua posição a mercado.
- **Se for recusada**: não fazer mais nada, cancelar as ordens no instrumento, também fechá-lo, ou cancelar tudo e fechar todas as posições. Quando a resposta do broker se perde, só as ordens no instrumento são canceladas.
- **Pré-visualizar** mostra a ordem ao preço do gráfico, a simulação do broker onde ele tem uma, e o que mais está armado no instrumento. **Armar** vem depois de uma pré-visualização e pede o código do autenticador quando o [bloqueio de trading](#trading-lock) está ligado e a conta não é paper.
- **Dispara uma vez**, depois espera ser armado de novo. Cada ordem passa por todas as verificações de uma ordem do gráfico ([limites de risco](#risk-limits), suspensões, o registo de auditoria, com o alerta como origem), e o resultado é notificado nos canais do alerta.
- **Desarmar.** Desligar o trading em segundo plano ou o trading na conta, remover o segundo fator, ligar o bloqueio de trading, ou o corta-circuito desarma o que estava armado; arme de novo conscientemente.

## Limites

- Na Interactive Brokers, as ordens vão para a conta por omissão do Gateway.
- Não se muda o lado nem o tipo de uma ordem ativa: cancele e envie uma nova.
- As ordens são na moeda do contrato, sem conversão.
- Os agentes de IA ([MCP](/pt/config/ai-agents)) e os workflows do Automator não podem enviar ordens, nem armar um trade no alerta. Um [bot de trading](/pt/modules/robots) ou um [trade no alerta](#trade-on-alert) só envia ordens depois de o armar, numa conta com o [trading em segundo plano](/pt/config/brokers#background-trading) assinado.
- As contas de broker, e portanto o trading, estão desativadas no [modo demo](/pt/guide/demo).
