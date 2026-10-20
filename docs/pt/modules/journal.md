# Trading Journal

Registre cada trade, em qualquer moeda, e obtenha estatísticas de desempenho honestas: curva de patrimônio, win rate, expectancy, profit factor, drawdown, Sharpe e mais. O journal é organizado em dez abas: **Resumo**, **Análise**, **Calendário PnL**, **Trades**, **Estratégias e capital**, **Tags**, **Modelos**, **Taxas e moeda**, **Importar**, **Tarefas pendentes**.

## Categorias

Os trades vivem em **categorias**: pastas como *Scalping de cripto* ou *Ações de longo prazo*, cada uma com sua própria cor, capital e estatísticas. Crie-as pela barra de categorias; arraste para reordenar. Excluir uma categoria exclui seus trades.

## Defina o capital

Em **Estratégias e capital**, dê a cada categoria uma **banca inicial** e registre **aportes** e **retiradas** ao longo do tempo. É contra isso que retorno, curva de patrimônio e drawdown são calculados; sem isso você ainda tem o PnL, mas não os retornos.

Você também pode nomear **estratégias** com seus nomes de sinais (por exemplo *Breakout, Pullback*). Marque os trades com uma estratégia/sinal e a aba Resumo pode filtrar por eles: é assim que você descobre quais setups realmente pagam.

## Modelos

Os modelos conduzem o formulário de trade. Existe um **trade padrão** pré-pronto; crie os seus por mercado ou estilo:

- **Campos reservados** (lado, preços, quantidade, taxas, alavancagem, multiplicador, moeda, tipo de unidade…) alimentam as estatísticas de desempenho.
- **Campos personalizados** (texto, números, listas de escolha…) são livres: nota do setup, condição de mercado, o que você acompanhar.
- Um modelo pode definir uma **tabela de taxas padrão**, pré-selecionada ao registrar a partir dele (substituível por trade).

## Registrando trades

Na aba Trades, escolha um modelo (ou o *Rápido*, que mostra todos os campos) e preencha o formulário. Dois níveis:

- **Simples**: uma entrada, uma saída (ou deixe a saída vazia para uma posição aberta).
- **Avançado**: aumento/redução de posição com várias **pernas de entrada e saída** (cada uma com seu preço, quantidade, taxas, sinal), mais **brackets de SL/TP**. Quando um bracket dispara, marque-o e ele se dobra em uma perna de saída.

O formulário mostra uma prévia da entrada média, do PnL líquido e da quantidade aberta enquanto você digita. Você pode anexar até duas imagens (capturas de gráfico), escolher alavancagem e multiplicador do contrato para derivativos, e escrever seu próprio feedback sobre o trade.

**O PnL é calculado na leitura** e lida com posições parcialmente abertas. A base de custo é alternável entre **custo médio ponderado** (padrão) e **FIFO** (útil para exportação de imposto), e a escolha é aplicada de verdade, tanto nas estatísticas quanto na prévia de PnL ao vivo do formulário de trade.

## Taxas

Em **Taxas e moeda**, salve **tabelas de taxas**: fixas ou percentuais, cobradas por lote, unidade, contrato ou trade (por exemplo *Ações IBKR: 0,05 % por trade*). Selecionar uma tabela em um trade calcula a taxa automaticamente; uma taxa digitada manualmente sempre vence.

## Multimoeda e câmbio

Os trades mantêm a moeda em que você os digitou. A **moeda do resumo** (exibição) é convertida usando um feed de câmbio diário que preenche as taxas automaticamente a cada dia útil, carregando as taxas adiante em fins de semana e feriados.

Se uma taxa não puder ser obtida para alguma data, esses trades são **excluídos dos totais convertidos** e aparecem em **Tarefas pendentes**, onde você digita à mão as taxas ancoradas em USD que faltam (1 USD = … dessa moeda) e os trades voltam a contar.

## Resumo (suas estatísticas)

Por categoria ou em todas, filtrável por intervalo de datas, ticker, lado, classe de ativo, estratégia, sinal e tag. A barra de filtros é compartilhada com a Análise, então um escopo definido em uma tela é o escopo da outra, e a lista de tickers oferece os símbolos que o journal realmente tem:

- **Curva de patrimônio** na moeda de exibição.
- PnL realizado · Retorno · Win rate · Trades (fechados/abertos) · Expectancy · Profit factor · Ganho médio / Perda média · Melhor / Pior trade · Drawdown máximo · Sharpe e Sortino · Total de taxas · Capital investido · Margem utilizada · Retorno sobre a margem.
- **Sharpe e Sortino** são calculados sobre retornos diários contra o patrimônio levado a cada dia, anualizados, a mesma definição que a Análise usa, então as duas telas concordam.

## Análise (lendo o book)

O book inteiro em oito abas, sobre os mesmos filtros do Resumo:

- **Visão geral**: expectancy em **R** e R total (sobre os trades que carregam um stop planejado), risco por trade, Sharpe com o Sortino ao lado, drawdown máximo com os dias passados abaixo do pico, dias de trading ganhos e perdidos, dia médio, sequência atual e melhor, depois a **distribuição de R** e o custo dos seus erros marcados com tag.
- **Distribuições**: PnL líquido por tempo de permanência, hora de entrada, dia da semana de entrada e tamanho da posição, e a contagem de trades por faixa de lucro. Qual hora do seu dia realmente paga.
- **Comportamento**: o que você faz em torno da vantagem, e quanto custa. Concentração de lucro, tamanho após uma sequência de perdas, ritmo após uma perda, como o dia decai, e o trade após uma vitória contra o trade após uma perda. Veja [Análise de comportamento](#behavior).
- **Dados de mercado**: os candles por trás dos seus trades. MAE e MFE, eficiência de saída, o que ficou na mesa, distância do stop em ATR, e resultados divididos por regime de volatilidade e por tendência na entrada. Veja [Enriquecimento com dados de mercado](#market-data).
- **Risco aberto**: a única aba sobre o presente. O que ainda está em jogo, onde esse risco se concentra, e se cinco linhas abertas são cinco apostas ou uma. Veja [Risco aberto](#open-risk).
- **Dispersão**: quaisquer dois valores de trade plotados um contra o outro (data, número do trade, líquido, PnL acumulado, retorno sobre o nocional, R, tempo de permanência, tamanho...), coloridos por resultado, lado, estratégia, ticker ou classe de ativo, com linha de tendência e zoom.
- **Repartição**: desempenho agrupado por estratégia, símbolo, tag, classe de ativo ou lado, com trades, win rate, líquido, expectancy, R médio e profit factor por linha.
- **Comparar**: este dia, semana, mês, trimestre, ano ou um intervalo personalizado contra o imediatamente anterior, linha a linha (líquido, trades, win rate, expectancy, R médio, profit factor, drawdown máximo, taxas, dias de trading), acima de uma faixa dos últimos doze períodos.

Trades fechados sem taxa de câmbio para a sua data são contados em voz alta, em vez de descartados em silêncio.

## Análise de comportamento {#behavior}

As estatísticas de desempenho dizem o que o book rendeu. O **Comportamento** diz como você chegou lá, e quais dos seus hábitos pagaram por isso. Os mesmos trades fechados do resto da Análise, a mesma barra de filtros, sem configuração extra e sem dados de mercado: ele lê os trades que você já registrou.

Cinco cartões, cada um respondendo a uma pergunta.

### De onde vem o lucro

A parcela do lucro bruto feita pelos seus cinco melhores trades, quantos ganhadores são necessários para fazer metade dele, e como fica a conta sem esses cinco. Ao lado, um índice de concentração: 0 significa que todo ganhador paga mais ou menos o mesmo, 1 significa que um único trade paga o ano. Ganho médio contra ganho mediano mostra a mesma assimetria por outro ângulo, e uma curva acumulada a desenha.

O número a olhar é o líquido sem os cinco melhores. Se ele for negativo, a vantagem repousa em outliers que você não consegue agendar.

### Tamanho após uma sequência de perdas

O nocional mediano de entrada agrupado pelo que veio antes do trade: depois de uma vitória, depois de uma perda, depois de duas, depois de três ou mais. Cada linha traz sua contagem de trades, win rate, expectancy, R médio e líquido, então a escalada é precificada, não apenas notada.

Aumentar o tamanho depois de duas perdas é o hábito mais caro que um journal pega. Uma linha plana aqui é a disciplina que a maioria dos books perde primeiro.

### Ritmo após uma perda

O intervalo mediano de uma saída até a próxima entrada, comparado depois de uma vitória e depois de uma perda. Um trade aberto em muito menos que **o seu próprio** intervalo habitual logo após uma perda é contado como revenge trade, já que um scalper e um swing trader não compartilham o mesmo relógio. Esses trades ganham sua própria linha: quantos, o que renderam, e quanto rendem em média contra todos os outros.

Os dias também são comparados, um dia com uma perda contra um limpo, em número de trades.

### Como o dia anda

Resultado médio pela posição do trade dentro do seu dia local: primeiro, segundo, terceiro, quarto e depois, com R médio por posição e quanto vale cada trade adicional do dia. Muitos books fazem seu dinheiro antes do almoço e devolvem depois. É aqui que isso aparece.

### Depois de uma vitória, depois de uma perda

O trade que **segue** um resultado, nunca o resultado em si. Trades, win rate, expectancy, R médio, tamanho mediano, risco médio, permanência mediana e intervalo mediano, lado a lado, com as três diferenças que importam (expectancy, tamanho, permanência) destacadas embaixo.

::: tip As afirmações têm um piso
A frase no topo de um cartão só é escrita acima de um **piso de amostra** (vinte trades fechados no escopo, oito de cada lado de uma comparação) **e** de um limiar de efeito. Abaixo de qualquer um, os cartões ainda são desenhados e rotulados como uma primeira olhada. Três trades não conseguem mostrar um hábito.
:::

## Enriquecimento com dados de mercado {#market-data}

O registro de trades conhece sua entrada, sua saída e seu stop. Ele não sabe para onde o preço foi enquanto você estava dentro, e é aí que estão a maioria das respostas úteis: se seus stops ficam dentro do ruído, quanto de cada movimento você realmente manteve, e em quais condições de mercado a estratégia funciona.

A aba **Dados de mercado** carrega os candles por trás dos seus próprios trades e os mede.

### Configure uma vez

Abra **Fontes** na aba:

- **Tamanho do candle**: *Automático* escolhe o timeframe mais grosso que ainda deixa cerca de vinte candles dentro de uma posição típica, lido a partir do seu próprio tempo mediano de permanência. Fixe um se preferir decidir.
- **Busca**: *Desligada* mede apenas o que já está armazenado, *Sob demanda* baixa quando você clica, *Automática* enfileira sozinha a janela faltante de um novo trade e avisa quando ela chega.
- **Fonte por tipo de ativo**: ações, ETFs, cripto, forex e futuros escolhem cada um um [data connector](/pt/config/connectors) concedido ao journal, ou *Automático*, que usa o primeiro connector concedido que serve aquele tipo.

Nada é baixado pelas suas costas, e os downloads são jobs comuns do [Historical Data](/pt/modules/market-data#histdata): mesma fila, mesma contabilidade de cota, mesma lista de jobs.

### Descobrir, baixar, medir

Três botões, nessa ordem.

- **Descobrir** lê o que os trades filtrados precisam contra o que você já armazena, e não grava nada. Por instrumento você recebe os trades no escopo, as barras armazenadas, as janelas faltantes e um status: *pronto*, *parcial*, *faltando*, *sem fonte*, *não suportado*, *contrato necessário*.
- **Baixar o que falta** enfileira essas janelas e acompanha o lote. Só os buracos são pedidos, e um buraco é pedido a partir das barras e não adivinhado por uma lacuna: uma série diária de ações falta todo fim de semana, uma série de cripto 24/7 nunca falta, então nenhuma largura de lacuna funciona nas duas.
- **Medir** percorre cada trade contra suas barras e armazena o resultado.

A medição é **incremental**. Uma medição armazenada é refeita quando o trade foi editado, quando novos candles chegaram, ou quando a granularidade mudou. *Remedir tudo* força todo o escopo, para quando você muda o tamanho do candle e quer todos os trades no mesmo patamar.

### Nomeando um contrato de futuros

Uma ação se chama a mesma coisa em todo lugar. Um contrato de futuros não, e o ticker do seu journal geralmente nomeia a raiz que você opera e não o contrato que seu provedor de dados serve.

Então o journal pergunta uma vez, em vez de adivinhar. Um instrumento que precisa disso mostra *contrato necessário*, e **Nomear o contrato** recebe o símbolo do jeito que sua fonte o escreve: `MNQU6` (o que o TWS mostra e o que você copia), ou a raiz com o mês do contrato, `MNQ.202609`, ou `MNQ.202609@CME` quando a raiz é listada em várias exchanges. Tudo a jusante usa esse símbolo.

Opções são informadas como **não suportadas** em vez de associadas ao seu subjacente. Medir um trade de opção contra os candles da ação produziria números que parecem certos e não significam nada.

### O que você recebe

- **Excursões**: MAE e MFE médios, em dinheiro e em unidades do risco planejado, com o tempo mediano da entrada até cada um.
- **Eficiência de saída**: a parcela do melhor movimento que você realmente manteve, e o que ficou na mesa em todos os trades medidos.
- **Os stops estão apertados demais**: distância mediana do stop em ATR na entrada, quantos stops ficam abaixo de um ATR, e quantos *ganhadores* primeiro passaram de 80 % do seu risco. Um stop dentro do ruído é um stop que o mercado leva no caminho até o seu alvo.
- **Os alvos estão perto demais**: ganhadores que mantiveram menos da metade do movimento oferecido, e o que estava aparecendo no melhor ponto contra o que foi para casa.
- **Quão fundo antes de funcionar**: o pior ponto de cada trade agrupado em R, de 0 a 0,25R até mais de 1,5R. É isso que diz onde um stop deve ficar.
- **Por regime de volatilidade** e **por tendência na entrada**: as mesmas estatísticas divididas em calmo / normal / volátil, e subindo / lateral / caindo.

Os regimes são **tercis do seu próprio book**, não limiares absolutos. Um limiar absoluto chamaria todo trade de cripto de volátil e não diria nada sobre quando sua estratégia funciona. Com menos de doze trades medidos nenhum regime é rotulado.

A aba Dispersão também lê isso: MAE contra R, eficiência contra tempo de permanência, a nuvem colorida por regime.

## Risco aberto {#open-risk}

Toda outra aba mede o passado. O **Risco aberto** mede o que ainda está em jogo agora.

Uma posição está aberta quando resta quantidade, e é contada nesse **resto**: um trade reduzido em três quartos carrega um quarto do risco, não o risco com que abriu.

### O destaque

- **Exposição bruta e líquida**, em dinheiro e como parcela da conta, longs e shorts somados e depois compensados.
- **Em jogo**: o que você perde se todo stop planejado for atingido. Posições sem stop registrado são contadas à parte e nomeadas, já que o que elas arriscam é desconhecido, não zero.
- **Resultado aberto**, marcado ao último fechamento armazenado, informando quantas posições puderam realmente ser marcadas.
- **Apostas efetivas**: quantas posições independentes suas linhas equivalem, por tamanho, e de novo às suas correlações medidas.

A tabela de posições as lista da maior para a menor, com lado, tamanho aberto, entrada, stop, último preço, valor, o que está em jogo, sua parcela do total, resultado aberto e dias mantida.

### Onde o risco está

Concentração por instrumento, classe de ativo, lado ou estratégia, calculada sobre o **risco** quando há stops registrados e recorrendo ao tamanho quando não há (o painel diz qual). Um instrumento carregando metade do que está em jogo é um fato sobre o seu book que nenhuma curva de patrimônio mostra.

### São apostas separadas

Cinco linhas que se movem juntas são uma posição com cinco vezes o tamanho. Para responder isso, a aba mede a correlação sobre **candles diários já armazenados**, qualquer que tenha sido a granularidade do enriquecimento, e nunca busca nada.

Três números, e só o terceiro descreve o seu book:

- **Somados**: todos os stops atingidos de uma vez, somados.
- **Se independentes**: qual seria o risco se nada se movesse junto.
- **Nestas correlações**: o que o book realmente arrisca.

A razão entre eles é o fator de **empilhamento**: 1,0 significa apostas realmente separadas, maior significa a mesma aposta várias vezes. Instrumentos sem candles armazenados são nomeados e deixados fora da matriz em vez de presumidos.

Os avisos são lidos como frases: um par se movendo a 0,9, um único instrumento carregando demais, posições sem stop, um book mais fino do que parece. Quando nada está errado, isso também é dito.

## Tags de disciplina

Uma **tag** é uma regra que você quebrou ou respeitou: *movi meu stop*, *sem setup*, *aumentei o tamanho depois de uma perda*. Marque-as nos trades em que se aplicam e a Análise as precifica: quantos trades fechados quebraram uma regra, quanto esses trades rendem em média contra os limpos, e a diferença entre os dois. Esse é o **custo dos erros**, em dinheiro.

## Calendário PnL

Uma grade mensal do PnL realizado diário, verde para dias de alta e vermelho para dias de baixa, escalada pelo maior dia do mês, com totais semanais ao lado. Clique em um dia para ir aos seus trades.

Ele também lê suas **rotinas de trading**. Anexe as rotinas que uma categoria segue, durante um período, e cada dia operado ganha um ponto: verde quando todas as rotinas devidas naquele dia foram marcadas, vermelho quando nenhuma foi, âmbar no meio. Um dia em que você não operou fica cinza, sejam quais forem as rotinas, e um dia sem rotina devida não recebe ponto nenhum. Passar o mouse nomeia cada rotina com sua própria marca.

As rotinas em si vivem em [Trading Routines](/pt/modules/productivity#routines) e são marcadas lá: o journal só registra quais um book segue, então o mesmo hábito nunca é escrito nem marcado duas vezes.

## Importar um livro de trades

A visão **Importar** pega um livro de trades que você mantém em outro lugar (uma exportação de broker, outro journal, uma planilha) e o transforma em trades do journal. Sem parser por broker: CSV, TSV, JSON e Parquet passam todos pela mesma detecção.

**Como funciona.** Solte o arquivo, o servidor propõe um mapeamento, você o confere contra uma prévia de trades reais, depois importa.

- A **detecção** lê os cabeçalhos (en, fr, es, de, it, pt, mais a terminologia comum dos brokers) *e* os próprios valores. Delimitador, separador decimal e datas dia-primeiro vs mês-primeiro são decididos por coluna. Uma coluna que ela não consegue identificar com confiança fica **sem mapeamento** em vez de adivinhada, e você a atribui por conta própria.
- **Prévia antes de gravar.** O passo de análise não grava nada: ele devolve os trades montados, os totais e os erros por linha, e roda de novo a cada edição do mapeamento, então o que você vê é exatamente o que será salvo. O P&L do próprio arquivo é conferido contra o calculado.
- **Formato da linha.** Uma linha é um **round trip** (uma linha = um trade) ou uma **execução** (uma linha = uma execução). As execuções são agrupadas por instrumento em posições com pernas de entrada e saída; o que ainda estiver aberto no final é importado como um trade aberto.
- **Extratos empilhados.** Uma exportação que reúne várias tabelas em um arquivo (o estilo IBKR) é lida seção por seção, com um seletor para trocar de tabela ou ler o arquivo como plano.
- **Valor do ponto.** Para cada ticker encontrado no arquivo, a importação pede o valor do ponto do contrato, já que nenhuma exportação o traz. Ele é salvo com o mapeamento.
- **Mapeamentos.** Salve um mapeamento e o próximo arquivo da mesma fonte é reconhecido pela impressão digital do seu cabeçalho e se mapeia sozinho. As colunas que você corrige à mão também são lembradas.

::: tip Um mapeamento não é um modelo
Um **modelo** do journal é o formulário com que você registra um trade à mão. Um **mapeamento** de importação diz qual coluna de um arquivo externo é qual campo de trade. Eles são listados separadamente e nunca misturados.
:::

### Puxar de um broker

Com uma [conta de broker](/pt/config/brokers) concedida ao journal, **Puxar de um broker** faz a mesma importação sem arquivo: escolha a conta e um período, e as execuções voltam dobradas em posições, com prévia antes de qualquer gravação. Não há mapeamento para conferir, uma API responde campos tipados. Rodar de novo um período mais amplo atualiza as posições já importadas em vez de duplicá-las.

**Desfazendo uma importação.** Toda importação é um **lote**, listado com sua data, arquivo e contagem de trades. *Reverter* exclui exatamente os trades que ele criou. As importações também são desduplicadas **por categoria**, então reimportar o mesmo arquivo não muda nada (o mesmo extrato ainda pode alimentar duas categorias, já que uma categoria é um book). *Esquecer* um lote derruba essa proteção e torna seus trades comuns de novo.

## Exportação e relatórios

Na aba Trades você pode exportar seus dados e gerar um relatório de desempenho:

- **Exportação CSV**: os trades brutos, para planilhas ou software de imposto.
- **Relatório periódico**: um resumo de desempenho semanal ou mensal (win rate, expectancy, taxas, repartição por estratégia e categoria, curva de patrimônio), renderizado em **Markdown ou PDF**.

## Funciona com

- **Tax Calculator**: carrega o PnL realizado do journal para um ano fiscal, dividido em ganhos de capital / derivativos / cripto.
- **Historical Data**: as abas Dados de mercado e Risco aberto leem candles pelos [data connectors](/pt/config/connectors) concedidos ao journal, e baixam o que falta como jobs comuns.
- **Trading Routines**: o calendário PnL mostra, por dia operado, se as rotinas que a categoria segue foram marcadas.
- **Dashboard**: um widget de quick-trade registra um trade a partir da página inicial.
- **RemindMe**: adicione lembretes ligados ao journal (por exemplo, revisão semanal).
