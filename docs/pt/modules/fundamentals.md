# Fundamentals

Um só lugar para ler a economia e uma empresa a partir das fontes que publicam os números: séries macro com gráficos, demonstrações de empresas, registros da SEC, transcrições de earnings calls, holdings de ETFs, um calendário de mercado e dados alternativos. Tudo é armazenado no seu próprio banco, então gráficos, o [Agent](/pt/modules/agent) e outros módulos o leem sem perguntar de novo ao provedor.

O Fundamentals não tem configurações de provedor próprias. Toda fonte é um **[data connector](/pt/config/connectors)** concedido ao módulo: o ícone de plugue no cabeçalho da página abre a tela compartilhada de connectors. Muitas fontes são órgãos públicos **sem chave** (SEC EDGAR, o Tesouro dos EUA, o BCE, o Eurostat, o BIS, a OCDE, o FMI, o Banco Mundial, a CFTC, a FINRA, o USAspending); as demais pedem uma chave gratuita ou paga que você traz. Nada é buscado até você adicionar um connector e concedê-lo ao Fundamentals.

## Páginas

| Página | O que ela mostra |
|---|---|
| **Macro** | Suas séries por categoria (crescimento, inflação, trabalho, juros, moeda, pesquisas, habitação, energia, fiscal, posicionamento), até quatro em um gráfico, a curva de juros do Tesouro e as taxas de política dos bancos centrais. |
| **Empresa** | Perfil e métricas-chave, demonstrações financeiras, estimativas, resultados, segmentos, dividendos e recompras, acionistas, pares, ESG e remuneração, registros e transcrições. |
| **ETF** | Perfil, principais holdings, exposição por setor e país. |
| **Eventos** | Próximos resultados, IPOs, ações corporativas e decisões de bancos centrais. |
| **Documentos** | Todo registro e transcrição armazenado, com busca em texto completo e o leitor de transcrições. |
| **Dados alternativos** | Trades do Congresso, gastos com lobby, contratos federais e patentes concedidas. |
| **Biblioteca** | Abas para as séries e empresas que você mantém (filtráveis), a prioridade de fontes e qual provedor serve qual família de dados. |

**Personalizar** (canto superior direito) define a densidade, quais seções aparecem e em que ordem, página por página.

## Séries macro {#macro}

**Adicionar séries** pesquisa o catálogo de um provedor ou aceita o código do próprio provedor (`CPIAUCSL` no FRED, `HICP/M.U2.N.000000.4D0.ANR` no BCE). Um código é verificado junto ao provedor antes de qualquer coisa ser armazenada: um código desconhecido é um erro que o nomeia, nunca uma série vazia. **Adicionar um conjunto inicial** adiciona uma primeira seleção de séries dos EUA e da zona do euro com um clique.

| Provedor | Chave | O que cobre |
|---|---|---|
| FRED | gratuita | A maioria das séries dos EUA (também espelha BLS, BEA e Census) |
| US Treasury | nenhuma | Curva de juros par diária, dívida pública total |
| ECB, Eurostat | nenhuma | Inflação, juros, moeda, PIB e desemprego da zona do euro |
| BIS | nenhuma | Taxas de política dos bancos centrais, taxas de câmbio efetivas |
| OECD, IMF, World Bank | nenhuma | Indicadores antecedentes, World Economic Outlook, dados anuais por país |
| BLS, BEA, EIA, US Census | gratuita | Detalhe dos EUA quando o FRED atrasa ou não tem uma série |
| CFTC | nenhuma | Commitments of Traders, posicionamento líquido não comercial |

Uma linha é um **período**: uma observação é armazenada contra o início do período que ela cobre. O gráfico calcula as transformações na leitura (nível, ano contra ano, variação do período, diferença, índice 100), então nada derivado é armazenado. Ano contra ano compara cada valor com o datado um ano antes; um período ausente mostra uma lacuna em vez de uma comparação com o mês errado. O sombreado de recessão segue as datas do NBER.

## Empresas {#company}

Empresa, ETF e Dados alternativos compartilham um **seletor de símbolo**: seus favoritos primeiro, depois os 15 abertos mais recentemente. A busca cobre todo símbolo armazenado, mais correspondências do EDGAR para empresas.

Digite um ticker. Ele é resolvido na lista de tickers da SEC EDGAR; um ticker que o EDGAR não conhece é um erro, nunca um palpite. Abrir uma empresa a armazena e busca, em segundo plano:

- **Demonstrações** a partir dos company facts XBRL, anuais e trimestrais. Quartos trimestres e linhas de fluxo de caixa acumuladas no ano são derivados por diferença; cada linha mantém a tag sob a qual foi informada.
- **Registros** (10-K, 10-Q, 8-K, proxies...) com um link para a fonte.
- **Trades de insiders** extraídos do Form 4.

As outras abas leem os agregadores que você conecta, da melhor fonte para a pior. Um provedor cujo plano deixa um conjunto de dados de fora (uma chave gratuita da FMP e o histórico de resultados, por exemplo) passa para o próximo, e um connector concedido ao módulo sem sua chave é ignorado. Para o preço, vence o histórico mais longo (planos gratuitos costumam parar em um ou dois anos):

| Dados | Provedores |
|---|---|
| Estimativas, preços-alvo, ações de rating | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Resultados (EPS estimado e realizado, próxima data) | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Segmentos | Financial Modeling Prep |
| Dividendos e splits | EODHD, Massive, Financial Modeling Prep, Alpha Vantage |
| Detentores 13F | Financial Modeling Prep |
| Short interest | FINRA, Massive |
| Pares | Financial Modeling Prep, Finnhub |
| ESG e remuneração de executivos | Financial Modeling Prep, Finnhub |
| Transcrições | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Preço e múltiplos de mercado | qualquer connector de dados de mercado com barras diárias de ações |

**Biblioteca → Prioridade de fontes** lista todo conjunto de dados com mais de uma fonte (transcrições incluídas) na ordem em que seus provedores são tentados. Escolha uma posição ao lado de um provedor para movê-lo para lá; **Ordem padrão** restaura a ordem do app. O preço não tem ordem: vence o histórico mais longo.

Cada aba diz qual provedor respondeu e quando, ou o erro que nomeia a correção (em geral um connector a adicionar ou um plano que não inclui o conjunto de dados). Uma resposta é mantida e reutilizada até ficar obsoleta (algumas horas para o calendário, um dia para estimativas, uma semana para detentores); **Atualizar** pergunta de novo agora.

Abrir uma página não gasta sua cota com um provedor que acabou de recusar: um que rejeitou o conjunto de dados (plano, símbolo, limite de taxa) é deixado em paz por um tempo, de alguns minutos após um erro de rede a uma semana após uma recusa de plano, e o mesmo vale para um cuja cota, declarada no seu connector, está esgotada. A aba diz isso e quando é a próxima tentativa automática; **Atualizar** pergunta a todos os provedores de uma vez.

**Seguir** uma empresa para tê-la atualizada diariamente e ser avisado sobre seus novos registros.

### Transcrições {#transcripts}

A aba Transcrições lista as calls que um provedor tem da empresa; o texto de uma transcrição é buscado na primeira vez que você a abre, dividido em falas por orador, com as declarações preparadas separadas do Q&A, e pesquisável junto aos outros documentos.

## Dados alternativos {#alt}

A empresa é a que está aberta em Empresa; um ticker digitado aqui é aberto primeiro.

| Aba | Provedores |
|---|---|
| Trades do Congresso | Quiver Quant (os mais recentes de todos os membros, ou de uma empresa), Finnhub premium (por empresa) |
| Lobby | LDA.gov, Quiver Quant |
| Contratos do governo | USAspending, Quiver Quant |
| Patentes | USPTO Open Data Portal (chave gratuita), Quiver Quant |

As fontes públicas conhecem uma empresa pelo seu **nome registrado**, não pelo ticker. LDA.gov, USAspending e a USPTO são consultados com o nome que o EDGAR armazena, e um registro só conta quando o nome é o mesmo depois de descartados a pontuação e o sufixo legal (`Lockheed Martin Corp` corresponde a `LOCKHEED MARTIN CORPORATION`, nunca a `Lockheed Martin Aculight`). Cada aba mostra o nome com que correspondeu. Para contratos, usa-se o beneficiário pai, então subsidiárias registradas sob ele contam e uma registrada separadamente (Amazon Web Services sob Amazon) não.

O LDA.gov pede uma chave gratuita (registre-se em lda.gov). Seu firewall barra redes de fora dos EUA (um HTTP 403 que o nomeia): acesse-o de uma conexão dos EUA, ou use o Quiver Quant. Um relatório conta uma vez: uma emenda substitui o original, e os registros de lobistas, que não trazem gasto, ficam de fora. Uma empresa que faz lobby por meio de uma subsidiária com outro nome (JPMorgan Chase Holdings para JPMorgan Chase) mostra apenas os relatórios arquivados sob o próprio nome.

## Cobertura de provedores {#coverage}

Qual provedor pode servir quais dados, como em **Biblioteca → Cobertura de provedores**. Uma família servida por vários provedores é tentada na ordem de **Biblioteca → Prioridade de fontes**. O preço e os múltiplos de mercado vêm de qualquer connector de dados de mercado com barras diárias de ações (Alpha Vantage, EODHD, Massive, Yahoo, IBKR...), não listados aqui.

| Provedor | Chave | Macro | COT | Demonstrações | Registros | Insiders | Estimativas | Resultados | Segmentos | Dividendos | Detentores | Short interest | Pares | ESG | ETF | Calendário | Transcrições | Dados alt. |
|---|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| SEC EDGAR | nenhuma |  |  | ✓ | ✓ | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |
| FRED (St. Louis Fed) | gratuita | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Treasury | nenhuma | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| ECB Data Portal | nenhuma | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| Eurostat | nenhuma | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BIS | nenhuma | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| OECD | nenhuma | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| IMF | nenhuma | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| World Bank | nenhuma | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BLS | gratuita | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BEA | gratuita | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| EIA | gratuita | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Census | gratuita | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| CFTC | nenhuma |  | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| FINRA | nenhuma |  |  |  |  |  |  |  |  |  |  | ✓ |  |  |  |  |  |  |
| Financial Modeling Prep | plano gratuito |  |  |  |  |  | ✓ | ✓ | ✓ | ✓ | ✓ |  | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| Finnhub | plano gratuito |  |  |  |  |  | ✓ | ✓ |  |  |  |  | ✓ | ✓ |  | ✓ | ✓ | ✓¹ |
| Alpha Vantage | plano gratuito |  |  |  |  |  | ✓ | ✓ |  | ✓ |  |  |  |  | ✓ | ✓ | ✓ |  |
| EODHD | plano gratuito |  |  |  |  |  |  |  |  | ✓ |  |  |  |  | ✓ | ✓ |  |  |
| Massive (Polygon.io) | plano gratuito |  |  |  |  |  |  |  |  | ✓ |  | ✓ |  |  |  |  |  |  |
| USAspending | nenhuma |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| LDA.gov (lobbying) | gratuita |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| USPTO Open Data Portal | gratuita |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| Quiver Quant | paga |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |

¹ A Finnhub serve trades do Congresso apenas em um plano premium.

Indicativo: os planos mudam, e um plano gratuito pode deixar uma família de fora (Financial Modeling Prep gratuito: sem detentores 13F, transcrições ou holdings de ETF; Finnhub gratuito: sem ESG nem trades do Congresso; Alpha Vantage gratuito: 25 requisições por dia). Confira a página do próprio provedor antes de pagar.

## Atualização automática e notificações {#refresh}

As séries armazenadas são atualizadas assim que sua frequência indica que um novo valor pode ter saído: uma série diária duas vezes ao dia, uma semanal ou mensal diariamente, uma trimestral a cada três dias, uma anual semanalmente. As empresas seguidas são atualizadas a partir do EDGAR uma vez ao dia. Uma fonte sem connector concedido é ignorada.

Uma série com um novo período e uma empresa seguida com um novo registro (formulários de insiders à parte) geram uma notificação, enviada aos [canais](/pt/config/settings#notifications) concedidos ao Fundamentals (o sino no cabeçalho da página).

## Widgets do dashboard {#dashboard}

O [Dashboard](/pt/modules/dashboard) oferece séries e quadros macro, snapshots de empresas, histórico de demonstrações, empresas, registros, próximos resultados e comparações de valuation. Os cartões mostram moeda, base de relato e datas; sua atualização lê apenas dados armazenados. Próximos resultados usa o snapshot de resultados armazenado de uma empresa ou, quando ele não tem data futura, o calendário de mercado armazenado. O valuation usa empresas armazenadas selecionadas manualmente ou os pares armazenados da sua aba Pares. Estimativas e múltiplos ausentes continuam indisponíveis em vez de inferidos.

## Busca e agents {#search}

A busca da barra superior, com títulos de conteúdo ligados, encontra empresas, séries e títulos de documentos armazenados.

Os agents alcançam o que está armazenado, somente leitura, pelo [gateway MCP](/pt/config/ai-agents) assim que um token recebe **Fundamentals**: séries e observações, empresas, demonstrações, registros, transcrições, trades de insiders e todo conjunto de dados armazenado. Consultar um provedor, atualizar e adicionar séries ficam de fora: eles gastam a cota do seu provedor.
