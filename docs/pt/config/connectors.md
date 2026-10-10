# Data connectors

Todo módulo que lê dados de mercado se serve de **uma lista compartilhada de connectors**. Uma conta de provedor é criada uma vez e concedida aos módulos que podem usá-la.

Gerencie-os em **Configurações → Data connectors**, na página independente **/connectors**, ou pelo botão de connector que todo módulo de dados coloca ao lado do seu seletor de provedor. Os três mostram a mesma tela.

## O que é um connector

Um **connector é uma instância nomeada de um provedor**, não o provedor em si. Quatro coisas pertencem a ele:

- **o provedor**: Binance, Yahoo Finance, EODHD…;
- **suas credenciais**: digitadas, ou conectadas do [Cofre](/pt/config/settings#vault). Somente escrita: o app só sabe *quais* nomes de segredo estão definidos;
- **um limite de requisições opcional**: um número máximo de chamadas por período;
- **os módulos autorizados a usá-lo**: um ou mais, ou *todos os módulos* (um curinga que também cobre módulos de dados adicionados em versões futuras).

Vários connectors do mesmo provedor podem coexistir. Esse é o ponto: uma chave somente leitura para gráficos e uma chave separada para downloads em massa, cada uma com seu limite, cada uma concedida a um módulo diferente.

## Provedores

| Provedor | Credenciais | Tipos de ativo | Busca de símbolos | Stream ao vivo |
|---|---|---|---|---|
| **Binance** | nenhuma | cripto | sim | sim |
| **Binance USDⓈ-M Futures** | nenhuma | cripto | sim | sim |
| **Bitget** | nenhuma | cripto | sim | sim |
| **OKX** | nenhuma | cripto | sim | sim |
| **Kraken** | nenhuma | cripto | sim | sim |
| **Coinbase** | nenhuma | cripto | sim | sim |
| **OANDA** | `api_token` (+ id da conta) | FX e CFDs | sim | não |
| **Yahoo Finance** | nenhuma | ações, ETF, índice, cripto | sim | não |
| **Alpha Vantage** | `api_key` | ações, ETF, cripto, FX | sim | não |
| **EODHD** | `api_key` | ações, ETF, FX, cripto | sim | não |
| **Alpaca** | `api_key`, `api_secret` | ações, cripto, opção | sim | intradiário |
| **Massive (Polygon.io)** | `api_key` | ações, ETF, opção, futuro, cripto, FX, índice | sim | intradiário, plano pago |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` | ações, ETF, opção, futuro, índice | por símbolo | não |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | FX e CFDs | sim | não |
| **Capital.com** | `api_key`, `identifier`, `api_password` | FX, índices, ações, cripto (todos CFDs) | sim | sim |
| **Interactive Brokers** | nenhuma (host + porta) | ações, ETF, cripto, FX, índice, futuro, opção | sim | intradiário |

Os sem chave funcionam no momento em que você cria o connector. Cada linha de provedor aponta para a própria documentação de API e traz uma nota sobre seus limites de taxa, e um connector que transmite também traz uma nota sobre o que custa o ao vivo ali.

### Streaming ao vivo

O alcance do ao vivo é mais estreito que o do download, e de propósito.

- As exchanges de cripto publicam um canal de candles por intervalo, então **todo** timeframe que elas baixam elas também transmitem, o diário incluído: num mercado 24/7 o candle diário *é* o dia epoch. Bitget e OKX ancoram seus próprios candles diários e semanais à meia-noite de Hong Kong, então tanto o download quanto o feed ao vivo pedem as variantes alinhadas ao UTC, e uma série baixada ali se alinha com uma baixada em qualquer outro lugar.
- **Três provedores não transmitem aqui.** OANDA e TradeStation publicam preços ao vivo numa resposta HTTP de longa duração, e a FOREX.com por Lightstreamer; nenhum dos três é o WebSocket que os gráficos ao vivo falam. Seus downloads de histórico e o lado da conta funcionam; o candle ao vivo não.
- Alpaca, Massive e Interactive Brokers publicam uma granularidade cada (barras de um minuto, agregados de um minuto e barras de cinco segundos, respectivamente) e o timeframe do gráfico é dobrado a partir dela. Isso torna disponível todo timeframe **intradiário** e deixa **diário e semanal para o download**: uma sessão de ações não tem 1440 minutos alinhados ao epoch, então um candle diário construído assim discordaria do que o download armazena. O gráfico diz isso em vez de esconder o controle.
- O ao vivo costuma ser vendido separadamente do histórico. Uma chave Massive gratuita baixa histórico e é recusada no login ao vivo; a chave gratuita da Alpaca transmite IEX e o feed indicativo de opções, mas não SIP nem OPRA; a Interactive Brokers serve o que sua conta assina. Quando um feed não consegue rodar, o gráfico nomeia qual desses é e para, em vez de reconectar atrás de um ponto que nunca fica verde.
- A maioria desses fornecedores permite **uma conexão ao vivo por conta**, então um segundo programa na mesma chave toma a vaga. Esse caso é informado como tal e continua tentando, já que se resolve quando você fecha o outro.

A **Alpaca** tem uma configuração para isso: *Market data feed*, `iex` (plano gratuito, o padrão) ou `sip` (pago). Ela seleciona apenas o socket ao vivo; os downloads não são afetados.

### Os provedores de derivativos de cripto

`BTCUSDT` é um par spot **e** um perpétuo, e os dois são séries diferentes: o perpétuo negocia com uma base em relação ao spot e um contrato datado converge para ele. Então qual mercado um conjunto de dados contém nunca é inferido do ticker.

- **Binance USDⓈ-M Futures** é seu próprio provedor ao lado da Binance, não uma configuração dela. Os contratos são escritos como o mercado de futuros os escreve: `BTCUSDT` para um perpétuo, `ETHUSDT_250926` para um datado. Contratos Coin-M (inversos) não são servidos.
- A **Bitget** tem uma configuração *Market*, `spot` (o padrão) ou `usdt-futures`, porque ela escreve o mesmo ticker de forma idêntica nos dois books.
- A **OKX** não precisa de configuração: seus próprios ids de instrumento dizem em que mercado um ticker está, `BTC-USDT` para spot, `BTC-USDT-SWAP` para um perpétuo, `BTC-USD-241227` para um contrato datado.

### OANDA

O único provedor de FX com chave aqui, e sua chave é a da conta: a OANDA não emite token somente de dados de mercado, então o connector pede o mesmo token de acesso pessoal que a [conta de broker](/pt/config/brokers) usa, mais o número da conta pela qual ele lê os preços.

- **Configurações**: *Account ID* (`001-004-1234567-001`) e *Environment* (`live` ou `practice`, que são hosts diferentes com tokens diferentes).
- **Os instrumentos** são escritos `base_quote`, índices e commodities incluídos: `EUR_USD`, `XAU_USD`, `SPX500_USD`. *Testar conexão* informa quantos a conta tem permissão para precificar.
- **Os candles diários são fixados na meia-noite UTC.** O padrão da própria OANDA vira o dia às 17:00 de Nova York, que é a sessão de FX mas não o dia em que todos os outros conjuntos de dados aqui são armazenados, então o connector pede o UTC.
- O volume é uma **contagem de ticks**, não um tamanho negociado: uma mesa de negociação publica quantos preços fez, não quanto mudou de mãos.

Quando um provedor tem vários connectors, o controle ao vivo do gráfico ganha um seletor de conta: duas chaves são dois direitos e duas vagas de conexão, então qual é gasta é sua escolha, não um fallback.

### TradeStation

Seu escopo de dados de mercado viaja na própria chave OAuth da conta, então o connector pede o mesmo par de chaves de API e refresh token que a [conta de broker](/pt/config/brokers) usa. Não há credencial de dados de mercado separada.

- **Configurações**: *Environment* (`live` ou `sim`).
- **Os símbolos** são os da própria TradeStation: `AAPL` para uma ação, `@ES` para o futuro contínuo, `ESH26` para um contrato, `$SPX.X` para um índice à vista, `MSFT 260116C400` para uma opção. Digitar um o consulta e mostra o que é, que é a forma mais rápida de pegar um erro de digitação.
- **As barras são carimbadas no fechamento**, então o connector subtrai o intervalo e armazena a abertura, como toda outra série aqui.
- **Só 1m, 5m, 15m e 1d são oferecidos.** A TradeStation monta barras intradiárias a partir da abertura da *sessão*, então seu candle horário começa às 9:30 e não se alinharia com o candle horário de nenhum outro lugar da sua biblioteca. Baixe 15m e leia em qualquer timeframe intradiário; a recusa diz isso.

### FOREX.com (StoneX)

Mesma história: sem credencial de dados de mercado própria, então ela entra com o mesmo usuário, senha e AppKey da [conta de broker](/pt/config/brokers), e as duas compartilham uma sessão.

- **Um mercado é um número.** A API recebe um id numérico de mercado; você digita `EUR/USD` e o connector o resolve. Quando um nome corresponde a vários mercados, o erro os lista com seus ids, e você baixa pelo id.
- **Sem volume.** Uma mesa de negociação publica preços, não tamanho, então a coluna de volume é zero em vez de um número plausível.
- **Uma barra diária é a sessão da venue**, que vira no fechamento de Nova York, não na meia-noite UTC. Esse é o período em que a StoneX realmente negociou e é armazenado assim, então uma série diária daqui não fica sobre uma de um provedor de dia UTC.
- `4h` é recusado: a StoneX não diz onde começa a contar um. Baixe `1h` e leia em 4h.

### Capital.com

A terceira com chave que é a da conta: a Capital.com não emite credencial de dados de mercado, então o connector entra com a mesma chave de API, login e senha personalizada da [conta de broker](/pt/config/brokers), e as duas compartilham uma sessão.

- **Configurações**: *Environment* (`live` ou `demo`).
- **Um instrumento é um epic**, o nome de mercado da própria Capital.com: `EURUSD`, `US500`, `AAPL`, `BTCUSD`. A busca de símbolos os retorna.
- **Um candle é o ponto médio dos dois lados** em que a mesa opera, no download e no gráfico ao vivo igualmente.
- **Uma barra diária é a sessão da venue**, não o dia UTC, então uma série diária daqui não fica sobre uma de um provedor de dia UTC.
- O **ao vivo** usa a mesma sessão e permite 40 instrumentos de uma vez. A Capital.com transmite o bid e o ask como dois candles separados, então um painel só se preenche depois que os dois lados tiquearam.

### Interactive Brokers

O diferente do grupo: não há URL de fornecedor nem chave de API. Você roda o **IB Gateway** ou o **TWS** na sua própria máquina e o connector fala o protocolo de socket dele, então o que ele carrega é um **endereço**, não uma credencial: um host e uma porta, armazenados em texto claro para que uma conexão falha possa ser diagnosticada. Os dados são o que sua conta IB assina.

- **Configurações**: *Gateway host* (`host.docker.internal` para um gateway na mesma máquina, já que o OpenTraderWorld roda em um contêiner) e *API port* (4001 live / 4002 paper para o Gateway, 7496 / 7497 para o TWS).
- **No gateway**: Global Configuration → API → Settings, marque *Enable ActiveX and Socket Clients*, e confira se a porta corresponde. No Docker Desktop a chamada chega do loopback do host, então *Allow connections from localhost only* já a cobre; no Docker Engine desmarque e adicione `172.28.53.10` a *Trusted IPs*, que aceita endereços individuais e não uma faixa.
- **Testar conexão** informa o que respondeu, e nomeia a configuração a mudar quando nada responde.
- **Tickers**: `AAPL`, `SAN:EUR` ou `7203@TSEJ:JPY` para ações, `EURUSD` para um par à vista, um símbolo OCC para uma opção. Um futuro é escrito com seu mês, `ES.202512`, ou com o símbolo local que o TWS mostra, `MNQU6`. Os futuros são consultados no gateway antes de qualquer coisa ser baixada, então a exchange é opcional: quando o ticker nomeia mais de uma listagem, o erro as lista e você escolhe.
- O id de cliente é escolhido pelo app em uma faixa privada alta, nunca pedido, então nada mais que você tenha conectado ao gateway é expulso.
- A Interactive Brokers permite 60 requisições históricas a cada 10 minutos móveis **por conta**: o connector se cadencia sozinho, então um backfill longo é lento por projeto.

Testado com o **IB Gateway build 10.50.1e (25 ago 2026)**. Espera-se que builds mais antigos funcionem, já que o protocolo de socket é negociado para baixo, mas essa é a versão em que este connector foi verificado.

## Crie um

1. **Adicionar connector**, escolha o provedor e dê um nome: o nome é o que os seletores dos módulos mostram, então *Binance gráficos* é melhor que *Binance 2*.
2. Preencha as credenciais que o provedor exige, ou escolha-as do Cofre. Provedores sem chave pulam isto.
3. Escolha os **módulos** que ele atende. Aberto a partir de um módulo, o novo connector é concedido apenas a esse módulo; aberto a partir das Configurações ou de `/connectors`, é concedido a tudo.
4. Opcionalmente defina um **limite de requisições** (veja abaixo).

Um connector sem uma credencial obrigatória aparece como *precisa de credenciais* e é ignorado por todos os módulos até você defini-la.

## Concessões de módulos

A lista de concessões é **do lado do servidor**: um módulo que pede um connector que nunca recebeu é recusado, então uma caixa que só vivesse no navegador seria decoração. Marcar todos os módulos de dados volta ao curinga *todos os módulos*, que mantém cobertos os futuros módulos de dados.

Os módulos concedíveis hoje:

| Módulo | O que ele lê |
|---|---|
| **Historical Data** | a lista de provedores do formulário de download, e a busca de símbolos |
| **Visualization** | a busca de símbolos do gráfico, suas janelas sob demanda e seu stream ao vivo |
| **Watchlists** | a fonte de cotações de uma lista, ou de um único símbolo |
| **Journal** | os candles por trás das abas Dados de mercado e Risco aberto |
| **Quant Tools** | as abas Derivativos: contratos de futuros, cadeias de opções e volatilidade implícita, da Interactive Brokers ou da Massive |

## Limites de requisições

Um limite é uma contagem de chamadas de saída por **dia**, **hora** ou **minuto**, acompanhada por connector.

- Em todo o resto do app ele é **apenas de observação**: alimenta os contadores em [Configurações → Taxa da API](/pt/config/settings#api-rate) e avisa, mas nada é limitado.
- Nas buscas sob demanda do gráfico (`/api/histviz/series`) ele **bloqueia**: quando o connector chega ao limite, a janela volta com as barras já armazenadas e um aviso de *limite de requisições atingido*, em vez de queimar silenciosamente um plano com cobrança por uso.

Deixe o limite desligado se preferir que o provedor seja quem diz não.

## Onde os connectors são usados

- **Historical Data**: a lista de provedores do formulário de download, e a busca de símbolos.
- **Visualization**: a aba Dados busca em todos os connectors concedidos ao gráfico de uma vez; o stream ao vivo roda no connector que você escolhe no controle ao vivo, ou no mais antigo que o gráfico recebeu para esse provedor.
- **Watchlists**: a fonte de cotações de uma lista, ou de um único símbolo. CoinGecko e Yahoo continuam disponíveis sem nenhum connector.
- **Trading Journal**: os candles por trás das abas Dados de mercado e Risco aberto, com uma fonte escolhível por tipo de ativo.
- **Quant Tools**: as abas Derivativos listam os contratos de futuros de um produto ou a cadeia de opções de um subjacente e precificam cada um. Apenas a Interactive Brokers e a Massive os listam; o histórico de volatilidade implícita vem apenas da Interactive Brokers.

::: tip Atualizando das antigas configurações por módulo
A aba *Configurações* do Historical Data e a aba *Fontes* das Watchlists não existem mais: eram duas cópias desconectadas desta tela sobre duas listas desconectadas. As contas criadas em qualquer uma delas agora são connectors aqui, cada um ainda concedido ao módulo de onde veio, então nada muda de alcance na atualização. Os nomes voltam a ser globalmente únicos: um nome que existia nas duas listas é mantido uma vez e o outro renomeado para `<name> #2`.
:::
