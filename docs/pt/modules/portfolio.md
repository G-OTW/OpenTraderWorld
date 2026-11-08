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

### Colunas e visualizações

- **Seletor de colunas**: a engrenagem no fim do cabeçalho da tabela escolhe as colunas de cada lista: valor de mercado, volume, P/L, dividend yield, RSI(14), faixa de 52 semanas, distância da máxima de 52 semanas, alvos. Um campo que a fonte não fornece mostra **n/d**, nunca 0 (o Yahoo fornece a faixa de 52 semanas e o volume, mas não valor de mercado, P/L nem yield; o CoinGecko fornece valor de mercado e volume).
- **Barra de posição de 52 semanas**: onde o preço está na sua faixa anual, exibida por padrão. Cripto na fonte padrão não tem faixa de 52 semanas; um ano de candles diários salvos a preenche quando existe.
- **Mapa de calor**: um bloco por símbolo, dimensionado pelo valor de mercado (tamanho igual quando desconhecido), colorido pela variação de 24 h, 7 d ou 30 d.
- **Comparar**: todos os símbolos rebaseados a 100 em 1M, 3M ou 1A. 1M vem das cotações em cache; 3M e 1A precisam de candles diários salvos em [Historical Data](/pt/modules/market-data#histdata), e um símbolo sem eles aparece como sem cobertura com a solução.
- **Correlação**: a correlação par a par dos retornos diários na janela, nos dias em que ambos os símbolos negociaram. Um par com menos de 10 dias em comum mostra um traço.

### Ferramentas de linha

- **Seções**: agrupe linhas (núcleo, candidatos, saídas), recolha um grupo, arraste uma linha para outra seção, renomeie uma seção pelo cabeçalho.
- **Bandeiras coloridas**: um ponto colorido antes do símbolo e um filtro por bandeira usada.
- **Alvos**: um alvo de compra e um de venda por símbolo com a distância até cada um; o sino ao lado de um alvo abre um alerta de nível preenchido com esse preço.
- **Posição virtual**: "se eu tivesse comprado N a P em D", com o P/L e o % sob o preço. Nada é registrado em lugar nenhum.
- **Eventos**: próximas datas de resultados, ex-dividendo e desdobramento por linha e numa faixa Próximos, lidas do que o [Fundamentals](/pt/modules/fundamentals) salvou. Sem dados salvos, a faixa avisa.
- **Selo de notícias**: quantas notícias do News & Research citam o símbolo desde a última vez que a lista foi marcada como lida; um clique abre as notícias filtradas por ele.
- **Abrir no gráfico**: abre o símbolo no gráfico com o ticker do seu provedor. Cripto na fonte CoinGecko padrão precisa antes de uma fonte de cotação fixada.
- **Adicionar à carteira**: adiciona o símbolo a uma carteira escolhida do Portfolio Tracker com uma primeira compra preenchida ao preço atual.

### Colar tickers e resumo diário

- **Colar**: a janela Adicionar símbolos aceita uma lista colada (vírgulas, espaços ou quebras de linha). Cada ticker só é resolvido com correspondência exata, nada é adicionado antes da sua confirmação, um ticker ambíguo lista as correspondências para escolher e um desconhecido diz como encontrá-lo.
- **Resumo diário de variações**: nas configurações da lista (engrenagem ao lado do título), uma vez por dia na hora UTC escolhida, as três maiores altas e baixas em 24 h vão para os canais de notificação escolhidos.

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

Toda visão roda sobre o mesmo seletor de janela: 1M, 3M, 6M, YTD, 1A, 3A, 5A, Tudo, ou um intervalo
personalizado. Uma janela mais longa que seu histórico é informada como **não coberta**, com os dias que
ela realmente tem, em vez de passar por três anos completos.

### Desempenho e risco

O **Performance** informa um retorno ponderado pelo tempo ao lado de uma IRR, e elas respondem perguntas
diferentes. O ponderado pelo tempo é o que os investimentos fizeram, ajustado pelos depósitos, porque um aporte
não é uma alta. A IRR é o que **você** obteve, ponderada pelo dinheiro, então comprar bem no momento certo aparece
ali e em nenhum outro lugar. Os aportes líquidos ficam ao lado, e um retorno sobre menos de um quinto de ano
(cerca de 73 dias) não é anualizado: multiplicar seis semanas por oito é uma previsão, não uma medida.

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

### Eventos corporativos, lotes e transferências

**Eventos corporativos.** O grupo **Evento corporativo** do formulário de caixa registra um desdobramento, um grupamento ou uma cisão em uma posição. Um desdobramento multiplica as unidades e divide o custo unitário pela mesma razão; nada é realizado. Uma cisão leva a parte do custo informada para a nova posição, com as datas de compra da controladora, que mantém suas unidades.

**Lotes fiscais.** Clique em um símbolo para abrir a posição. **Lotes** lista cada compra aberta com custo, valor, dias de detenção e prazo curto ou longo. O método de casamento, o limite de detenção e a janela anti-wash vêm do seu primeiro perfil do [Tax Calculator](#taxcalc); sem perfil, vale FIFO sem prazo nem regra anti-wash, e a tela informa.

**Transferência entre carteiras.** **Transferência** na posição move unidades para outra carteira com seus lotes, custo e datas de compra. Nada é realizado em nenhum dos lados; o ativo de destino é criado se faltar.

### Renda, tese e classificação

**Calendário de dividendos.** A aba **Renda** prevê a renda anual por posição (unidades × valor por pagamento × pagamentos por ano), mostra o rendimento sobre custo, as próximas datas ex e de pagamento e a renda registrada em doze meses ao lado da previsão. As fontes de cotação não trazem dividendos: você informa valor, frequência e datas na posição, e uma posição sem eles aparece *sem estimativa*. O Dashboard tem um cartão **Upcoming income**.

**Tese e data de revisão.** Cada posição tem sua própria tese e uma data de revisão. Teses que vencem em uma semana aparecem em uma faixa na página de carteiras.

**Setor e país.** Digitados na posição, ou lidos de [Fundamentals](/pt/modules/fundamentals) para uma ação conhecida. Um ETF fica não classificado até você digitar. O donut de alocação agrupa por setor ou país, e a parte não classificada é uma fatia própria.

### Entre carteiras

**Todas as carteiras.** Com mais de uma carteira, um bloco acima da lista soma tudo em uma moeda: valor, custo, P/L, caixa, símbolos presentes em várias carteiras com o peso somado, moedas de negociação e maiores posições.

**Comparar.** Analysis → **Comparar** desenha esta carteira e outra na mesma curva, ponderadas no tempo e em 100 no primeiro dia em comum.

### Atribuição e metas

**Contribuição.** Abaixo da tabela de desempenho, barras mostram quanto cada posição trouxe ao retorno da janela e ao maior drawdown, a partir dos fechamentos diários. As contribuições somam o retorno da carteira reconstruída. Uma posição sem fechamentos diários na janela é citada e as barras não são desenhadas.

**Prejuízos.** Analysis → **Prejuízos** lista os lotes no negativo, do pior ao menor, com a janela anti-wash do regime e um sinal em cada lote comprado dentro dela. **Enviar ao Tax Calculator** o abre com a perda preenchida.

**Metas.** Analysis → **Projeção** recebe valor alvo, data e aporte mensal, e mostra o progresso, o aporte necessário ao retorno medido e um cone de 1000 trajetórias a partir do retorno e da volatilidade medidos. Uma curva com menos de um quinto de ano é recusada em vez de extrapolada.

### Planos e notificações

**Operações planejadas.** **Plano** na posição registra uma compra ou venda prevista a um preço. A cotação do ativo é verificada a cada hora; o plano é marcado atingido uma vez e pode notificar. **Registrar** abre o formulário preenchido.

**Alertas e resumo.** **Alertas** no cabeçalho da carteira define o peso máximo de uma posição (uma posição acima é sinalizada), uma notificação ao ultrapassá-lo, uma notificação quando uma classe alvo sai da banda (também no diálogo de metas de alocação) e um resumo diário ou semanal com valor, variação, os três maiores movimentos e a renda desde o anterior. Cada alerta dispara uma vez por violação e se rearma quando a carteira volta aos limites. As mensagens vão aos [canais de notificação](/pt/config/settings#notifications) autorizados para o Portfolio Tracker.

## MyWealth {#wealth}

Patrimônio líquido de **tudo**: contas de corretagem, imóveis, cripto, caixa, bens de valor. Onde o Portfolio Tracker acompanha posições com preço ao vivo, o MyWealth acompanha qualquer ativo que você avalia por conta própria.

- Adicione ativos com nome, tipo, moeda e categoria, depois **registre atualizações de valor** ao longo do tempo (preço × quantidade, ou um valor direto, com uma nota). O histórico é editável.
- **Gráfico de patrimônio líquido** por mês ou ano, mais um detalhamento por categoria. Multimoeda com o mesmo tratamento de câmbio do journal (ativos sem taxa são excluídos e sinalizados).
- **Modelos**, como os do journal: campos reservados de preço/quantidade alimentam o valor, campos personalizados guardam notas por revisão.
- **Possuído ou devido**: um ativo pode ser um **passivo** (uma hipoteca, um empréstimo), então o destaque é um patrimônio líquido real. A página mostra o que você possui e o que você deve antes de compensá-los.
- **Vincule uma carteira** em vez de copiá-la: uma carteira vinculada é lida ao vivo do tracker a cada vez, então seu valor no seu patrimônio líquido nunca é uma cópia obsoleta.
- **Idade da avaliação**: diga a um ativo com que frequência ele deve ser reavaliado e a página nomeia os que envelheceram além disso, o mais antigo primeiro. Uma casa avaliada há três anos está errada em silêncio, e é isto que quebra o silêncio. Uma carteira vinculada nunca fica obsoleta: ela é lida, não lembrada.
- **Cronograma do empréstimo**: informe valor, taxa anual, prazo e data de início (e a parcela, se a do credor for diferente). O saldo segue a fórmula de amortização, um valor por vencimento é gravado para você e Atualizar lista as parcelas restantes com juros e amortização.
- **Associar um ativo à sua dívida**: um ativo pode indicar o passivo que pesa sobre ele (uma casa e seu financiamento); o detalhe mostra o patrimônio e a relação empréstimo sobre valor.
- **Deriva anual**: uma taxa suposta (+3 % para imóveis, -15 % para um carro) estima o valor após a última avaliação real. O gráfico a desenha pontilhada, ela nunca é gravada como avaliação e o cabeçalho mostra quanto é estimado.
- **Conta de corretora e endereço de carteira**: a aba Conta de corretora do diálogo de vínculo adiciona o patrimônio de uma conta como ativo, lido com as credenciais dela (a conta precisa estar autorizada para o MyWealth em Configurações, Corretoras). Um ativo Endereço de carteira lê um endereço BTC ou ETH público na rede, precificado em USD. Ambos são lidos no máximo uma vez por hora e guardados como o valor do dia; uma leitura com falha mostra o erro na linha, nunca um zero.
- **Metas**: patrimônio líquido e data desejados, despesas mensais. A aba mostra o crescimento medido na sua curva (12 meses de histórico necessários), a data projetada, a poupança em 12 meses sem mercados e deriva, a taxa de poupança e o número FIRE (25 × despesas anuais).
- **Abaixo do gráfico**: Variações divide a mudança de um período por categoria em reavaliação, entradas e saídas; Antes e agora coloca a tabela de categorias de 1 mês, 1 ano ou 5 anos atrás ao lado de hoje; Liquidez divide as categorias marcadas como líquidas pelas despesas mensais mais a previsão de Assinaturas; Alvos define fatia e banda por categoria e mostra o desvio, com o cálculo dos alvos do Portfolio Tracker.
- **Patrimônio líquido real**: o botão Real deflaciona a curva por um índice de preços carregado em Fundamentals (CPI dos EUA por padrão, outro em Configurações). Sem índice, o gráfico avisa e oferece carregar o CPI dos EUA do FRED.
- **Titular**: atribua cada ativo a um membro da família, filtre por titular e veja a divisão na visão Titulares do gráfico.
- **Documentos**: anexe uma escritura, um contrato ou um extrato (10 MB cada) a um ativo ou a uma de suas avaliações.
- **Aviso de avaliações vencidas**: em Configurações, um aviso semanal lista os ativos cuja avaliação passou do intervalo de revisão e envia pelos canais autorizados para o MyWealth.
- **Exportação e calculadora de impostos**: o menu do cabeçalho exporta ativos e avaliações em CSV; Calculadora de impostos soma as categorias escolhidas e abre a calculadora com a base do imposto sobre fortunas preenchida. A demo recusa exportações e downloads de documentos.

## Managers' Portfolios {#mportfolios}

Navegue pelas **carteiras 13F de superinvestidores**: o que gestores de fundos famosos têm, tamanhos de posição, atividade recente, valor informado vs atual, faixas de 52 semanas. Filtre por gestor ou por ticker (*quem tem AAPL?*).

Como os dados 13F mudam trimestralmente, você pode **salvar snapshots** de qualquer carteira e comparar ao longo do tempo.

## Tax Calculator {#taxcalc}

Estimativa aproximada de impostos de trading e investimento. **Não é aconselhamento fiscal.**

- Os **perfis** partem de um **regime fiscal** por país e ano fiscal: França (taxa única, ou profissional), Alemanha, Reino Unido, Estados Unidos, Austrália, Suíça, Espanha e Itália. Um regime define o calendário do ano fiscal, o método de custo de aquisição (preço médio ponderado, FIFO ou o pool britânico, ajustável por perfil), a regra anti-wash quando o país tem uma, isenções e alíquotas, e mostra suas fontes e a data em que suas regras foram verificadas. As **alíquotas personalizadas** cobrem qualquer outro país: alíquota única ou marginal, encargos sociais, isenções de ganhos de capital e dividendos, alívio opcional por tempo de detenção. Qualquer perfil pode adicionar **faixas de imposto sobre patrimônio** (por exemplo CH, ES, NO).
- **Reporte de perdas**: registre o resultado líquido de cada ano fiscal por pool (valores mobiliários, cripto...). O que ainda é levado para o ano é calculado a partir dessas linhas, a perda mais antiga primeiro, e expira após o prazo de reporte do regime.
- Informe os números no modo **Resumo** (valor inicial/final, aportes, retiradas, parcela realizada) ou no modo **Detalhado** (ganhos de capital, ganhos de derivativos, ganhos de cripto, dividendos, juros, prejuízos anteriores carregados).
- **Carregar o Trading Journal**: com o journal instalado, um clique carrega o PnL realizado de um ano fiscal, dividido em ganhos de capital / derivativos / cripto, convertido à taxa de câmbio do fim do ano.
- **De um broker**: com uma [conta de broker](/pt/config/brokers) concedida ao tax calculator, um ano fiscal é lido direto da conta, dobrado em posições fechadas e totalizado por linha do formulário, cada disposição convertida à taxa da sua própria data de saída.
- Os resultados mostram o imposto estimado com um detalhamento por item (tributável, isenção, base, alíquota) e a alíquota efetiva. Salve cenários no histórico para comparar.

## Subscriptions {#subscriptions}

Todo custo recorrente em uma lista (ferramentas de trading, feeds de dados, streaming) com preço, moeda, frequência de cobrança (semanal/mensal/trimestral/anual) e categoria.

Você recebe **gráficos de gasto** mensais/anuais (agrupados ou por assinatura), o **equivalente mensal** de cada assinatura, próximas datas de cobrança e totais do próximo mês. Pause uma assinatura para mantê-la listada sem contá-la.

### Ciclo de vida, lembretes e orçamentos

- **Histórico de preços**: mudar um preço ou moeda fica registrado. Os detalhes da linha mostram o preço em degraus, e o gráfico mensal traça uma linha tracejada de «aumento de custo»: quanto custava a cada mês tudo o que estava ativo, ao preço da época.
- **Lembretes de renovação**: marque «Lembrar antes da cobrança», defina os dias e escolha canais (nenhum = todos os canais autorizados para Subscriptions). Cada data de cobrança é lembrada uma vez. Um lembrete precisa de data de início ou de fim do teste.
- **Testes grátis**: marque «Teste grátis» e defina a data de fim. A cobrança começa nesse dia, a linha ganha um selo e os testes prestes a converter aparecem primeiro.
- **Cancelar**: o botão de parar define uma data de fim. A assinatura sai de todos os totais a partir desse dia, mantém o histórico e o painel conta o que economizou desde então. Filtro ativas / canceladas / todas. Excluir continua para erros.
- **Mudar para anual**: para um plano cobrado com mais frequência, informe o preço anual. A coluna «Economia anual» mostra 12 meses no ritmo atual menos o preço anual.
- **Avaliação de uso**: um aviso pede para avaliar cada serviço ativo de 1 a 5 a cada trimestre. A visualização «Vale a pena?» ordena os serviços avaliados por custo mensal por ponto, os mais caros para o uso primeiro.
- **Orçamentos**: defina um teto mensal por categoria na moeda de exibição. Uma categoria acima do teto aparece no painel e notifica uma vez por mês.
- **Calendário**: a visualização Calendário mostra as datas e valores de cobrança do mês. «Exportar .ics» baixa os próximos 12 meses como eventos de calendário (bloqueado na demo).
- **Forma de pagamento e validade do cartão**: registre qual cartão paga, filtre por ele e veja um selo quando ele vence em até 60 dias ou já venceu.
- **Planos compartilhados**: informe sua parte em % de um plano familiar. Preços, equivalente mensal e todos os totais usam sua parte.
- **Previsão**: a aba Previsão mostra as cobranças dos próximos 12 meses do calendário, mês a mês, com a média mensal. O MyWealth lê os mesmos números.
