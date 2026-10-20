# Carteiras e patrimônio

Módulos independentes para acompanhar o que você observa, o que você possui, quanto isso custa e como pode ser a conta de impostos.

## Watchlists {#watchlists}

Listas nomeadas de símbolos que você quer acompanhar: sem posições, sem ledger, só cotações.

- **Adicione símbolos** pesquisando (cripto via CoinGecko, ações/ETFs via Yahoo), comece de um **modelo curado** (Top 10 de Cripto, Magnificent 7, ETFs de Índices dos EUA, Semicondutores), ou **importe uma carteira do Portfolio Tracker**, onde reimportar reconcilia em vez de duplicar.
- Cada linha mostra o preço ao vivo em USD, **variações de 24h / 3d / 7d / 30d**, um **sparkline de 30 dias**, a exchange e uma **nota** livre por símbolo. Ordene por qualquer coluna, filtre por nome.
- **Auto-atualização** por lista, de a cada minuto a diária (padrão 15 min). A página estima a taxa de requisições e **avisa antes de um intervalo arriscar throttling de APIs gratuitas**. As cotações são armazenadas em cache no servidor, então reabrir a página é instantâneo e nunca atinge os provedores.

### Fontes de cotação personalizadas

As fontes públicas (CoinGecko / Yahoo) funcionam de imediato, sem configuração. Se você tem sua própria conta de dados de mercado, conecte um **[data connector](/pt/config/connectors)**, a mesma conta de provedor compartilhada que o [Historical Data](/pt/modules/market-data#histdata) e o gráfico usam, criada uma vez e concedida às Watchlists. Cada connector carrega suas próprias credenciais (digitadas, ou escolhidas do [Cofre](/pt/config/settings#vault)) e seu próprio limite de requisições.

- Uma lista pode **fixar um connector como sua fonte padrão**, e cada símbolo pode substituí-lo: *seguir a lista*, *auto*, ou um connector específico.
- Os **tickers do provedor** por símbolo (`BTCUSDT`, `AAPL.US`, …) são derivados automaticamente e continuam editáveis quando um provedor nomeia um símbolo de forma diferente.
- Uma cotação que falha aparece **na sua própria linha**, então um símbolo ruim não esconde o resto da lista.

::: warning Conheça os limites do seu plano
Uma lista apoiada por uma fonte personalizada libera **intervalos de atualização de 5s a 30s**. Eles são rápidos o bastante para queimar um plano de API rapidamente: chamadas em excesso falham e podem fazer sua chave ser bloqueada. Acompanhe os contadores em **Configurações → Taxa da API**.
:::

### Alertas de preço

Qualquer símbolo pode ter alertas, definidos pelo sino na sua linha. Cada um se lê como uma frase que você monta da esquerda para a direita, *avise-me quando BTC se mover ±5% a partir de agora*:

- **Um nível** (preço acima ou abaixo de um valor), ou **um movimento** medido em **%** ou em **$**, para cima, para baixo ou em qualquer direção.
- Um movimento é medido **a partir de agora**, ou numa **janela móvel** (1h, 4h, 12h, 1d, 3d, 7d, 30d).
- **Uma vez ou repetindo**, com um atraso de rearme (5m a 1d) para que uma única oscilação não dispare a cada atualização.
- **Destinos**: a caixa de entrada do app mais qualquer [canal de notificação](/pt/config/settings#notifications) que você escolher por alerta. As Watchlists só podem usar canais que lhes foram concedidos.

Os alertas são avaliados **no servidor dentro do loop de atualização**, então disparam com a página fechada e o navegador encerrado.

### Descrição da lista

Uma watchlist carrega uma descrição editável sob seu nome, para o que a lista realmente serve.

## Portfolio Tracker {#portfolios}

Valor ao vivo das suas posições reais, uma carteira por conta ou tema.

- **Adicione ativos** pesquisando (moedas cripto ou ações/ETFs) e registre **operações de compra/venda** (data, quantidade, preço, taxa, nota). P/L realizado e não realizado, custo médio e pesos são calculados a partir do ledger.
- **Moeda de negociação por ativo**: cada ativo declara a moeda em que suas operações são digitadas, então uma ação comprada em EUR não é registrada como se fosse USD. Os rótulos do formulário seguem essa moeda. As cotações à vista continuam em USD: a base de custo e o P/L realizado convertem **na data de cada operação** usando as taxas de câmbio do [Trading Journal](/pt/modules/journal), então uma compra de três anos atrás mantém sua taxa histórica. Um popover no formulário explica de onde vem cada preço.
- **Auto-atualização**: uma etapa de reconciliação única confere cada posição contra sua fonte de preço; corrija as que aparecerem *não resolvidas* (ou marque-as como manuais) e ative a **atualização diária automática**, depois da qual os preços são atualizados em segundo plano todo dia.
- Por carteira: valor, base de custo, P/L não realizado/realizado/total, melhor e pior ativo, **alocação** por ativo ou classe, e um **gráfico de valor ao longo do tempo** (dia/semana/mês/ano) que se preenche conforme as atualizações se acumulam.
- A **descrição** continua editável após a criação, ao lado de uma nota dobrável de **tese de investimento** mantida com a carteira, sobre por que você tem o que tem.

### Caixa, renda e custos

O ledger não é só compras e vendas. **Caixa e renda** na aba de operações registra um
**depósito**, uma **retirada**, um **dividendo**, **juros**, um **cupom**, uma **taxa** ou um
**imposto**, cada um na sua própria moeda e com uma taxa retida opcional. A renda pode nomear a
posição que a pagou, ou nada, quando veio da própria conta.

A partir dessas linhas a carteira ganha um saldo de caixa por moeda, uma renda total e um
patrimônio líquido real (posições mais caixa). O caixa negativo é mostrado, nunca cortado: significa margem, ou um
ledger sem seus depósitos, e ambos merecem ser vistos.

### Análise

A aba **Análise** responde desempenho, risco e exposição em um só lugar. Sete visões
independentes, cada uma pedindo apenas os dados de que precisa, então *Book* renderiza instantaneamente em uma instalação nova
enquanto *Stress* paga por candles.

Uma visão que não consegue responder **diz por quê e o que fazer a respeito**. Ela nunca mostra um zero que não
mediu. Sem histórico ainda, um book curto demais para anualizar, nenhum benchmark escolhido, nenhum candle
para ele, nenhuma meta definida: cada um é uma frase e um botão, não um gráfico vazio.

| Visão | Precisa de | Responde |
|---|---|---|
| **Book** | o ledger, nada mais | patrimônio líquido, investido contra caixa, não realizado, realizado, renda, alocação por classe |
| **Performance** | histórico diário | retorno, IRR, anualizado, aportes líquidos, por janela |
| **Risk** | histórico diário | volatilidade, drawdown, Sharpe, Sortino, Calmar, melhores e piores períodos |
| **Benchmark** | histórico diário e os candles do benchmark | o que o índice teria rendido à sua volatilidade, alpha, beta, captura |
| **Income & costs** | as linhas de caixa do ledger | renda recebida, custos pagos, arrasto anual, a curva sem taxas |
| **Allocation** | uma alocação-alvo | atual contra alvo, desvio, os trades que o fecham |
| **Stress** | candles diários por posição | replay histórico e choques de fatores, com a parcela coberta |

Toda visão roda sobre o mesmo seletor de janela: 1M, 3M, 6M, YTD, 1A, 3A, 5A, tudo, ou um intervalo
personalizado. Uma janela mais longa que seu histórico é informada como **não coberta**, com os dias que
ela realmente tem, em vez de passar por três anos completos.

### Desempenho e risco

O **Performance** informa um retorno ponderado pelo tempo ao lado de uma IRR, e elas respondem perguntas
diferentes. O ponderado pelo tempo é o que os investimentos fizeram, ajustado pelos depósitos, porque um aporte
não é uma alta. A IRR é o que **você** obteve, ponderada pelo dinheiro, então comprar bem no momento certo aparece
ali e em nenhum outro lugar. Os aportes líquidos ficam ao lado, e um retorno abaixo de dois meses não é
anualizado: multiplicar seis semanas por oito é uma previsão, não uma medida.

O **Risk** lê a mesma curva: volatilidade, drawdown máximo, Sharpe, Sortino, Calmar, a parcela de
dias que terminaram em alta, melhor e pior dia, mês, trimestre e ano, e todo drawdown maior
que 2 % com **quanto tempo levou para ser recuperado**. Um ainda aberto é marcado como em andamento, com quanto
você está abaixo da última máxima e há quantos dias.

O fator de anualização é **medido na sua própria curva**, não presumido. Um book de ações opera
cerca de 252 dias por ano e um de cripto opera 365, e um misto não é nenhum dos dois. Sharpe e
Sortino usam a taxa livre de risco definida nas configurações de medição.

### Benchmark

Escolha um instrumento cujos candles diários você tenha (SPY, QQQ, BTCUSDT) e a página responde à
única pergunta que encerra uma discussão: **o que aquele índice teria rendido à sua
volatilidade**, ao lado do que você realmente fez. Bater o índice assumindo três vezes o risco dele
não é bater o índice.

Embaixo: retorno total e anualizado de ambos, volatilidade, drawdown máximo e Sharpe lado a
lado, depois alpha, beta, tracking error, information ratio e captura de alta e de baixa.

Seu book é medido nas **sessões do próprio benchmark**. Compare uma carteira 24/7 com um
índice dia a dia e toda segunda-feira do índice engole um fim de semana seu, o que
subestima silenciosamente o seu retorno.

### Renda e custos

O que você recebeu, o que pagou e o que pagar lhe custou. Dividendos, juros e
cupons de um lado; taxas de trading e taxas da conta do outro, com o arrasto anual como
parcela do seu patrimônio líquido médio.

A curva é desenhada duas vezes: como aconteceu, e o mesmo book com as pernas de taxa removidas. As taxas
já estão dentro da sua base de custo e do seu caixa, então isto é uma comparação, não uma subtração
que você mesmo poderia fazer.

### Alocação-alvo

Diga que parcela do patrimônio líquido cada bucket deve ter e quanto ele pode se desviar antes de contar
como fora, em **Definir alvos**. Uma alocação precisa somar 100 %, e um bucket pode receber o
restante com um clique. Salvar uma lista vazia desliga a visão.

A visão então mostra atual contra alvo por bucket, o desvio, se cada um está dentro
da sua faixa, e os **trades que fechariam a diferença**: compre tanto disto, venda tanto
daquilo. Tudo o que você tem sem alvo é listado, não ignorado.

Somente exibição. Nada aqui envia uma ordem, e nada rebalanceia sozinho.

### Stress testing

Dois motores, e ambos dizem quanto do seu book o número cobre.

O **replay histórico** aplica o caminho diário realizado de 2008, 2020, 2022, do 4º trimestre de 2018 ou do topo cripto
de 2021 ao que você tem hoje, usando os candles dos próprios instrumentos nessas datas. Sem
modelo, sem proxy. Um instrumento que não existia então não tem caminho: ele é **nomeado e
excluído**, nunca substituído por um índice.

O **choque de fator** move um instrumento real (S&P 500, Nasdaq, juros, EUR/USD, petróleo, spreads
de crédito) e chega a cada posição por uma sensibilidade **medida**, ajustada nos próprios
candles. Uma posição sem candles, com histórico curto demais ou com um ajuste sem poder explicativo
fica **sem beta**: ela cai em *não explicado* com seu peso, e o destaque diz
"−11,8 % nos 74 % do book que puderam ser medidos". O caixa tem beta zero, que é
em geral a única diversificação já existente.

Um choque de juros em pontos-base chega a um título por uma duration escrita no cenário, para que o
número possa ser contestado. Recessão, pico de inflação e alargamento de crédito vêm como
combinações editáveis dessas pernas.

Um painel de prontidão lista o que pode ser testado sob estresse e o que não pode antes de você rodar qualquer coisa, para que um
resultado fraco seja explicado de antemão e não depois.

### Histórico diário

Toda medida acima, exceto *Book*, precisa de uma curva, e os snapshots só começam no dia em que você liga
o job diário. Uma carteira que você mantém há seis anos seria medida a partir da
terça-feira passada. Então a curva é **reconstruída a partir do ledger e dos candles armazenados**, dia a dia.

A engrenagem na aba Análise abre a **Configuração de medição**:

1. **Nomeie o ticker de candle de cada ativo** e a moeda em que esses candles são cotados. Um ticker
   no seu ledger nem sempre é o símbolo que seu provedor serve, e uma ação comprada em EUR
   precificada contra candles em USD fica errada pela taxa de câmbio.
2. **Baixe os candles faltantes**. Eles são enfileirados como jobs comuns do [Historical Data](/pt/modules/market-data#histdata)
   pelos connectors concedidos às carteiras. Se nenhum deles tem um dos seus
   instrumentos, ele é nomeado, com o que conceder.
3. **Reconstrua a curva**. Ela informa os dias reconstruídos e os dias pulados porque uma posição
   não tinha candle naquele dia. Um dia que não pode ser avaliado não é armazenado, em vez de armazenado errado.

Editar uma operação datada no passado marca a curva como **obsoleta a partir dessa data** e diz isso.
Reconstruir continua sendo decisão sua. Atualizar uma carteira também baixa o que falta e estende
a curva por trás, e avisa quando um broker não consegue servir um dos seus instrumentos.

### Importar um ledger de operações

**Importar** no cabeçalho da carteira lê uma exportação de broker, uma planilha ou outro tracker (CSV, TSV, JSON, Parquet) e transforma cada linha em uma operação de compra ou venda. O mesmo motor de detecção da [importação do journal](/pt/modules/journal#importar-um-livro-de-trades): cabeçalhos em seis idiomas mais detecção de valores, delimitador, decimal e convenções de data decididos por coluna, colunas não identificadas deixadas sem mapeamento.

Três coisas que ele se recusa a adivinhar:

- **O que um símbolo é.** Todo símbolo do arquivo precisa apontar para um ativo: um que você já tem nesta carteira (associado automaticamente), um novo ativo a criar, ou *ignorar*. Um símbolo não resolvido bloqueia a importação, e os ativos só são criados depois que você confirma.
- **Uma linha que não é compra, venda nem um tipo que ele reconheça** é listada como erro de linha em vez de ser inventada como uma operação. Dividendos, depósitos, retiradas, taxas e impostos **são** reconhecidos, em seis idiomas, e registrados como são. Se o arquivo não informa nenhum tipo, defina o padrão uma vez para toda a importação.
- **Um preço ausente** é derivado de valor ÷ quantidade e sinalizado, nunca preenchido em silêncio.

Nada é gravado até você validar a prévia. Cada importação é um **lote**, revertível por inteiro (os ativos criados permanecem), e desduplicada **por carteira**, então reimportar o mesmo arquivo não muda nada.

### Importar holdings de um broker

**De um broker** no cabeçalho da carteira lê o balanço de uma [conta de broker](/pt/config/brokers) em vez de um arquivo: a diferença contra o seu ledger é mostrada linha a linha, e cada linha que você aceita grava a única operação que faz a carteira concordar. A base de custo é usada onde o broker publica uma (Interactive Brokers) e solicitada onde não publica (as exchanges de cripto publicam uma quantidade e nada mais).

## MyWealth {#wealth}

Patrimônio líquido de **tudo**: contas de corretagem, imóveis, cripto, caixa, bens de valor. Onde o Portfolio Tracker acompanha posições com preço ao vivo, o MyWealth acompanha qualquer ativo que você avalia por conta própria.

- Adicione ativos com nome, tipo, moeda e categoria, depois **registre atualizações de valor** ao longo do tempo (preço × quantidade, ou um valor direto, com uma nota). O histórico é editável.
- **Gráfico de patrimônio líquido** por mês ou ano, mais um detalhamento por categoria. Multimoeda com o mesmo tratamento de câmbio do journal (ativos sem taxa são excluídos e sinalizados).
- **Modelos**, como os do journal: campos reservados de preço/quantidade alimentam o valor, campos personalizados guardam notas por revisão.
- **Possuído ou devido**: um ativo pode ser um **passivo** (uma hipoteca, um empréstimo), então o destaque é um patrimônio líquido real. A página mostra o que você possui e o que você deve antes de compensá-los.
- **Vincule uma carteira** em vez de copiá-la: uma carteira vinculada é lida ao vivo do tracker a cada vez, então seu valor no seu patrimônio líquido nunca é uma cópia obsoleta.
- **Idade da avaliação**: diga a um ativo com que frequência ele deve ser reavaliado e a página nomeia os que envelheceram além disso, o mais antigo primeiro. Uma casa avaliada há três anos está errada em silêncio, e é isto que quebra o silêncio. Uma carteira vinculada nunca fica obsoleta: ela é lida, não lembrada.

## Managers' Portfolios {#mportfolios}

Navegue pelas **carteiras 13F de superinvestidores**: o que gestores de fundos famosos têm, tamanhos de posição, atividade recente, valor informado vs atual, faixas de 52 semanas. Filtre por gestor ou por ticker (*quem tem AAPL?*).

Como os dados 13F mudam trimestralmente, você pode **salvar snapshots** de qualquer carteira e comparar ao longo do tempo.

## Tax Calculator {#taxcalc}

Estimativa aproximada de impostos de trading e investimento. **Não é aconselhamento fiscal.**

- Os **perfis** partem de **modelos por país** (pessoa física ou profissional) e continuam totalmente editáveis: alíquota marginal de renda, encargos sociais, isenções de ganhos de capital e dividendos, **faixas de imposto sobre patrimônio** opcionais (por exemplo CH, ES, NO), faixas de alívio de longo prazo.
- Informe os números no modo **Resumo** (valor inicial/final, aportes, retiradas, parcela realizada) ou no modo **Detalhado** (ganhos de capital, ganhos de derivativos, ganhos de cripto, dividendos, juros, prejuízos anteriores carregados).
- **Carregar o Trading Journal**: com o journal instalado, um clique carrega o PnL realizado de um ano fiscal, dividido em ganhos de capital / derivativos / cripto, convertido à taxa de câmbio do fim do ano.
- **De um broker**: com uma [conta de broker](/pt/config/brokers) concedida ao tax calculator, um ano fiscal é lido direto da conta, dobrado em posições fechadas e totalizado por linha do formulário, cada disposição convertida à taxa da sua própria data de saída.
- Os resultados mostram o imposto estimado com um detalhamento por item (tributável, isenção, base, alíquota) e a alíquota efetiva. Salve cenários no histórico para comparar.

## Subscriptions {#subscriptions}

Todo custo recorrente em uma lista (ferramentas de trading, feeds de dados, streaming) com preço, moeda, frequência de cobrança (semanal/mensal/trimestral/anual) e categoria.

Você recebe **gráficos de gasto** mensais/anuais (agrupados ou por assinatura), o **equivalente mensal** de cada assinatura, próximas datas de cobrança e totais do próximo mês. Pause uma assinatura para mantê-la listada sem contá-la.
