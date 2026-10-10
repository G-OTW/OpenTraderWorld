# Contas de broker

Uma **conta de broker** é uma chave somente leitura para o lugar onde você realmente opera. Ela responde três perguntas que ninguém deveria redigitar à mão: o que foi executado, o que estou segurando, o que tenho em andamento.

Gerencie-as em **Configurações → Brokers**, ou pelo botão *Brokers* que todo módulo que lê o seu book coloca ao lado do seletor de conta. Os dois mostram a mesma tela.

::: warning Somente leitura, por construção
As contas de broker leem, e só leem: nenhuma rota por trás delas envia, altera ou cancela uma ordem. As credenciais pedidas são do tipo somente leitura, então forneça exatamente isso: onde um broker pode emitir uma chave apenas de visualização, o formulário diz. Se um dia o roteamento de ordens for lançado, será um recurso à parte, pedindo suas próprias chaves e sua própria permissão, e isso será dito aqui.
:::

Não confunda com os [data connectors](/pt/config/connectors): aqueles leem **preços**, estas leem **a sua conta**. Duas credenciais diferentes, duas listas diferentes, duas concessões diferentes, de propósito.

## O que você precisa para conectar

| Broker | Credenciais | Permissão a conceder | Lê |
|---|---|---|---|
| **Alpaca** | `api_key`, `api_secret` | Chave da Trading API do ambiente escolhido (live *ou* paper, são chaves separadas) | execuções, posições, ordens, holdings |
| **Binance** | `api_key`, `api_secret` | Somente *Enable Reading*. Sem trading, sem saques | execuções, ordens, holdings |
| **Binance USDⓈ-M Futures** | `api_key`, `api_secret` | *Enable Reading* mais acesso a futuros. Sem trading, sem saques | execuções, posições, ordens, saldos de margem |
| **Bitget** | `api_key`, `api_secret`, `api_passphrase` | *Read-only*. Sem trade, sem saque | execuções, posições, ordens, holdings |
| **OKX** | `api_key`, `api_secret`, `api_passphrase` | Somente *Read*. Sem trade, sem saque | execuções, posições, ordens, holdings |
| **OANDA** | `api_token` + o id da conta | Um token de acesso pessoal. **A OANDA não tem token somente leitura**: o mesmo pode operar | execuções, posições, ordens, holdings |
| **Coinbase Advanced Trade** | `api_private_key` + o nome completo da chave | Chave CDP, somente **View**, criada como **Ed25519**. Sem Trade, sem Transfer | execuções, ordens, holdings |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` + o id da conta | Login OAuth concedendo `ReadAccount`, `MarketData`, `openid`, `offline_access`. **Não `Trade`** | execuções, posições, ordens, holdings |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | Seu login mais a AppKey emitida pela StoneX. **Não existe credencial somente leitura** | execuções, posições, ordens, holdings |
| **Capital.com** | `api_key`, `identifier`, `api_password` | Uma chave de API (a autenticação em dois fatores deve estar ligada) e sua senha personalizada. **Sem nível somente leitura** | execuções, posições, ordens, holdings |
| **NinjaTrader** | `username`, `password`, `cid`, `sec` | Login da plataforma mais o par de chaves da API de desenvolvedor. **Sem chave somente leitura** | execuções, posições, ordens, holdings |
| **Kraken** | `api_key`, `api_secret` | *Query Ledger & Trade History* e *Query Open Orders* | execuções, ordens, holdings |
| **Interactive Brokers (Flex)** | `flex_token` + um id de query Flex | Token do Flex Web Service: ele lê extratos, não consegue operar | execuções, posições, holdings |

Os segredos são somente escrita: o app só sabe *quais* nomes estão definidos. Eles podem ser digitados, ou conectados do [Cofre](/pt/config/settings#vault) para que uma chave sirva a várias contas.

### Notas por broker

- **Alpaca**: a configuração *Environment* é `live` ou `paper`, e tem de ser dita em vez de adivinhada, já que os dois vivem em hosts diferentes com chaves diferentes. A Alpaca não informa comissão em uma execução, então os trades importados não trazem taxas: correto para ações sem comissão, aquém da verdade para cripto e opções, cujas taxas chegam como atividades de conta separadas.
- **Binance**, spot e futuros, responde seu histórico de trades **um instrumento por vez**, então uma consulta precisa nomear os instrumentos (`BTCUSDT`, `ETHEUR`). Todos os outros brokers aqui respondem a conta inteira.
- **Três venues de cripto param em 90 dias.** Bitget, OKX e Binance futures servem três meses de execuções pela API e nada além; qualquer coisa mais antiga é um download do site deles. O modal de importação mostra o limite e recusa um período que comece antes dele, em vez de devolver uma meia resposta silenciosa.
- **Uma conta de derivativos não é uma conta spot.** Bitget, OKX e Binance futures podem ficar short, então *Esta conta pode ficar short* vem marcada por padrão ali. Em uma conta Bitget configurada apenas para o book spot, desmarque: uma venda spot sem nada aberto é a venda de uma moeda comprada antes, não um short.
- **Um contrato é contado em contratos.** A OKX informa execuções de derivativos em contratos e publica quanto vale um (`ctVal × ctMult`), o que é lido da lista de instrumentos e carregado como o valor do ponto do trade. Os contratos Bitget USDT-M e Binance USDⓈ-M são dimensionados na moeda base, então o deles é um. Contratos Coin-M (inversos) não são lidos em lugar nenhum: são dimensionados na moeda de cotação e o PnL deles não é uma quantidade vezes um preço.
- **A TradeStation é a única com OAuth.** Não há chave estática: um login único no navegador produz um refresh token, que o app troca por um token de acesso de 20 minutos conforme avança. Conceda `ReadAccount`, `MarketData`, `openid` e `offline_access` nesse login e deixe `Trade` de fora; um token que pudesse operar seria um risco permanente por nada. Uma execução aqui é uma *perna de ordem*, já que a TradeStation publica ordens fechadas e não execuções, e a identidade pela qual uma nova sincronização deduplica é o id da ordem mais a posição da perna nela.
- **A FOREX.com faz login, não usa chave.** As credenciais são o usuário e a senha da própria conta mais a AppKey que a StoneX emite depois de assinados os termos da API, então o mesmo login pode operar: trate-o como um segredo de acesso total e troque a senha ao remover a conta. Um trade importado dali traz **nenhuma comissão**, porque a StoneX cobra pelo spread; se a sua conta cobra comissão, os números aqui ficarão aquém da verdade. O endpoint de histórico de trades não aceita data final nem cursor, então um período amplo é percorrido para a frente em páginas de 200.
- **A Capital.com responde um dia por vez.** Seu log de atividade limita o intervalo entre duas datas a 24 horas, então um ano de trading custa uma chamada por dia; períodos maiores que 400 dias são recusados pelo nome em vez de deixados correr até o limitador de taxa. Todo instrumento da plataforma é um **CFD**, então um CFD de ação é registrado como derivativo e não como a ação, que é o que um formulário de imposto quer. Um deal id nomeia uma posição e não uma execução, então a identidade pela qual uma nova sincronização deduplica é o deal, seu timestamp e sua direção juntos.
- **A NinjaTrader permite duas sessões por login**, e uma terceira fecha a mais antiga: este connector segura uma, e um aplicativo de trading conectado ao lado segura a outra. Sua API de trading é a plataforma Tradovate que ela adquiriu, e por isso os erros dizem `tradovateapi`. Ela responde as execuções que sua sessão enxerga e não publica limite de profundidade, então confira a linha mais antiga que a primeira consulta retornar antes de depender dela para um ano fiscal. O valor do ponto de um futuro é lido do produto do contrato, nunca presumido a partir da raiz.
- **A OANDA não dá token somente leitura.** O token de acesso pessoal que lê esta conta também pode operá-la, e por isso o formulário avisa: trate-o como uma credencial de acesso total e revogue-o ao remover a conta. Nada no app jamais o usa para escrever.
- **A Interactive Brokers** não é uma chave de API: ela lê um relatório salvo pelo Flex Web Service. A configuração, a regra do período e a configuração de fuso horário têm [sua própria seção abaixo](#interactive-brokers-o-flex-web-service).
- Os limites de taxa são os do broker, e cada linha mostra o que se aplica. A IBKR é a mais estrita: ela monta o extrato sob demanda e recusa uma segunda requisição enquanto uma ainda está sendo gerada.

## Conecte uma

1. **Adicionar conta**, escolha o broker e dê um nome: o nome é o que os seletores dos módulos mostram, então *Kraken principal* é melhor que *Kraken 2*.
2. Preencha as credenciais, ou escolha-as do Cofre. Uma conta sem alguma delas aparece como *incompleta* e é ignorada por todos os módulos até você defini-la.
3. Preencha as configurações não secretas de que o broker precisa: um ambiente (Alpaca, Binance futures, TradeStation, Capital.com, NinjaTrader), um id de conta (OANDA, TradeStation, Capital.com, FOREX.com, NinjaTrader), o nome da chave Coinbase, o id da query e o offset da IBKR, ou os books da Bitget.
4. Escolha os **módulos** que ela atende, ou *todos os módulos*.
5. **Testar conexão** alcança o broker e informa o que respondeu: o número da conta e o status, quantos ativos têm saldo, ou qual extrato o token Flex retornou. Se algo estiver errado, o erro nomeia a configuração a alterar.

As concessões são aplicadas no servidor: um módulo que pede uma conta que nunca recebeu é recusado.

## O que ela permite fazer

Quatro destinos, um só formato. Seja o que você importar e onde chegar, uma importação de broker roda nos mesmos dois trilhos de uma importação de arquivo:

1. **Puxar, depois olhar.** O app pergunta ao broker, dobra a resposta no que o módulo armazena (posições para o journal, um balanço para a carteira, disposições fechadas para o formulário de imposto) e mostra a você. Nada é gravado nesta etapa, então uma consulta de que você não gostou não custa nada.
2. **Confirmar, e manter o fio.** Tudo o que é gravado carrega o id dessa importação, então continua sendo um só objeto depois: *Reverter* apaga exatamente o que criou e nada mais, *Manter, deixar de rastrear* corta o vínculo e deixa as linhas no lugar. Os dois vivem no histórico de importações do módulo.

Duplicatas são trabalho do trilho, não seu. Cada linha importada recebe uma impressão digital, então puxar de novo um período sobreposto reconhece o que já está arquivado, marca na prévia e grava uma só vez.

### Importar trades para o journal

**Journal → Importar → Puxar de um broker**. Escolha a conta, o período e o book onde arquivar, e as execuções voltam dobradas em posições, com prévia antes de qualquer gravação.

1. **A conta.** Só aparecem as concedidas ao journal, e uma incompleta avisa. A última conta e o último período usados para um book são lembrados, então a próxima consulta leva dois cliques.
2. **O período**, por data ou com os chips *7 / 30 / 90 / 365 dias*. Leia-o como a janela em que as *execuções* caem, não a janela em que os trades fecharam: uma posição é dobrada a partir das execuções dentro do período, então comece cedo o bastante para pegar a entrada.
3. **Os instrumentos.** A Binance responde seu histórico um instrumento por vez, então os símbolos são obrigatórios ali e *Sugerir* oferece o que a conta tem. Em todos os outros lugares o campo é um filtro: deixe vazio para a conta inteira.
4. **Shorts.** Uma conta spot não pode ficar short, então uma venda sem nada aberto é informada como erro de linha nomeando a correção (ampliar o período) em vez de virar um short fantasma. Em uma conta com margem, marque *Esta conta pode ficar short*.
5. **Prévia**, depois importar. Os contadores são execuções, trades, dos quais fechados e abertos, mais o que já está no book e o que não pôde ser montado. Cada linha diz qual é antes de você confirmar.

- Mesmo destino e mesma dobra de uma importação de arquivo, menos o mapeamento: uma API responde campos tipados, então as perguntas que um CSV levanta (qual coluna é a data, o decimal é vírgula) não existem aqui.
- **Rodar de novo é seguro.** A identidade de uma posição é sua execução de *abertura*, então ampliar a janela e puxar de novo atualiza o que fechou desde então e deixa o resto em paz, em vez de registrar o mesmo trade duas vezes.
- **Uma atualização é apenas mecânica.** Preços, quantidades, taxas e datas vêm do broker de novo; suas notas, tags, estratégia e campos de modelo são seus e sobrevivem.

### Alinhar uma carteira com o que a conta tem

**Portfolio → De um broker**. Ele lê um **balanço**, não um histórico de trades: a diferença em relação ao seu ledger é mostrada linha a linha, e você escolhe quais linhas alinhar. Cada uma que você aceita grava a única operação que faz a carteira concordar.

- Um símbolo que a carteira já tem se resolve sozinho; qualquer outro é perguntado, porque "BTC" em uma exchange é uma string e um ativo aqui é uma fonte de preço.
- **O custo base nunca é inventado.** A Interactive Brokers publica um e ele é usado. As exchanges de cripto publicam uma quantidade e nada mais, então essas linhas dizem isso e assumem o preço de hoje, o único preço que ninguém confunde com uma afirmação sobre o passado.
- **Pegue o preço da exchange.** Em uma linha sem custo base, um clique pergunta à venue por quanto o ativo negocia agora e preenche o preço. A linha então nomeia o mercado que respondeu (`BTCUSDT`, `XBT/USD`) e avisa quando esse mercado cota em algo diferente da moeda do próprio ativo: um preço da Binance é em USDT, não em dólares. Um ativo que a venue não cota é deixado em paz em vez de avaliado de outro lugar, e você digita o preço.

Alpaca, Binance (spot e futuros), Bitget, Coinbase, Kraken, OKX, OANDA e TradeStation respondem a essa pergunta. A Interactive Brokers Flex não: um extrato não é um feed de cotações, a FOREX.com cota um mercado pelo id numérico e não pelo nome que uma linha da carteira carrega, e a NinjaTrader serve preços por um entitlement de dados de mercado à parte. A Capital.com responde com o meio dos dois lados em que opera.

### Desenhe o seu book no gráfico

**Gráfico → Book do broker**. Sincronize uma conta e suas posições e ordens em andamento são desenhadas como níveis de preço no gráfico do instrumento correspondente, custo médio para uma posição, limit e stop para uma ordem. A correspondência é pelo ticker, ignorando pontuação. Uma posição sem custo médio não recebe linha e é contada como tal.

### Ler um ano fiscal

**Impostos → De um broker**. Puxa as execuções, dobra-as em posições fechadas e totaliza o que foi realizado dentro do ano fiscal, dividido entre as linhas de capital, derivativos e cripto do formulário. **Não grava nada**: você aplica os números ao formulário e salva o cenário você mesmo.

- **A janela não é o ano fiscal.** O que você vendeu em março foi comprado antes, e sem essa compra não há custo base: recue a data inicial o bastante para cobri-la. Uma disposição cuja compra está faltando é informada, nunca avaliada contra nada.
- Cada posição fechada é convertida à taxa **da sua própria data de saída**. Uma que não tem taxa é listada e deixada fora dos totais em vez de somada na moeda errada.

## Interactive Brokers: o Flex Web Service

A IBKR é o único broker aqui que não é lido por uma API de trading. Ele é lido pelo **Flex**, o serviço de relatórios do Account Management: você salva uma query descrevendo o que quer em um extrato, e o app busca esse extrato por HTTPS com um token.

### Por que Flex e não TWS

O [connector de dados de mercado](/pt/config/connectors#interactive-brokers) fala o socket TWS com um Gateway que você mesmo roda. Esse socket é a ferramenta certa para o presente, e a errada para o histórico: ele responde posições abertas e as execuções da **sessão atual**, então *importar meus trades de março* não tem forma de socket nenhuma. O Flex serve um período, que é exatamente a pergunta que uma importação faz.

A diferença prática:

| | Flex Web Service | Socket TWS / IB Gateway |
|---|---|---|
| **O que você roda** | nada, é uma chamada HTTPS | Gateway ou TWS, conectado, na máquina |
| **Histórico** | o período da query, até um ano atrás | apenas a sessão atual |
| **Credencial** | um token que lê relatórios | sua sessão ativa, capaz de operar |
| **Usado aqui para** | importação do journal, holdings da carteira, ano fiscal, posições no gráfico | preços, gráficos, barras ao vivo |

### Por que é a entrada segura

- **O token não consegue operar.** Ele é emitido para o Flex Web Service e esse serviço serve extratos. Não há endpoint de ordens por trás dele para esquecer de desativar, nem caixa de permissão para errar. Compare com a chave de API de uma exchange, onde somente leitura é uma caixa que você precisa lembrar de marcar.
- **Nada fica escutando.** Sem Gateway rodando, sem porta de API aberta, sem Trusted IP a declarar, nada esperando na sua máquina enquanto o app está ocioso.
- **Ele expira sozinho.** A IBKR dá um prazo de vida ao token e manda um lembrete por e-mail antes de ele vencer. Um token esquecido deixa de funcionar em vez de continuar válido para sempre.
- **A query é a cerca.** Um token só consegue retornar o que as queries que você salvou descrevem. Mantenha a query em trades e posições abertas e é só isso que o app poderá ver, seja o que for que ele peça.
- Como toda credencial aqui, o token é **somente escrita no app**: pode ser digitado ou conectado do [Cofre](/pt/config/settings#vault), e nunca é mostrado de novo.

### Como uma consulta realmente roda

1. O app chama `SendRequest` com seu token e o id da query. A IBKR responde com um código de referência e começa a **montar** o extrato.
2. Depois consulta `GetStatement` com esse código até o XML chegar, o que normalmente leva alguns segundos e pode demorar mais para uma query ampla. Um extrato ainda sendo gerado é a resposta esperada das primeiras tentativas, não um erro.
3. O extrato é interpretado em execuções (linhas `Trade` em nível de execução) e holdings (`Open Positions`), e o app filtra isso pelo período que você escolheu.

Se a IBKR ainda estiver gerando após um minuto, o app diz isso em vez de travar: reduza o intervalo de datas da query, ou tente de novo em instantes.

### Configure

1. **O token**: Account Management → Settings → **Flex Web Service**. Gere um, copie-o uma vez, anote a data de expiração.
2. **A query**: Account Management → Performance & Reports → **Flex Queries** → nova query de *Activity*. Inclua:
   - **Trades**, nível de detalhe **Execution**, para o journal e o formulário de imposto;
   - **Open Positions**, para o alinhamento da carteira e o overlay do gráfico.

   Salve e anote o **id da query**, o número mostrado ao lado do nome dela.
3. No OpenTraderWorld: adicione a conta, cole o token, preencha **Flex query id** e **Statement time offset**, depois **Testar conexão**. Ele informa o número da conta, o período que o extrato cobre e quantas linhas de trade ele traz, que é o jeito mais rápido de ver que a query está sem uma seção.

::: tip Duas configurações que decidem se a importação está certa
**O período é o da query, não o seu.** Uma query Flex carrega seu próprio intervalo de datas (*Last 365 Calendar Days*, *Year to Date*, uma janela personalizada) e o web service não aceita datas. As datas que você escolhe no app **filtram** o que o extrato retornou, então uma query configurada para *Last 30 days* nunca trará março por mais que você recue, e a IBKR não serve mais que um ano. Configure a query ampla, filtre no app.

**Um extrato Flex nunca nomeia seu fuso horário.** Ele carimba o fuso da própria query e não diz qual, então defina *Statement time offset* para esse fuso em minutos (`-300` Nova York no inverno, `60` Paris) ou toda execução cai na hora errada, e trades intradiários caem no dia errado.
:::

### O que o Flex não faz

- **Sem ordens em andamento**, então o overlay do gráfico desenha as posições da IBKR pelo custo médio e nenhum nível de ordem.
- **Sem cotações.** Um extrato não é um feed de preços: o botão *pegar o preço da exchange* da carteira é oferecido pelos brokers de API, não aqui. A IBKR é a única das cinco que publica um **custo base**, que é o número que importa para um ledger.
- **Um extrato por vez.** A IBKR o monta sob demanda e recusa uma segunda requisição enquanto uma está sendo gerada, então consultas em sequência na mesma query esperam uma pela outra.

## Concessões de módulos

| Módulo | O que ele lê |
|---|---|
| **Trading Journal** | as execuções do período, para a importação |
| **Portfolios** | o que a conta tem |
| **Visualization** | posições e ordens em andamento, para o overlay do gráfico |
| **Tax Calculator** | as execuções de um ano fiscal |

Marcar todos os módulos volta ao curinga *todos os módulos*, que também cobre os módulos adicionados em versões futuras.

## Limites

- **Um ticker nunca é adivinhado.** Um símbolo que o app não consegue resolver é um erro que nomeia a correção, não uma correspondência aproximada.
- O que um broker não publica fica vazio em vez de plausível: sem custo base inventado, sem taxa inventada, sem lado inventado.
- Excluir uma conta remove suas credenciais. O que ela já importou permanece.
- As contas de broker são desativadas no [modo demo](/pt/guide/demo).
