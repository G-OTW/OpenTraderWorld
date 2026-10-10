# Dados de mercado e backtesting

Estes módulos formam uma cadeia: o **Historical Data** baixa histórico de preços para conjuntos de dados locais; o **Backtest** e o **Quant Tools** trabalham sobre esses conjuntos de dados, e o **Visualization** plota qualquer instrumento que um connector sirva, armazenado ou não. Os três exigem o Historical Data instalado, já que ele é dono do catálogo de conjuntos de dados que eles leem.

## Historical Data {#histdata}

Baixe candles OHLCV de provedores externos para conjuntos de dados armazenados no seu banco, ou [importe um arquivo](#import) que você já tem. Uma vez armazenados, os dados são seus: plote-os, faça backtest, exporte, sem buscar de novo.

### Provedores e credenciais

Os provedores são configurados uma vez, de forma central, como **[data connectors](/pt/config/connectors)**: uma conta de provedor nomeada com suas credenciais, um limite de requisições opcional e os módulos autorizados a usá-la. O Historical Data **não tem configurações de provedor próprias**: o botão *Connectors* ao lado do seletor de provedor abre a mesma tela compartilhada que você encontraria nas Configurações.

Alguns provedores são **sem chave** (Binance e Binance futures, Bitget, OKX, Kraken, Coinbase, Yahoo Finance) e funcionam de imediato; outros precisam de uma chave de API, e a maioria tem planos gratuitos. Um connector sem suas credenciais mostra *precisa de credenciais* e é ignorado até você defini-las.

As chamadas de saída são contadas em **Configurações → Taxa da API** para você acompanhar o uso do plano gratuito.

### Baixando

Escolha provedor, tipo de ativo, timeframe, ticker e intervalo de datas, depois **Baixar**. Notas:

- **Futuros** usam códigos de contrato: base + letra do mês + dígito do ano (`F G H J K M N Q U V X Z` = jan…dez), por exemplo `GCJ5` para ouro de abril de 2025.
- **Opções** são montadas a partir do subjacente, vencimento, call/put e strike.
- **Limites intradiários**: os provedores só servem granularidade intradiária por um período limitado (por exemplo ~7, 60 ou 730 dias dependendo do provedor). O histórico mais antigo está disponível em **1d / 1w** sem limite. O formulário avisa antes de você enfileirar um intervalo impossível.

::: warning Conjuntos de dados baixados antes da v0.0.15
Antes da v0.0.15, um download ou uma atualização que chegasse ao momento atual podia armazenar o candle ainda em formação, e as atualizações seguintes começavam depois dele. Um conjunto de dados assim pode conter **candles incompletos** (máxima, mínima, fechamento e volume cortados). Baixe-o de novo para substituí-los. Desde a v0.0.15, um candle só é armazenado quando seu período terminou.

| Provedor | Conjuntos de dados que podem conter candles incompletos |
|---|---|
| Coinbase, Kraken, Yahoo Finance, Alpaca, Massive, EODHD, Alpha Vantage, Capital.com, Interactive Brokers | baixados ou atualizados antes da v0.0.15 |
| Binance (spot) | baixados ou atualizados antes da v0.0.12 |
| Binance USDⓈ-M, Bitget, OKX, OANDA, TradeStation, FOREX.com | nenhum |
:::

### Vários de uma vez

Um formulário enfileira um lote inteiro: marque **quantos timeframes** precisar e digite **vários tickers separados por vírgula** (`BTCUSDT, ETHUSDT, SOLUSDT`). Um download é enfileirado por par símbolo × timeframe (3 símbolos × 2 timeframes = 6 downloads), todos no mesmo connector e no mesmo intervalo de datas.

Antes de você pressionar Baixar, o formulário **precifica o lote**: quantos downloads são, aproximadamente quantas requisições ao provedor isso custa, e o tempo mínimo que levará (eles rodam um após o outro para respeitar os limites de taxa do provedor). Quando o connector tem um [limite de requisições](/pt/config/connectors#limites-de-requisicoes), um pequeno medidor mostra quanto da janela atual já foi gasto, e a linha avisa quando o lote passa dele. Ele não é bloqueado: o restante espera a cota reiniciar e retoma sozinho.

### Acompanhando os jobs

Os downloads rodam como **jobs** em segundo plano, pedaço por pedaço, com progresso ao vivo. Filtre jobs por status, provedor, timeframe ou ticker; um lote é agrupado sob um cabeçalho mostrando quantos de seus downloads terminaram.

- **Um job que atingiu um limite está `waiting`, não falhou.** A linha diz por quê (*cota atingida* ou *limite de taxa do provedor*) e faz a contagem regressiva até o momento em que retoma sozinho.
- **Cancelar** interrompe qualquer job não terminado, e um clique cancela **o restante de um lote**. O cancelamento é cooperativo: o worker para no próximo limite de pedaço e as barras já gravadas são mantidas.
- Uma parada longa e o fim de um lote geram uma notificação, enviada aos [canais](/pt/config/settings#notifications) concedidos ao Historical Data.

### Conjuntos de dados

A aba **Conjuntos de dados** lista tudo o que está armazenado: contagem de barras, intervalo de datas, tamanho. Daqui você pode:

- **Buscar mais novos**: puxar barras mais novas que a última armazenada (completar um conjunto de dados).
- **Exportar** como **CSV** ou **Parquet**. O CSV abre em qualquer planilha; o Parquet são as mesmas barras tipadas e comprimidas, mais ou menos um décimo do tamanho, lidas por `pd.read_parquet` sem interpretação de datas nem adivinhação de dtypes. O arquivo Parquet também carrega o instrumento, o timeframe e a fonte nos próprios metadados, então importá-lo de volta em qualquer lugar do app preenche o formulário sozinho.
- **Excluir** um conjunto de dados (descarta todas as suas barras).
- Ir direto para um **gráfico** dele.

Os downloads da Capital.com e da OANDA também armazenam o **bid e o ask** de cada candle; para os outros provedores um backtest os busca quando [precisa deles](#bid-ask-providers).

Os conjuntos de dados importados ficam na mesma lista, com o nome de onde veio o arquivo e não de um provedor. Eles não têm botão *Buscar mais novos*: não há provedor por trás, e a forma de estender um é outro arquivo.

### Importando seu próprio arquivo {#import}

**Importar** na aba Conjuntos de dados lê um histórico de preços que você já tem: um dump de exchange, uma exportação de broker, uma planilha, o arquivo de um fornecedor. CSV, TSV, TXT, JSON ou **Parquet**, até 20 MB, uma linha por barra.

O arquivo nunca sai do seu navegador entre as etapas e nunca é armazenado no servidor: cada etapa o envia de novo, então não há upload pela metade para retomar ou limpar.

**As colunas são propostas, você as confirma.** Os cabeçalhos são comparados com um dicionário multilíngue (seis idiomas) *e* com o aspecto real dos valores, então `Date;Ouverture;Plus haut;…` e `open_time,open,high,low,close,volume` caem mapeados. Um ponto ao lado de cada coluna diz o quão seguro o detector está; o que ele não tem certeza fica para você. Corrigir uma coluna o ensina: o próximo arquivo com esse cabeçalho se mapeia sozinho.

Um arquivo Parquet é lido na mesma grade de um CSV, então a detecção de colunas, a etapa de mapeamento e a prévia funcionam de forma idêntica. Seus tipos são respeitados: um timestamp INT96 legado (o que Spark e pandas antigos escrevem) e um `DATE` viram datas, um `DECIMAL` mantém sua escala, um nulo continua sendo uma célula vazia. O mesmo leitor serve as importações do Journal e do Portfolio, que também aceitam Parquet.

Os **timestamps** são lidos como datas ou como inteiros Unix em segundos, milissegundos, microssegundos ou nanossegundos, detectados por coluna e substituíveis. Um timestamp de texto que não carrega fuso horário é lido no offset que você escolher, o que decide *a qual período* cada linha pertence, e não apenas como ela é exibida.

**O que falta é preenchido, nunca inventado.** Um arquivo com uma única coluna de preço é uma série de fechamento (um NAV, um nível de índice): abertura, máxima e mínima são preenchidas a partir do fechamento, formando uma barra plana, e o formulário diz isso. Uma coluna que você *mapeou* e que está vazia em uma linha é um erro que nomeia a linha, não um zero.

Antes de qualquer coisa ser gravada, a prévia informa sobre o arquivo inteiro: barras, linhas, colunas, a primeira e a última data, o espaçamento que seus timestamps realmente têm (oferecido como timeframe), períodos ausentes nesse espaçamento, linhas que compartilham um período, barras cuja máxima/mínima não contém a abertura/fechamento, e cada linha que não pôde ser lida.

**O que o arquivo não consegue dizer, você diz.** Um arquivo diz "Close"; ele não diz que as barras são AAPL diário. Então a importação pergunta:

- **Ticker**, **tipo de ativo** e **timeframe** (o timeframe é preenchido a partir do espaçamento do próprio arquivo).
- **Fonte**: o broker, venue ou fornecedor de onde o arquivo veio, texto livre. Ela é *parte da identidade da série*, então o mesmo instrumento exportado por dois brokers continua sendo dois conjuntos de dados em vez de duas fitas tiradas em média em uma só.
- **Nome** e **tags**: seus próprios rótulos, usados para encontrar a série de novo no catálogo e filtrá-la.

**Importar duas vezes é seguro.** Uma reimportação cai no mesmo conjunto de dados e sobrescreve período a período: mesmo arquivo, mesmo resultado. Os períodos sobrepostos são contados na prévia antes de você confirmar.

Um arquivo Parquet exportado daqui pula a maior parte desse formulário: ele já conhece seu ticker, tipo de ativo, timeframe e fonte, e só preenche as caixas que você deixou vazias, então o que você digitou ainda vence. Exporte, edite no pandas, importe de volta.

Uma vez importada, a série é um conjunto de dados comum: backtests, Quant Tools, enriquecimento do journal e os proxies de fator da carteira a leem como qualquer uma baixada.

### Olhe antes de baixar

Você não precisa enfileirar um job para descobrir se um símbolo vale a pena armazenar. O gráfico busca uma janela por um connector e **não armazena nada**; quando a janela parece certa, **salve-a** e o job de download normal é enfileirado para exatamente esse intervalo. Salvar sobre barras que você já tem consolida em vez de duplicar, então completar um conjunto de dados a partir do gráfico é seguro.

## Historical Data Visualization {#histviz}

O gráfico não está preso a um conjunto de dados: ele abre um **instrumento**. Busque um símbolo, escolha um timeframe, e as barras chegam, você as tendo baixado ou não: o servidor serve o que já está no seu catálogo e busca apenas as pontas faltantes por um [connector](/pt/config/connectors). Nada é gravado a menos que você peça.

A página é um **workspace**: uma grade de gráficos, uma lista de instrumentos ao lado e uma sessão de quick-backtest embaixo. Tudo abaixo descreve um único gráfico, a menos que diga o contrário; a grade em si está em [Workspaces](#workspaces).

### Encontrando um instrumento

Nada é escrito sobre os candles que não pertença a eles: o **símbolo no canto superior esquerdo de um gráfico é um botão**, e ele abre o seletor de instrumentos como um modal, uma caixa de busca sobre todo connector que o gráfico pode usar. Digite `BTC` e Binance, Bitget, OKX, Kraken, Coinbase, Yahoo, EODHD, Alpha Vantage, Alpaca e Massive respondem juntos, cada resultado rotulado com o connector que o serviu. Filtre por tipo de ativo; marque ou desmarque fontes no mesmo modal (a marca **é** a concessão, e o servidor recusa um connector que este módulo nunca recebeu).

Com a caixa vazia, o painel lista o que você plotou **recentemente**, depois o que já está **armazenado**, ambos abrindo com um clique e sem custo.

Uma linha que você não consegue plotar diz isso no lugar do gráfico, nomeando o motivo: o connector nunca foi concedido, o provedor não conhece o símbolo, falta uma credencial, ou nenhum connector concedido o serve. Cada mensagem traz o botão que resolve.

### Workspaces {#workspaces}

Um workspace é uma **grade de gráficos**, de 1x1 até 3x4. Linhas e colunas são escolhidas na barra de ferramentas, então uma divisão vertical, uma horizontal e uma 2x2 são o mesmo controle e não uma lista de layouts nomeados; arraste os divisores para dar mais espaço a um gráfico, ou maximize um gráfico e volte à grade. Mantenha quantos workspaces quiser, nomeie-os e alterne pelo seletor; o que você tinha aberto volta ao recarregar.

Cada gráfico carrega seu próprio instrumento, timeframe, estilo de plotagem, indicadores e desenhos. Um gráfico é fechado pelo próprio cabeçalho, e uma célula vazia pede um instrumento.

**Grupos de vínculo.** Clique no botão de vínculo de um gráfico para dar a ele uma cor. Gráficos que compartilham uma cor compartilham o **símbolo** e o **crosshair**, e o intervalo visível também quando estão no mesmo timeframe. O timeframe em si nunca é compartilhado de propósito: três painéis no mesmo símbolo em 1m, 1h e 1d é o motivo de vinculá-los.

**Uma conexão para todos.** Painéis na mesma conta compartilham um único stream ao vivo, então quatro gráficos em uma chave Alpaca gastam uma vaga de conexão, não quatro.

### A lista de instrumentos {#rail}

Uma barra à esquerda da página, de duas fontes que nunca se misturam:

- **Listas de gráficos** são da própria barra, montadas com o botão `+`, que abre o mesmo seletor de instrumentos. Elas guardam coordenadas de gráfico, então um clique as plota sem busca. **Recentes** é a mesma coisa sem nome.
- **As listas do módulo Watchlists** guardam símbolos de cotação, então plotar um é uma busca. Quando a lista ou a linha cota por um data connector, só esse connector é consultado: um símbolo cotado pela IBKR plota na IBKR ou em lugar nenhum.

Clique em uma linha para plotá-la no painel ativo, ou arraste-a para qualquer painel. **Uma watchlist nunca é escrita a partir daqui**: editar uma a copia primeiro para uma lista de gráficos e edita a cópia, e transformar uma lista de gráficos em uma watchlist de verdade é o botão **Promover** e nada mais.

### Carregando histórico

O gráfico abre nas **1500 barras** mais recentes e coloca um botão na borda esquerda dos dados carregados. Cada clique anda uma fatia para trás. Nenhuma requisição ao provedor acontece sem um gesto seu, o que mantém previsível uma chave com cobrança por uso; a caminhada para quando o histórico do provedor acaba.

O menu de **timeframe** oferece todo tamanho de barra que o connector suporta, não só os que você baixou, e trocar de timeframe **mantém as datas que você estava olhando**.

### Streaming ao vivo {#live}

Para um connector capaz de stream, o gráfico **entra ao vivo sozinho** assim que a barra mais nova
é a que está se formando agora: o último candle é atualizado no lugar, e o controle mostra o estado
da conexão e o atraso em relação à exchange. O mesmo controle para e reinicia o feed à mão.

O ao vivo é endereçado por **instrumento**, não por conjunto de dados, e **não armazena nada**. Qualquer símbolo que um
provedor de streaming sirva pode ser observado ao vivo sem baixá-lo antes, e esse é o ponto:
você pode olhar algo antes de decidir se vale mantê-lo. Salve o instrumento enquanto ele está
ao vivo e o mesmo feed começa a gravar as barras fechadas também no conjunto de dados.

**Quem transmite o quê.** Binance (spot e futuros), Bitget, OKX, Kraken e Coinbase transmitem todo
timeframe que baixam, o diário incluído, porque num mercado 24/7 o candle diário é o dia
epoch. A Capital.com também transmite todo timeframe, a partir dos candles de bid e ask que publica
separadamente, plotados no ponto médio. Alpaca, Massive e
Interactive Brokers publicam **uma granularidade cada** (barras de um minuto, agregados de um minuto,
barras de cinco segundos) e o gráfico dobra o seu timeframe a partir dela. Isso cobre todo timeframe
**intradiário** e deixa diário e semanal para o download: uma sessão de ações não tem 1440
minutos alinhados ao epoch, então um candle diário construído assim discordaria do armazenado. Em
um timeframe que não pode transmitir, o controle diz quais podem em vez de sumir. O
detalhe por provedor está em [streaming ao vivo](/pt/config/connectors#streaming-ao-vivo).

Um gráfico transmite um símbolo qualquer que seja o número de painéis: vários painéis em um instrumento, e
vários painéis em uma conta, compartilham a conexão em vez de cada um ocupar uma vaga.

**Qual conta.** Quando um provedor tem mais de um connector, o controle ao vivo ganha um seletor
de conta. Duas chaves são dois direitos e duas vagas de conexão, então qual é gasta
é decisão sua, não um fallback. Trocar reinicia o feed na outra.

#### Sem lacuna na emenda

Carregar a janela, abrir o socket e esperar o provedor publicar levam tempo,
e um provedor de barras de minuto só fala uma vez por minuto. Quando o primeiro tick ao vivo chega, o
gráfico pode estar um ou vários candles atrasado, e esses candles ficavam faltando até você
recarregar.

Então o stream diz ao servidor onde o gráfico termina, e o servidor envia o que fechou nesse
meio-tempo: do catálogo quando o instrumento está armazenado, do provedor apenas para a cauda que ele
não consegue responder, nada quando não há lacuna. O que você vê ao entrar ao vivo é o que o mercado
fez, sem buraco na junção.

#### Quando não consegue rodar

Uma chave rejeitada, um plano sem streaming, um instrumento para o qual sua conta não tem assinatura:
são respostas, não falhas. O gráfico nomeia qual delas é, cita as palavras do próprio provedor
e **para** em vez de reconectar para sempre atrás de um ponto que nunca fica verde.

| O que você vê | O que significa | O que fazer |
|---|---|---|
| *Não autorizado* / chave rejeitada | as credenciais estão erradas, ou o plano não tem feed ao vivo (uma chave Massive gratuita baixa histórico e é recusada no login ao vivo) | corrija o connector, depois pressione *Tentar de novo* |
| *Sem assinatura* para este símbolo | a conta está conectada mas sem direito ao feed desse instrumento (a Alpaca gratuita transmite IEX, não SIP nem OPRA; a IBKR serve o que você assina) | escolha outro instrumento ou adicione a assinatura no fornecedor |
| *Conexão ocupada* | a maioria dos fornecedores permite uma conexão ao vivo por conta, e outro programa está com a vaga | feche o outro programa; este continua tentando sozinho, já que se resolve por si |
| *Este timeframe não transmite* | o provedor publica uma única granularidade e o seu timeframe está acima dela | troque para um dos timeframes que o controle lista, ou fique nos dados baixados |
| *Connector não concedido* | o gráfico nunca recebeu este connector | conceda-o em [Data connectors](/pt/config/connectors), pelo link na mensagem |
| *Linhas de dados de mercado* quase todas usadas | a Interactive Brokers limita quantos símbolos uma conta transmite de uma vez, e o workspace está perto desse limite | feche um painel, ou pare o feed ao vivo de um que você não está olhando, antes que o próximo gráfico fique mudo sem motivo |

Todo o resto (um socket derrubado, um soluço do provedor) reconecta em silêncio com backoff.

### Gráfico

- **Tipos de gráfico**: candles, barras OHLC, linha e **Renko** (com tamanho do tijolo).
- **Indicadores**: SMA, RSI, MACD e mais, como overlays ou painéis separados, cada um com fonte, cores de linha/preenchimento e espessura configuráveis. Cada série ganha **sua própria linha no cabeçalho do gráfico**, com ocultar, configurações e remover ao passar o mouse, e cada painel é titulado sobre o desenho que contém. O cabeçalho **lê no crosshair**: O H L C, a variação, e o valor de cada indicador na barra sob o cursor, voltando à barra visível mais nova quando o cursor está fora.
- **Configurações do gráfico**: escala linear ou logarítmica, linhas de grade horizontais e verticais, **separadores de dia**, o **fechamento anterior** desenhado como linha de referência, crosshair e suas etiquetas de valor por série, tooltip ao passar o mouse (desligado por padrão), cores de alta/baixa, escala de preço à esquerda ou à direita, e uma **etiqueta do último preço** fixada no eixo de preço, colorida como o candle que a produziu.
- **A navegação é manual**: arraste para deslocar os dois eixos (tempo para os lados, preço para cima e para baixo), roda para zoom (calibrada por dispositivo, então um clique de mouse e um tick de trackpad movem a mesma quantidade). Mudar o tipo de gráfico mantém o zoom atual.
- O **volume** é desenhado na base do painel de preço, do jeito que os terminais de mercado o desenham, em vez de em um painel próprio. Mais um único modo de tela cheia.
- **Preços de micro-cap** são escritos `0.0₅4549`, com o subscrito contando os zeros, em vez de uma escala de rótulos `0.0000` idênticos.

### Comparando dois instrumentos

Adicione outro instrumento ao mesmo gráfico e ele é desenhado **rebaseado**, já que dois preços em duas moedas num eixo não dizem nada. Duas leituras, a um clique de distância:

- **variação percentual**, as duas séries rebaseadas para o início da janela, que responde *qual subiu mais*;
- **razão**, este instrumento dividido pelo outro, rebaseado para 100, que é a visão de pair trade: a linha sobe quando o que você está plotando supera o outro.

As séries de comparação são alinhadas ao gráfico **período a período**, então dois mercados que carimbam o mesmo dia de forma diferente ainda se alinham.

### Salvando o gráfico

- **Como imagem**: um PNG do gráfico exatamente como está na tela, desenhos e overlays incluídos.
- **Como página**: um arquivo HTML que abre offline em qualquer navegador, contendo a imagem, *do que* ela é uma imagem (instrumento, janela, timeframe, indicadores, comparações, quem serviu as barras), e **as próprias barras**, embutidas. Uma captura de tela colada em um documento é uma afirmação que ninguém pode verificar depois; esta pode ser relida. Nada é enviado: o arquivo é montado no seu navegador.

### Indicadores personalizados no gráfico

O diálogo de indicadores tem uma segunda aba: **Personalizados**. Ela guarda a mesma biblioteca de grafos de nós que o módulo [Backtest](#estrategias-e-indicadores-personalizados) usa para montar, então um indicador existe **uma vez** e os dois módulos veem a mesma definição. Escolha um da lista para plotá-lo, ou monte um novo aqui mesmo com o mesmo construtor; salvar o grava de volta na biblioteca compartilhada.

Ao contrário de um indicador do catálogo, um personalizado **escolhe seu próprio painel**: no preço, ou em um painel próprio. Essa escolha é sua por instância, então o mesmo indicador pode sobrepor os candles em um gráfico e ficar abaixo deles em outro. Um indicador de 0-100 desenhado como overlay ganha uma segunda escala oculta, para não achatar o preço.

A definição **viaja com a instância**: um gráfico continua desenhando seu indicador personalizado depois que a linha da biblioteca é excluída, e recarregar o atualiza a partir da biblioteca enquanto a linha existir.

### Ferramentas de desenho

Uma barra na borda esquerda do gráfico: **linha de tendência**, linha **horizontal** e **vertical**, **retângulo**, **retração de Fibonacci**, **texto**, caixas de posição **long** e **short** (entrada, alvo e stop, com o R:R resultante) e uma **régua** informando a variação de preço, a porcentagem, o número de barras e o tempo decorrido.

- Cada objeto tem seu próprio **estilo** (cor, espessura, tracejado) e é editado arrastando suas alças.
- Um **ímã OHLC** prende uma alça à abertura, máxima, mínima ou fechamento da barra sob ela, e uma alça solta perto de um objeto já no gráfico se prende a ele, com uma linha-guia dizendo o que capturou.
- **Modelos de estilo**: estilize um objeto, salve-o com um nome e aplique aos próximos. Um modelo pode ser o padrão para todo novo desenho.
- **Copiar entre gráficos**: <kbd>Ctrl/⌘+C</kbd> depois <kbd>Ctrl/⌘+V</kbd> cola o objeto selecionado, em outro instrumento também; <kbd>Ctrl/⌘+D</kbd> o duplica no lugar, deslocado uma barra.
- Os desenhos são ancorados em **tempo e preço**, não em pixels, então ficam nas suas barras em qualquer zoom, deslocamento ou mudança de timeframe.
- Eles são mantidos **por instrumento**, não por timeframe nem por painel: uma linha de tendência desenhada no 1h é a mesma linha no 15m, e dois painéis no mesmo símbolo mostram um só quadro.
- **Desfazer** (o botão da barra, ou <kbd>Ctrl/⌘+Z</kbd>) desfaz o último desenho, ou a última ordem do quick-backtest.

#### A lista de objetos

Passando de cinco desenhos um gráfico precisa de uma lista, então há uma: todo objeto neste instrumento com o que ele é e o preço em que está, e as quatro coisas que ele então precisa, **ocultar**, **travar**, **excluir** e **reordenar**. A ordem é a ordem de pintura, que decide o que fica por cima. Selecionar uma linha a seleciona no gráfico, e a lista é o mesmo array que o gráfico desenha, então uma edição aparece antes de o diálogo fechar.

### Alertas {#alerts}

Um preço ou um nível de indicador, **observado pelo servidor**. O navegador pode estar fechado, a máquina pode estar fazendo outra coisa: o alerta dispara do mesmo jeito, nas suas notificações e nos [canais](/pt/config/settings#notifications) concedidos ao gráfico (nenhum marcado = todos eles).

Defina um pelo diálogo de alertas, ou a partir de uma linha horizontal que você já desenhou, que entrega o seu preço.

Três regras merecem ser conhecidas, porque são decisões e não detalhes:

- **Somente barras fechadas.** A máxima de um candle em formação ainda não é um fato, pode ser revisada pelo próximo tick. Um alerta que disparasse nela estaria informando algo que nunca aconteceu.
- **Um cruzamento, não um estado.** *Cruza para cima* espera o preço **atravessar** o nível subindo, então um alerta colocado abaixo do preço atual não dispara no instante em que você o cria.
- **O nível é lido no timeframe deste gráfico**, e um alerta diário é relido muito menos vezes que um de um minuto: um instrumento não armazenado custa uma pequena requisição por verificação, e uma barra que se move uma vez por dia não merece uma por minuto.

Cada alerta pode disparar **uma vez** ou toda vez, com um cooldown. A lista diz quando cada um disparou pela última vez, o que ele leu por último e, quando um connector ou uma cota atrapalha, por que não pôde rodar. Os alertas são pausados e armados de novo a partir dessa lista.

### Book do broker {#broker-book}

Sincronize uma [conta de broker](/pt/config/brokers) e o gráfico desenha o que você realmente tem: uma linha de preço por posição aberta no seu custo médio, uma por ordem em andamento no seu limit ou stop. Os níveis caem no gráfico cujo ticker corresponde, ignorando pontuação, então um workspace de vários instrumentos se anota sozinho. Somente leitura, e relido apenas quando você pressiona *Sincronizar*: uma posição sem custo médio não recebe linha e é contada como tal em vez de ser colocada em algum lugar plausível.

### Quick backtest

Um bloco de rascunho para operar um gráfico à mão: **clique no gráfico para abrir uma posição, clique de novo para fechá-la**. Em vez disso, pressione e segure para escolher o lado e o tamanho, e clique na seta de um marcador para invertê-lo. Pirâmide, fechamentos parciais e reversões decorrem todos das execuções que você colocar.

A sessão abrange **o workspace inteiro, não um gráfico**: cada gráfico na tela lança suas execuções nela e os números são a soma, do jeito que um book de vários instrumentos realmente se lê. O seletor no painel nomeia o **gráfico alvo**, aquele em que um clique coloca um trade e que a caixa de tamanho edita; é o painel ativo, então escolher aqui e clicar ali são o mesmo ato.

A faixa sob o workspace é uma linha de números da sessão quando recolhida. Expandida, ela é redimensionável pela borda superior e tem três abas:

- **Trades**: a lista de trades da sessão com entrada, saída, P&L e R (medido contra a pior perda aberta do trade, já que não há stop para citar), mais um reset.
- **Estatísticas**: win rate, expectancy, profit factor, resultados por lado, sequências e uma distribuição de retornos.
- **Desempenho**: a curva de P&L da sessão, cada trade carregando seu run-up e sua pior perda aberta, então um ganhador que passou o dia no vermelho se lê como tal.

O tamanho é informado em **unidades**, **contratos** (multiplicados por um valor do ponto) ou **nocional**, e convertido ao preço de execução. Essas execuções vivem **no seu navegador, por instrumento**: é um bloco de rascunho para ler um gráfico, nunca dados do journal, e nada é enviado ao [Trading Journal](/pt/modules/journal).

O botão **Backtest** entrega o mesmo instrumento ao módulo [Backtest](#backtest), salvando-o antes se não estava armazenado.

### O gráfico lembra onde você parou

Tipo de gráfico, indicadores e desenhos pertencem ao **instrumento**, não a um conjunto de dados nem a um painel: eles são salvos no servidor sob as coordenadas do próprio símbolo logo após cada edição, e voltam do mesmo jeito em qualquer painel, em qualquer workspace, de qualquer navegador. Isso vale para um símbolo que você olhou uma vez e nunca baixou, e esse é o ponto: plotar algo que você não decidiu manter não significa mais perder o que desenhou nele.

O que *não* é armazenado no servidor é a sessão de quick-backtest, que fica neste navegador.

O interruptor **salvar dados automaticamente** nas configurações do gráfico é uma decisão à parte, sobre as barras e não sobre o layout: ele armazena um instrumento na primeira vez que você o plota, o que enfileira um download. Vem desligado por padrão.

### Quando faltam dados

Uma janela que volta curta sempre diz **por quê**, num aviso acima do gráfico, mantendo as barras que chegaram:

| Motivo | O que aconteceu |
|---|---|
| **auth** | A credencial do connector está ausente ou foi rejeitada. |
| **quota** | O connector atingiu seu próprio [limite de requisições](/pt/config/connectors#limites-de-requisicoes). |
| **rate_limit** | O provedor limitou a requisição. |
| **symbol** | O provedor não conhece este ticker. |
| **depth** | O provedor não serve histórico tão antigo neste timeframe. |
| **provider** | Qualquer outra coisa que o provedor retornou. |

## Backtest {#backtest}

*Combine sinais de indicadores, dimensione com pirâmide, meça a vantagem.* Escolha um conjunto de dados ou uma carteira inteira, defina regras, execute. Sem código.

### Estratégia

- **Regras de entrada / saída** por lado, montadas a partir de comparações entre indicadores, preço e valores fixos. Agrupe regras com **AND** (todas devem valer) ou **OR** (qualquer uma basta).
- **Direção**: long, short ou ambos. Opções: derivar o lado short como espelho do long (operadores inversos, níveis de osciladores espelhados: RSI abaixo de 30 vira RSI acima de 70; filtros de ADX, ATR e volume mantidos como estão), e **stop & reverse** (inverter a posição quando o sinal oposto dispara).
- **Stop-loss / take-profit** por lado: cada um é uma caixa que você liga ou desliga de forma independente (porcentagem da entrada média, ou um múltiplo do ATR do último candle fechado antes da entrada). Sem regras de saída, as saídas acontecem por SL/TP ou reversão.

### Dimensionamento, conta e custos

- Dimensione por **porcentagem do patrimônio** ou **quantidade/lotes/contratos fixos**, com **alavancagem** e **capital inicial**. A quantidade fixa **escala com a alavancagem**, seguindo a convenção do varejo, então uma alavancagem de 3 com tamanho fixo de 1 abre 3 unidades.
- **Pirâmide**: permite até N entradas empilhadas quando o sinal de entrada dispara de novo; SL/TP então acompanham o preço médio de entrada. Um aumento é enviado na abertura, antes de o candle ser testado: um candle que então atinge o stop fecha a posição que o aumento fez, e um stop que o aumento moveu (breakeven) é testado nesse mesmo candle.
- **Custos**: taxa (fixa ou % do nocional, por trade ou por unidade) e **spread %**, para os resultados não serem fantasia. Uma taxa pode ser negativa, para um rebate de maker ou um broker que paga por execução. A taxa de entrada sai do caixa na execução, como um broker a debita, então o patrimônio, o drawdown e os aumentos de uma posição aberta são líquidos dela.
- Configurações sem sentido (sem capital, um tamanho ou alavancagem zero ou abaixo, um contrato que não vale nada, uma grade que não pode ser operada) são recusadas antes da execução, e também um conjunto de dados com um candle que não é um (uma máxima abaixo da mínima, um preço que não é número), com o candle nomeado.

### Dimensionamento (avançado)

Além de porcentagem do patrimônio e quantidade fixa:

- **Risco por trade**: dimensione para que um stop-loss atingido custe uma % fixa do patrimônio (precisa de um stop no lado operado).
- **Kelly fracionário**: dimensione a partir do win rate e do payoff dos últimos *N* trades de sinal da estratégia, os ignorados incluídos (um trade de breakeven não é ganho nem perda), escalado pela fração escolhida e limitado; um tamanho de aquecimento é usado até a janela encher. Uma fase de perdas pausa as entradas sem pará-las de vez: elas retomam quando a vantagem volta.
- **Faixas de patrimônio**: uma tabela de limiares; a maior faixa cujo nível é ≤ o patrimônio atual define o tamanho.

### Portfolio (multiativo)

Adicione vários conjuntos de dados e execute uma estratégia em todos eles sobre um **relógio mesclado** (todos travados no mesmo timeframe):

- Uma **prévia de alinhamento** mostra o comprimento do relógio mesclado, a janela de sobreposição, as barras de aquecimento dos indicadores (incluindo o lookback cumulativo de um indicador personalizado encadeado) e as barras faltantes por ativo, tudo antes de você simular.
- **Limites da carteira**: limite o número de posições abertas e a exposição total / por ativo. Uma posição mantida em uma abertura conserva seu lugar ali mesmo se fechar depois no mesmo candle.
- **Sessões**: ativos cujos candles abrem em horas diferentes (um dia cripto às 00:00 UTC, um de Nova York às 13:30) agem nessa ordem. Uma ordem na abertura anterior dimensiona e verifica seus limites sobre o fechamento anterior do ativo posterior, não sobre uma abertura que ainda não aconteceu.
- Um **detalhamento por ativo** informa trades, PnL líquido, taxas, win rate e exposição de cada instrumento.

### Estratégia de grade

Uma escada de níveis de preço entre um limite inferior e um superior; cada célula compra baixo e vende no próximo nível acima, **long**, **short** ou **neutra**. Dimensione uma quantidade fixa por nível ou divida um orçamento total entre as células, com stops opcionais acima/abaixo da escada. Os resultados informam execuções, round trips e inventário final.

- **Neutra** opera os dois lados da linha central: as células abaixo dela compram e vendem um nível acima, as células acima vendem short e recompram um nível abaixo. Ela precisa de um número ímpar de níveis.
- Uma compra repousa apenas em uma linha **abaixo** do último fechamento (uma venda acima dele), e é executada na linha, ou na abertura quando o candle abre além dela. Os alvos são executados do mesmo jeito.
- Um **stop** abaixo ou acima da escada é executado no seu nível (na abertura em um gap), depois das execuções que o preço encontrou no caminho até ele, e então interrompe a grade.
- Sem limites, a escada abrange o intervalo conhecido até agora: a mínima mais baixa e a máxima mais alta dos candles anteriores a cada um.
- Fora da janela de trading com *fechar*, o inventário sai na abertura, antes de qualquer outra coisa no candle.

### DCA (plano de poupança)

Um terceiro modo ao lado das regras de sinal e da grade, para a forma como a maior parte do dinheiro é de fato investida: uma **cesta ponderada**, comprada ao longo do tempo, nunca rebalanceada.

- **Pesos, fixos.** Cada euro aplicado é dividido pelos pesos que você define por ticker. Nada é rebalanceado, então uma regra que dispara em um ativo de cinco aplica a parcela própria desse ativo.
- **Dinheiro entrando.** O capital inicial é comprado de uma vez na primeira barra de cada ativo. Tudo depois disso é **dinheiro novo**: um aporte recorrente (por barra, dia, semana, mês, trimestre ou ano, investido na chegada ou mantido em caixa), e regras de compra que pagam um valor quando sua condição vale.
- **Regras de compra**: um valor fixo, uma % do caixa, da carteira ou da base de custo, com um número máximo de disparos e um cooldown, executadas na abertura da próxima barra.
- **Regras de venda**: uma % da posição, a posição inteira, um número de unidades ou um valor, acionadas por um **objetivo de ganho**, uma condição, ou ambos, com o produto mantido em caixa ou retirado.
- **Condições** são os grupos de regras comuns do motor mais duas famílias escritas para este modo: **métricas de mercado** (queda desde a máxima, alta desde a mínima, variação em N barras, variação desde o início) e a **posição ao vivo** (P&L %, desvio desde a última compra, custo médio, unidades, valor, peso %, caixa %, drawdown). Elas são avaliadas por padrão sobre um **índice de cesta ponderada**, ou por ativo, que então compra apenas os ativos que valem.
- **Medidas para um plano de poupança**, não para uma estratégia: drawdown e Sharpe sobre a curva **ajustada por depósitos (ponderada pelo tempo)**, para um depósito não ser lido como alta; retorno sobre o dinheiro aportado; **IRR** para o retorno ponderado pelo dinheiro; e um benchmark do mesmo total aportado aplicado de uma vez no início.

Dimensionamento, pirâmide e stops não se aplicam aqui: as regras do próprio plano decidem cada execução.

### Janela de trading

Uma etapa de **Filtros** decide *quando* a estratégia pode abrir, no relógio que você escolher: um fuso horário nomeado (`America/New_York`), que segue o horário de verão, ou um offset UTC fixo, que não segue:

- **Dias da semana** e **sessões** (várias por dia, um fim antes do início passa da meia-noite).
- **Calendário**: opere apenas em datas dadas, ou nunca nelas. *Nunca* vence *apenas*.
- Fora da janela a posição é **mantida** ou **fechada**, e os aumentos de pirâmide também podem ser bloqueados. As entradas são controladas; saídas, stops e take-profits continuam rodando em toda barra.

### Custos e realismo de execução

- **Slippage**: um número fixo de ticks ou uma porcentagem do preço, aplicado a toda execução.
- **Funding**: uma taxa anual constante sobre o nocional aberto para estimativas de perp (longs pagam, shorts recebem).
- **Circuit breakers**: interrompe o trading após uma perda diária máxima (pelo dia) ou um drawdown máximo (pela execução).
- **Perfil do instrumento**: tick de preço, passo de lote, quantidade mínima e multiplicador do contrato, para que tamanhos e preços se ajustem a um contrato realista. Toda execução e todo stop, alvo, limit e linha de grade é arredondado para o tick, contra o trader: uma compra paga o tick acima, o stop de um long fica um tick abaixo.

### Execução de ordens {#execution}

Como stops e alvos são executados, sempre:

- Um trade é testado contra seu stop e take profit **no candle em que abre**, não a partir do próximo.
- O **take profit é uma ordem limit**: é executado no alvo, sem slippage e sem spread cobrado, assim que o lado que negocia o alcança (o bid para um long, o ask para um short), ou apenas quando o preço **atravessa** o alvo, se você escolher isso (Avançado, *Execução*). Um candle que **abre além do alvo** o executa nessa abertura, antes de qualquer outra coisa no candle.
- O **stop loss é uma ordem stop**: é executado no stop, ou na abertura quando o candle dá um gap além dele, e paga spread e slippage.
- Quando um candle alcança **tanto** o stop quanto o alvo e nada mais diz qual veio primeiro, o stop vence.
- Um trade fechado dentro de um candle (stop, alvo, limit) não é reaberto na abertura desse candle, um preço anterior à saída: uma nova entrada espera o próximo candle.
- Todo sinal decidido em um fechamento é acionado na **próxima abertura**: uma entrada, a condição de saída e *sair quando a entrada deixa de valer*. Nada é executado no fechamento que produziu a própria decisão.
- MAE e MFE contam apenas o que o trade viveu: da execução até a saída, nunca o restante do candle depois de ele sair.

Opções da etapa **Avançado** (*Execução*), todas desligadas por padrão:

- **Ordem de entrada**: market, ou **limit**. Um limit repousa abaixo da referência para comprar e acima dela para vender, a um offset (porcentagem, distância de preço ou múltiplo de ATR) do fechamento do candle do sinal ou da abertura do próximo candle. Ele continua válido pelo número de candles que você definir, é executado ao toque ou apenas quando o preço o atravessa, e é executado ao seu próprio preço sem slippage (na abertura quando um candle abre além dele). Um sinal que continua valendo não move uma ordem em repouso. Os aumentos de pirâmide seguem a mesma regra.
- **Ordem de saída**: a mesma escolha para saídas por sinal (a condição de saída, e *sair quando a entrada deixa de valer*). Um limit de saída que não é executado a tempo **vira market** na próxima abertura ou é **cancelado**. Um stop-and-reverse continua sendo uma inversão a mercado.
- **Verificar timeframe menor para SL/TP**: quando um candle alcança tanto o stop quanto o alvo, ou um limit é executado no meio do candle, a execução lê um timeframe menor do mesmo instrumento (mesmo provedor) dentro *apenas desse candle* para ver o que veio primeiro. Ela lê primeiro um conjunto de dados armazenado nesse timeframe, depois candles baixados por uma execução anterior, que são mantidos para as execuções seguintes. *Auto* usa o timeframe armazenado mais fino, caso contrário o mais fino que o provedor serve em uma requisição por candle. Candles de timeframe menor que não correspondem ao candle (extremos diferentes) não são usados.
- **Baixar candles faltantes de timeframe menor**: com esta opção, os candles de que uma execução precisa e não tem são baixados do provedor, e apenas esses. Depois de uma execução, um aviso diz quantos candles foram resolvidos como stop por falta deles, com o custo (requisições e tempo) e uma caixa para buscá-los. Um download de até cerca de 4 minutos começa sozinho numa janela de progresso; um mais longo espera por você, com o limite de taxa do provedor e a cota restante no seu connector, já que pode falhar quando não sobra o bastante. A execução então é repetida com os candles baixados. Quando o provedor recusa (sem permissão ou assinatura para esses dados, uma chave, um limite de taxa) ou falha três vezes seguidas, a execução para de pedir a ele e diz isso ao lado dos resultados. Uma sessão paper com a opção baixa os candles sob o candle que acabou de fechar quando precisa deles, esperando um momento para o provedor publicá-los; sem a opção, ou quando eles nunca vêm, o stop vence.
- **Usar preços bid/ask**: as execuções leem o bid e o ask em vez do meio e do spread (uma compra a mercado paga o ask, o stop e o alvo de um long disparam no bid). Eles são buscados do provedor apenas para os candles em que uma execução pode acontecer, da forma mais fácil que ele oferece (veja a tabela abaixo), e mantidos para as execuções seguintes. Uma busca curta começa sozinha depois da execução, uma longa pergunta antes, com a mesma janela de progresso do timeframe menor. Um candle sem bid/ask usa o spread, e o resultado diz quantos usaram. Provedores sem bid/ask histórico deixam a opção desligada e dizem por quê.
- **Taxa de maker separada** (no bloco de Custos): execuções limit (limits de entrada e saída, take profit) pagam sua própria taxa, negativa para um rebate.
- **Preços brutos**: ações e ETFs são precificados em barras ajustadas por splits e dividendos quando o provedor as oferece (Yahoo, EODHD, Alpaca; as barras da Interactive Brokers vêm ajustadas por splits). Marque para rodar com os preços como negociados. Um conjunto de dados de ações sem série ajustada é sinalizado ao lado dos resultados.

O resultado mostra as ordens limit colocadas, executadas e expiradas, em quantos candles havia um stop e um alvo ao alcance e como foram resolvidos (timeframe menor ou pior caso), e quantas execuções foram precificadas em bid/ask.

#### Bid/ask e preço ao vivo por provedor {#bid-ask-providers}

| Provedor | Bid/ask histórico (backtest) | Preço ao vivo (stops, alvos, limits do paper) | Bid/ask ao vivo (execuções do paper) |
|---|---|---|---|
| Capital.com | Candles de bid e ask | Sim | Sim |
| OANDA | Candles de bid e ask | Não, verificado no fechamento do candle | Não usado |
| Interactive Brokers | Séries de candles de bid e ask | Sim | Sim |
| FOREX.com | Candles de bid e ask | Não, verificado no fechamento do candle | Não usado |
| Alpaca | Cotações na abertura e no fechamento do candle (ações, ETFs, cripto) | Sim | Sim |
| Massive | Cotações na abertura e no fechamento do candle (ações, ETFs, opções, forex) | Sim | Sim |
| Binance, Binance Futures, Kraken, OKX, Bitget, Coinbase | Nenhum, o spread se aplica | Sim | Sim |
| TradeStation, Yahoo, EODHD, Alpha Vantage | Nenhum, o spread se aplica | Não, verificado no fechamento do candle | Não usado |

Com cotações lidas na abertura e no fechamento, o spread dentro do candle é a média delas em torno da máxima e da mínima do próprio candle.

#### Paper trading no preço ao vivo {#paper-live}

Uma sessão paper lê seus sinais em candles fechados, no seu timeframe, e observa seus níveis no preço ao vivo no meio-tempo:

- O **stop loss, o take profit e as ordens limit** (entrada e saída) disparam no primeiro preço ao vivo que os alcança, sem esperar o candle fechar. O **trailing stop** ainda se move a cada fechamento, e o preço ao vivo o aciona no nível definido então.
- A execução é precificada no bid/ask ao vivo quando a estratégia usa bid/ask, caso contrário no preço ao vivo com o spread e o slippage das configurações. Ela é **alertada de imediato**, e a próxima execução a reproduz no seu candle como aconteceu, sem um segundo alerta.
- Um provedor sem preço ao vivo para o instrumento é nomeado no diálogo da sessão: ali, stops, alvos e limits são verificados no fechamento do candle. Se o feed ao vivo para, ou o app está offline, o candle assume de novo e o log da sessão diz isso.
- Uma entrada é **alertada no fechamento que dá seu sinal**. Uma ordem a mercado é anunciada ao preço desse fechamento, com seu tamanho (tomado sobre o patrimônio marcado nesse fechamento) e seu **valor** em dinheiro para um broker que aceita uma ordem nocional em unidades fracionárias, depois executada na próxima abertura: no primeiro preço ao vivo quando o provedor transmite um (seu stop e alvo são então observados ao vivo a partir dessa execução), caso contrário na abertura desse candle depois que ele fechou. O preço é atualizado sem um segundo alerta. Uma ordem limit é anunciada quando é colocada (*Acionando ordem limit*), e sua execução é alertada quando acontece.
- Uma entrada limit executada ao vivo recebe seu stop e alvo da próxima execução; até lá essa posição é protegida no fechamento do candle.

O paper trading roda o mesmo motor do backtest, com as mesmas opções: com *Baixar candles faltantes de timeframe menor* ou bid/ask ligados, uma execução busca o que o candle que acabou de fechar precisa, esperando um momento para o provedor publicá-lo. A linha da sessão lista as ordens limit que ela tem em andamento.

### Estratégias e indicadores personalizados

- **Estratégias nomeadas**: salve, busque, duplique e edite configurações completas de estratégia.
- **Versões de estratégia** (opcional): com o versionamento ligado em [Configurações → Versões](/pt/config/settings#versioning), uma estratégia salva pode ser versionada pelo menu de histórico no cabeçalho. Salve uma versão (datada, com uma nota opcional), reveja o que cada etapa dizia, restaure-a (o estado restaurado é salvo como uma nova versão, anotada com a data da versão restaurada, e mantém seu nome) ou exclua-a. O histórico abre em uma janela com busca em notas e datas; a versão que corresponde à estratégia atual é marcada. As notas são limitadas a 500 caracteres. As edições não salvas são salvas antes de a versão ser tirada. Desligar o versionamento de uma estratégia pergunta se você quer manter ou excluir suas versões. Excluir uma estratégia com versões pergunta se você quer mantê-las; as mantidas podem restaurá-la em **Estratégias excluídas com versões** na aba Estratégias. Agents com acesso de escrita ao Backtest podem fazer o mesmo por [MCP](/pt/config/ai-agents), salvando uma versão após cada atualização como um commit.
- **Indicadores personalizados**: monte os seus a partir de etapas nomeadas, sem código. Cada etapa aplica um indicador integrado a uma **fonte** (um campo de preço ou a saída de uma etapa anterior) ou calcula uma **fórmula** referenciando etapas anteriores pelo nome (`@volume / SMA(@volume)`, com `+ − × ÷`, `min`, `max`, `abs`, `clamp`). Isso permite encadear indicadores: uma Hull MA de um RSI, um MACD de um RSI, uma razão de volume suavizada e assim por diante. Indicadores que leem candles completos (ATR, Stochastic, ADX, VWAP…) só se aplicam ao preço, não a uma etapa derivada. As etapas destacadas são a saída. Os indicadores personalizados viram operandos no editor de regras ao lado dos integrados, e a biblioteca é **compartilhada com o gráfico**, que desenha a mesma definição ([Indicadores personalizados no gráfico](#indicadores-personalizados-no-grafico)).
- **Seletor de indicadores pesquisável**: escolha indicadores de uma lista agrupada com filtro por digitação (tanto no editor de regras quanto no construtor de indicadores personalizados) em vez de rolar um dropdown longo.

### Janelas de datas e varreduras de parâmetros (API)

Duas capacidades vivem na API e não no formulário. Elas existem para o [assistente](/pt/modules/agent) e para quem controla o app por [MCP](/pt/config/ai-agents):

- **Execuções com janela de datas.** `from` / `to` em uma execução (e na prévia de alinhamento) restringem o intervalo simulado, que é o que a validação walk-forward e o fatiamento por regime precisam: rode 2019–2021, depois 2022–2024, e compare. `to` inclui o dia inteiro.
- **`POST /api/backtest/sweep`**: execute uma grade de parâmetros no servidor e receba **todas as tentativas de volta**, junto com a contagem de tentativas. Os caminhos da grade alcançam arrays (`long.entry.conditions.0.left.period`), então os períodos dos indicadores são varríveis; limitado a 4 eixos e 64 tentativas.

Uma varredura também retorna um **Sharpe deflacionado**: o Sharpe que a melhor de N estratégias *sem valor* esperaria alcançar, dado o quanto essas tentativas específicas variaram. Compare o vencedor com essa barra, não com zero: em barras diárias reais, a melhor de oito médias móveis cruzadas marcando 0,59 contra uma barra de seleção de 0,70 significa *nenhuma evidência de vantagem*, o que o máximo sozinho teria escondido.

Ela também retorna a **probabilidade de overfitting do backtest** (PBO) da grade: as curvas de patrimônio das tentativas são cortadas em 16 blocos, cada metade é usada uma vez como conjunto in-sample e uma vez como out-of-sample, e a PBO é a parcela de divisões em que o vencedor in-sample fica na metade de baixo fora da amostra. A aba [Comparar](#quant) do Quant calcula o mesmo número em execuções salvas.

### Otimizador

Pegue uma execução terminada e **varie seus parâmetros**: comprimentos e limiares de indicadores por lado, stops, dimensionamento, custos, limites da carteira, configurações de grade, e quais dias da semana excluir (todo subconjunto é tentado). Cada parâmetro recebe um de / até / passo, e o cabeçalho conta as variantes conforme você as amplia, limitado para a grade continuar finita.

- **Antes de começar**, ele estima o custo a partir do que execuções passadas mediram na sua máquina: milissegundos por variante, workers, tempo total. Você pode parar a qualquer momento e manter o que foi calculado.
- O **ranking** é pela métrica que você escolher (Sharpe, Sortino, retorno, profit factor, win rate, expectancy, drawdown máximo, trades); qualquer coluna reordena depois. Com uma divisão out-of-sample, todo número ranqueado é o **in-sample**, e o retorno out-of-sample é mostrado mas nunca ranqueado: um vencedor escolhido nele o teria visto.
- A **análise** mostra a dispersão da métrica escolhida em todas as variantes: pior, média, melhor, e quantas ficaram positivas. Um único número bom significa pouco se seus vizinhos são péssimos.
- **Desconto por múltiplos testes**: medido nos candles por ano que os dados realmente têm (um mercado 24/7 tem cerca de cinco vezes mais candles horários que uma ação), o melhor Sharpe é mostrado contra a **barra de seleção**, o Sharpe que a melhor de tantas estratégias *sem valor* esperaria alcançar. Abaixo da barra, tentar tantas variantes basta para explicar o vencedor.
- Clique em uma variante para ler seu **backtest completo**, reproduzido a partir das próprias configurações. Nada é armazenado até você **mantê-la** no histórico.

### Divisão out-of-sample

Divida os dados em uma cabeça **in-sample** e uma cauda **out-of-sample**; a estratégia roda nas duas e os dois blocos de estatísticas (retorno, profit factor, win rate, drawdown máximo, trades) são mostrados lado a lado. Uma grande diferença entre as colunas é sinal de overfitting.

### Paper trading

*Uma execução terminada, deixada rodando para a frente.* Pressione **Paper trade** em um resultado e a estratégia continua operando em papel, em agenda, alertando os canais que você escolher. Não há segundo motor: toda execução re-simula a janela com o backtest comum e informa o que mudou, então uma execução paper é por construção a execução que o backtest teria mostrado para os mesmos candles.

- **A estratégia é congelada** como rodou, instrumentos incluídos. Editar essa estratégia depois não altera uma sessão em andamento; o formulário da própria sessão oferece atualizá-la ou iniciar uma cópia quando você salvar a estratégia de novo.
- **A primeira execução semeia o book.** Todo round trip já na janela é registrado de uma vez, resumido em um único evento: abrir uma sessão não dispara uma rajada de alertas sobre o histórico. Apenas o que acontece depois disso é uma execução que merece uma mensagem.
- **Janela**: quanto histórico cada execução alimenta ao motor (candles finais, ou um início fixo).
- A aba **Paper** lista as sessões com seu status, próxima execução, posições abertas e eventos não lidos, e cada uma pode ser executada agora, pausada, retomada ou excluída. O log de eventos é mantido tenha algo sido enviado ou não, então o que não pôde ser entregue ainda está lá quando você volta.

#### Agenda

Cada execução, e seus dados, está no relógio do próprio candle.

- **Intervalo** (a cada N minutos), **diário**, **semanal**, **mensal** ou **uma vez**, em um fuso horário real.
- Um intervalo dispara na **grade de candles**, nunca no segundo em que a sessão foi criada: a cada minuto em :00, a cada 15 minutos em :00 / :15 / :30 / :45, de hora em hora na hora cheia.
- A execução **baixa o candle que espera** para os conjuntos de dados da própria sessão. O candle em andamento nunca é armazenado: o que é lido é o último *fechado*, uma semana da sua segunda-feira até a próxima, e um dia somente depois que um dia inteiro passou desde seu carimbo (um candle diário dos EUA chega depois das 04:00 UTC). Um último candle armazenado antes de seu período terminar é lido de novo. Um candle que o provedor ainda não publicou é pedido de novo ao longo de alguns segundos, e um período sem nenhum trade não grava nada, o que simplesmente significa que a próxima execução não tem nada novo para simular.
- Uma execução que não encontra candle novo não custa nada: ela nem simula.

#### O que é enviado, e para onde

- **Notificar**: a cada execução, apenas em uma nova execução, ou nunca. *A cada execução* também informa as silenciosas, que é a única forma de distinguir "nada aconteceu" de "o motor parou de rodar".
- **Agrupar mensagens**: enviar imediatamente, ou um digest horário, diário ou semanal. Um envio agrupado é uma mensagem sobre um período, não um ping por execução.
- **Alertar em** entradas, saídas, ou ambas, e opcionalmente só para os **instrumentos** que você nomear.
- **Canais**: os [canais de notificação](/pt/config/settings#notifications) concedidos ao Backtest, todos eles ou os que você escolher. Sem nenhum canal configurado nada é enviado, e todo evento ainda é registrado no app.

#### Mensagens personalizadas

Três mensagens, três redações, cada uma com seu vocabulário: **Entrada**, **Saída** e **Resumo** (a agrupada). Deixe um campo vazio e a redação integrada é usada.

- Cada uma tem um **Título** e um **Corpo**, escritos com placeholders:

```
{{trade.ticker}} {{trade.direction}} at {{trade.entry_price}}
```

- **Variáveis** lista exatamente o que aquela mensagem pode ler, com um valor de exemplo; clique em uma para inseri-la. Uma entrada lê a metade de entrada do trade, uma saída o trade inteiro (P&L incluído), e o resumo lê o período, `since.*` (desde o último alerta), `total.*` (desde que a sessão começou) e `open.*` (o que está em posse agora), mais `session.*`, `event.*` e `stats.*`. Um caminho que uma mensagem não pode ler não lhe é oferecido.
- Os **filtros** são escritos `| name:arg`, e `upper`, `lower`, `trim` e `json` não recebem nenhum:

```
{{trade.pnl | round:2}} {{since.from | date:YYYY-MM-DD}} {{trade.exit_reason | default:-}}
```

- A **Prévia** é o próprio renderizador do servidor, então o que ela mostra é o que seria enviado. Ela roda sobre um trade de exemplo e os últimos números da sessão, então um valor que a sessão ainda não produziu é marcado no lugar, entre colchetes, e o restante da mensagem ainda é renderizado.

### Resultados

- Estatísticas de destaque: retorno (vs **buy & hold**), PnL líquido e taxas, win rate, profit factor, expectancy, drawdown máximo, Sharpe/Sortino.
- **Curva de patrimônio** sobreposta ao preço com marcadores de entrada/saída.
- Um **resumo de desempenho** completo (lucro/perda bruto, payoff ratio, maior ganho/perda, máximo de ganhos/perdas consecutivos, média de barras em trade…) e a **lista de trades** completa com **MAE/MFE** por trade (pior perda aberta / melhor lucro aberto enquanto no trade), filtrável, e motivos de saída (sinal esmaeceu, sinal de saída, stop-loss, take-profit, revertido, fim dos dados).
- **Salve execuções** por nome e mantenha um histórico para comparar estratégias depois. O menu **Relatórios** de uma execução terminada a exporta inteira, por lado e por ativo: um **PDF** com todos os gráficos, ou **Markdown** apenas com os números. Nenhum dos dois traz a lista de trades; o arquivo recebe o nome da estratégia e do momento em que foi exportado.

## Quant Tools {#quant}

Análises sobre seus conjuntos de dados, seus backtests salvos e, para derivativos, direto de um provedor. As abas vêm em seis grupos.

Quais abas precisam do quê:

- **Ativo** e **Multiativos** leem conjuntos de dados armazenados do [Historical Data](#histdata).
- **Estratégia** lê execuções salvas do [Backtest](#backtest).
- **Dimensionamento** e **Calculadoras** recebem números que você digita (o alvo de volatilidade também lê um conjunto de dados).
- **Derivativos** lê um [connector](/pt/config/connectors) da Interactive Brokers ou da Massive concedido ao Quant.

### Ativo

Um conjunto de dados e uma janela de tempo, compartilhados por todas as abas do grupo.

- **Risco**: volatilidade histórica anualizada, drawdown máximo, **Value at Risk** e **Conditional VaR** na sua confiança. A cauda além do VaR é estimada de três formas (normal, Cornish-Fisher, um ajuste de Generalized Pareto das piores perdas). Uma tabela lista os piores drawdowns com sua profundidade, datas e as barras que levou para chegar ao fundo e se recuperar, já que um único número de drawdown máximo esconde quanto tempo o buraco levou para ser preenchido.
- **Estatística**: que tipo de série é esta.
  - Distribuição contra uma normal de mesma média e volatilidade: assimetria, curtose em excesso, um gráfico QQ.
  - Dependência serial: autocorrelação dos retornos e dos retornos absolutos, com p-valores de Ljung-Box. Os retornos raramente mostram alguma, os retornos absolutos geralmente mostram: é o agrupamento de volatilidade.
  - Tendência ou reversão à média: Hurst (R/S e DFA), razões de variância por horizonte e a meia-vida do preço.
  - Estacionariedade: ADF e KPSS, lidos juntos.
  - Se o Sharpe é distinguível de sorte: t-stat, Sharpe probabilístico, comprimento mínimo de histórico.
- **Volatilidade**:
  - Cinco estimadores sobre as mesmas barras. Close-to-close usa apenas os fechamentos; Parkinson, Garman-Klass, Rogers-Satchell e Yang-Zhang também leem a amplitude da barra.
  - Seus caminhos móveis.
  - Um **cone de volatilidade** que diz se a leitura de hoje é alta ou baixa para o seu horizonte.
  - Uma previsão **GARCH(1,1)** com sua persistência e meia-vida de choque.
- **Regimes**:
  - Um modelo oculto de Markov gaussiano divide os retornos em 2 a 4 estados, o mais calmo primeiro, e sombreia o gráfico de preço pelo estado mais provável.
  - O retorno, a volatilidade, o tempo gasto e a permanência típica de cada estado.
  - As probabilidades de transição, e em qual estado o mercado mais provavelmente está agora.
- **Eventos**: escolha uma condição (gap, grande fechamento, cruzamento de SMA ou RSI, nova máxima ou mínima de N barras, sequência, pico de volume) e veja o que o mercado fez depois.
  - Retornos futuros em vários horizontes contra a linha de base incondicional sobre as mesmas barras, com um p-valor por horizonte.
  - O caminho médio em torno do evento.
- **Sazonalidade**:
  - Um heatmap mês × dia da semana de retorno médio, volatilidade, amplitude da barra ou volume, mais faixas por mês, dia da semana e (apenas intradiário) hora.
  - Cada célula mostra sua contagem de amostras e win rate. O relógio horário é **UTC**.

### Multiativos

Dois ou mais conjuntos de dados com o mesmo timeframe.

- **Carteira**: matriz de correlação, **fronteira eficiente** (uma nuvem de alocações aleatórias; clique no ponto de Sharpe máximo ou de volatilidade mínima para ler seus pesos) e **risk parity**.
- **Pares**:
  - Cointegração (Engle-Granger, e Johansen nas duas direções) e o spread com seu hedge ratio, z-score e meia-vida.
  - Correlação e beta móveis.
  - Correlação lead-lag e causalidade de Granger, para ver se uma série se move primeiro.
- **Cesto**:
  - **PCA**: quantas apostas independentes o cesto realmente tem.
  - Um **dendrograma** de correlação: quem se move junto.
  - Uma alocação de **hierarchical risk parity**.
  - Uma tabela de força relativa em 1, 3, 6 e 12 meses.
  - Um stress test que mantém uma ponderação em cada crise passada que os dados cobrem (2008, 2020, 2022 e outras).
- **Regressão**:
  - Os retornos de um ativo sobre um ou mais conjuntos de dados de fatores (um índice, títulos, ouro, um setor).
  - Alpha com seu t-stat, o beta de cada fator, R², tracking error e information ratio.
  - Captura de alta/baixa e um beta móvel.

Misturar classes de ativos é permitido, inclusive provedores diferentes: as barras são associadas pelo **período** a que pertencem, não pelo timestamp que o provedor lhes deu. Um candle diário de cripto abre às 00:00 UTC e um de ações dos EUA no início da sessão de Nova York, e os dois são o mesmo dia. Duas coisas decorrem disso, e o painel diz qual se aplicou:

- Um cesto que mistura um mercado 24/7 com um de horário de bolsa é medido **semanalmente**. Alinhado diariamente, o movimento de fim de semana do ativo contínuo cairia na mesma linha da segunda-feira do outro e subestimaria o quanto eles realmente se movem juntos.
- A anualização é **contada no relógio** em vez de presumida: os mesmos conjuntos de dados diários são 252 períodos por ano em uma bolsa e 365 em um mercado 24/7.

Os conjuntos de dados intradiários são a exceção: barras de 4h ancoradas em uma sessão de trading e barras de 4h ancoradas no relógio ficam 90 minutos distantes, então um cesto intradiário de provedores mistos é recusado em vez de aproximado. Use conjuntos de dados diários, ou um provedor para o cesto inteiro.

### Estratégia

Execuções de backtest salvas. Cada execução é reproduzida no servidor para regenerar seus trades exatos.

- **Monte Carlo**: reamostra os trades de uma execução milhares de vezes (um a um, ou em blocos para manter as sequências juntas).
  - **Bandas de percentis** no caminho do patrimônio, patrimônio final e drawdown máximo.
  - A probabilidade de terminar com prejuízo.
  - Um **risco de ruína**: a parcela de caminhos cujo patrimônio chegou a cair a um limiar que você define.
  - A curva de patrimônio real desenhada por cima.
- **Operações**: quanto os trades valem por unidade de risco.
  - Expectancy em moeda e em **R**, a distribuição de múltiplos de R e **SQN** (em no máximo 100 trades, com a classificação de Van Tharp).
  - A dispersão **MAE/MFE**: quanto calor os ganhadores aguentaram, até onde os perdedores correram antes.
  - 1R é o stop quando a execução tem um stop percentual, caso contrário a perda média, e a página diz qual. Execuções de grade e DCA não registram MAE/MFE, então a dispersão fica de fora para elas.
- **Comparar**: 2 a 20 execuções nas suas datas em comum.
  - Curvas de patrimônio rebaseadas, uma tabela de retorno, volatilidade, Sharpe e drawdown, e a correlação dos seus retornos.
  - O **Sharpe deflacionado** da melhor execução, as outras contando como as tentativas de onde ela foi escolhida.
  - A **probabilidade de overfitting do backtest** (PBO, por validação cruzada combinatorialmente simétrica sobre 8 a 16 blocos). Uma PBO acima de 50% significa que o vencedor in-sample normalmente cai na metade de baixo fora da amostra.

### Dimensionamento

- **Tamanho da posição**: a partir da sua banca, entrada, stop e risco (porcentagem ou fixo), o **tamanho, nocional, margem, exposição e reward:risk**. Ele pode **sugerir stops** a partir de um conjunto de dados (volatilidade, ATR, swing) e preencher a entrada com o último fechamento.
- **Kelly**: a fração de Kelly a partir do win rate e do payoff, com meio e quarto de Kelly. O Kelly cheio maximiza o crescimento de longo prazo mas oscila muito; a maioria dos traders dimensiona em meio ou quarto.
- **Alvo de vol**:
  - Mantenha a volatilidade-alvo ÷ volatilidade estimada do ativo, com a estimativa móvel ou EWMA, limitada a uma alavancagem máxima.
  - Cada barra é dimensionada na estimativa conhecida antes dela, então não há look-ahead.
  - Mostra o peso e as unidades a manter agora para o seu patrimônio, e o histórico escalado contra manter o ativo sem ajuste.
- **Risco de ruína**: a chance de que um win rate, um payoff e um risco por trade alcancem um dado drawdown, com um valor fixo ou uma fração fixa arriscada por trade. Três respostas:
  - As formas fechadas: Vince, para um valor fixo; o limite de Cramér-Lundberg, para qualquer dimensionamento.
  - Uma simulação sobre o número de trades que você definir, com seu erro padrão e a curva de ruína por contagem de trades.

### Calculadoras

- **Opções**: preço e gregas de Black-Scholes-Merton (vega e rho por ponto, theta por dia), uma árvore binomial para exercício americano com o prêmio de exercício antecipado, e a **volatilidade implícita** de um preço cotado.
- **Base de futuros**: a partir de um preço à vista e de um futuro, a base, o carry que ela implica por ano, o repo implícito, o valor justo à sua taxa e yield, e o roll yield até o próximo contrato.
- **Capitalização**:
  - Aonde levam um capital e um retorno por período, com aportes.
  - O retorno necessário para atingir um alvo, e quantos períodos leva.
  - O ganho necessário para subir de volta de um drawdown.
- **Teste de Sharpe**: para um Sharpe citado sem seus dados.
  - Ele é distinguível de zero, ou de um benchmark?
  - De que tamanho de histórico ele precisa?
  - O que sobra dele depois de considerar o número de estratégias tentadas (Sharpe deflacionado, com assimetria e curtose)?

### Derivativos

Estes leem o provedor diretamente em vez de um conjunto de dados armazenado, então precisam de um connector da **Interactive Brokers** ou da **Massive** concedido ao Quant.

Uma execução são dezenas de requisições ao provedor, cadenciadas por ele. A página mostra as requisições feitas sobre as planejadas e o que está sendo buscado. O que foi buscado é mantido por seis horas, então mudar uma taxa ou uma regra de roll recalcula sem perguntar de novo ao provedor.

- **Curva de futuros**: os contratos datados de um produto (`ES@CME` na Interactive Brokers, `ES` na Massive), os expirados incluídos.
  - A **estrutura a termo** na última sessão encerrada.
  - O **roll yield** entre o contrato da frente e o seguinte ao longo do tempo.
  - Uma série contínua de manter o da frente e fazer roll, ajustada retroativamente por razão para terminar no preço de hoje, ao lado da emendada que dá um salto a cada roll.
  - O roll é uma regra de calendário: o da frente é o contrato mais próximo com mais de *N* dias restantes.
  - Com um ticker à vista (por exemplo `SPX` como índice), acrescenta a base, o carry `ln(F/S)` por ano, o repo implícito, e a distorção de preço contra o valor justo à taxa e yield que você informar.
  - As curvas da Massive usam o preço de liquidação de cada sessão.
- **Superfície de VI**: a cadeia de opções de um subjacente.
  - Alguns vencimentos distribuídos entre o mínimo e o máximo de dias que você definir, strikes out-of-the-money de cada lado, cada preço transformado em uma volatilidade implícita Black-Scholes-Merton.
  - Smiles por vencimento, a **estrutura a termo ATM**, **risk reversal** e **butterfly** de 25 delta, e uma verificação de que a variância total ATM nunca cai de um vencimento para o seguinte.
  - A Interactive Brokers precifica cada opção no último ponto médio horário, tomado na mesma hora do subjacente, então nenhuma assinatura de dados de mercado de opções é necessária. A Massive precifica cada uma no fechamento da sessão.
  - A página informa o número de requisições antes de você começar: em uma chave Massive gratuita (5 por minuto) uma cadeia leva vários minutos.
- **Implícita vs realizada** (somente Interactive Brokers): a volatilidade implícita de 30 dias do subjacente, até dez anos atrás, contra a volatilidade close-to-close antes e depois de cada dia.
  - O **prêmio de volatilidade**: VI menos a volatilidade que se seguiu.
  - **IV rank** e **percentil** sobre um lookback que você escolher.
  - Quão bem a VI previu a volatilidade realizada (uma regressão da realizada que se seguiu sobre a VI).
  - A correlação das variações de VI com os movimentos de preço.
