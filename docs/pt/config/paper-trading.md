# Conta de simulação

Uma **conta de paper trading** é um broker gerido pelo próprio OpenTraderWorld. As ordens que recebe são executadas ao preço em direto do gráfico de onde saem, contra o seu próprio dinheiro simulado, com as comissões, o slippage e a alavancagem que definir. Sem chave, sem mercado, sem dinheiro real.

Negoceia-se a partir do gráfico exatamente como um broker real: o mesmo ticket, a mesma barra rápida, as mesmas linhas de ordens e posições, separadores do painel e registo de auditoria. Estão descritos em [Operar a partir do gráfico](/pt/config/trading); esta página cobre o que é próprio da simulação.

## Criar uma

**Definições → Brokers → Adicionar conta**, broker **Paper trading**. Sem credenciais. As definições (rotuladas em inglês pela aplicação):

| Definição | Por omissão | Significado |
|---|---|---|
| **Account currency** | `USD` | A moeda do dinheiro e dos resultados, 3 a 5 letras |
| **Starting balance** | `100000` | Registado como primeiro depósito |
| **Commission per order** | 0 | Um montante fixo por execução |
| **Commission per unit** | 0 | Por ação ou contrato |
| **Commission (% of value)** | 0 | `0.1` significa 0,1 % do valor negociado |
| **Minimum commission** | 0 | Uma execução nunca custa menos |
| **Slippage (basis points)** | 0 | `5` significa 0,05 %, contra si |
| **Leverage** | 1 | 1 é uma conta à vista |
| **Short selling** | yes | `no` recusa uma venda sem nada para vender |
| **Fill on** | `quote` | `quote` executa ao bid e ao ask, `last` ao último preço |

Os números têm de ser zero ou mais, e a alavancagem pelo menos 1. As definições são relidas a cada execução: uma alteração aplica-se a partir da seguinte.

Uma conta paper nova tem **Todos os módulos** e **Trading** ativados. O trading numa conta paper não pede palavra-passe e nunca está atrás do [bloqueio de trading](/pt/config/trading#trading-lock).

**Testar ligação** responde com o equity, o dinheiro, as posições abertas e as ordens ativas da conta.

## Dinheiro

O painel **Conta paper**, sob a conta em Definições → Brokers, tem um campo **Montante** e três botões:

- **Depositar** soma o montante ao dinheiro.
- **Levantar** retira-o, até ao dinheiro detido (não o equity).
- **Repor conta** apaga cada ordem, execução e movimento de dinheiro, e recomeça com um único depósito: o montante escrito, ou o saldo inicial se o campo estiver vazio. As definições mantêm-se, e os trades já importados para o journal ficam.

O painel mostra também o total depositado, e as últimas 50 ordens que deixaram de estar ativas com o seu estado (executada, cancelada, recusada, expirada) e o motivo. **É nessa lista que uma recusa é explicada.**

## Como as ordens são executadas

### A que preço

Uma ordem é executada no **fluxo em direto do conector de dados do gráfico**: o conector de onde veio a ordem, ou o conector do fornecedor concedido aos Gráficos. O motor corre no servidor, por isso **as ordens continuam a ser executadas com o gráfico fechado**, e param enquanto a aplicação está desligada.

- Um instrumento sem fluxo em direto é recusado ao enviar a ordem. Um índice também: não há mercado para negociar.
- O motor segue as barras em direto mais finas do fornecedor (1 minuto quando disponível) e as suas cotações quando o fluxo as publica. Só contam os preços impressos depois de a ordem ficar ativa.
- Com **Fill on: quote**, as compras executam ao ask e as vendas ao bid, enquanto a cotação tiver menos de 15 segundos. Caso contrário, e com **last**, ambas ao último preço.
- Uma barra com mais de 2 minutos não é usada. Uma ordem a mercado enviada com o mercado fechado espera pelo primeiro preço fresco em vez de ser recusada.
- Se o fluxo parar, é retentado a cada 10 segundos e as ordens esperam. Se o conector de dados for apagado, as suas ordens deixam de poder ser executadas.

### Por tipo de ordem

| Tipo | Executada quando | A |
|---|---|---|
| **Market** | o preço seguinte | esse preço, mais slippage |
| **Limit** | o mercado atinge o limite | o limite, ou melhor se o mercado já o ultrapassou. Sem slippage |
| **Stop** | o mercado atinge o stop | o stop se tocado, o mercado num gap, mais slippage |
| **Stop limit** | o stop é atingido, depois o limite | como um limit, possivelmente ao mesmo preço |
| **Trailing stop** | o mercado recua a distância de seguimento desde o seu melhor preço desde o início do stop (o mais alto atrás de uma venda, o mais baixo atrás de uma compra) | como um stop |
| **MIT** | o mercado chega ao gatilho pelo lado do limit | o mercado, mais slippage |
| **LIT** | o gatilho, depois o limite | como um limit |

- **Sem execuções parciais**: uma ordem é executada inteira, sem verificação de volume ou liquidez.
- Um **trailing stop** com preço de início espera que o mercado o atinja (subir até ele atrás de uma venda, descer até ele atrás de uma compra) e depois segue a partir desse preço. O seu melhor preço mantém-se após um reinício.
- **Reduce only**: colocada sozinha, é recusada a menos que a conta detenha uma posição do outro lado. Na execução é limitada ao que então se detém, e cancelada quando não resta nada a reduzir.
- **Post only**: um limit recusado na colocação quando seria executado de imediato contra o bid ou ask atual (o último preço sem cotação).
- Sem iceberg, escolha de gatilho, fora do horário regular, close-position nem algo: um preço em direto sem profundidade, sessão ou mark não tem onde os jogar, por isso o ticket não os oferece.
- O preço de execução é arredondado ao tick do instrumento quando é conhecido.
- **Comissão** por execução = por ordem + por unidade × quantidade + % × valor negociado, elevada ao mínimo. É cobrada na moeda do instrumento.

### Validade

- **Day** (por omissão) expira na próxima **meia-noite UTC**, não no fecho da bolsa.
- **GTC** nunca expira. **GTD** expira na data e hora indicadas.
- **IOC** e **FOK** são canceladas se o primeiro preço que encontrarem não as executar. Com o mercado fechado, esperam por esse primeiro preço. O paper executa inteiro, por isso as duas acabam iguais.

A expiração é verificada mais ou menos a cada 10 segundos; a ordem aparece então como expirada.

### Brackets e OCO

- Um **take profit** ou **stop loss** espera que a sua entrada seja executada e depois fica ativo. Se a entrada for cancelada, recusada ou expirar, as suas saídas são canceladas.
- Num **OCO**, uma execução cancela a outra ordem do grupo, e as suas saídas.
- Os preços, a quantidade e a validade de uma ordem ativa podem ser alterados. **Fechar posição** envia uma ordem a mercado GTC e depois cancela as saídas da posição.

## Fundos e alavancagem

- Uma posição imobiliza o seu valor ÷ alavancagem. Os futuros imobilizam antes a sua margem por contrato (ver abaixo) e ignoram a alavancagem.
- Uma ordem que abre ou aumenta uma posição é verificada ao enviar e de novo ao preço de execução, comissão incluída. É recusada se a conta imobilizasse mais do que o seu equity:

  > not enough funds: the positions would tie up X USD and the account's equity is Y USD

- Uma ordem que reduz uma posição é sempre aceite.
- Com **Short selling** em `no`, uma venda que ficaria a descoberto é recusada.
- **Não há chamadas de margem nem liquidação forçada**: o equity pode cair abaixo da margem em uso.

## Futuros

Um futuro precisa dos dados do contrato, nas **Definições do instrumento** do gráfico:

- **Tamanho do tick, valor do ponto e moeda.** Se estiverem vazios, são preenchidos a partir das especificações de contratos de código aberto do [QuantConnect Lean](https://github.com/QuantConnect/Lean), incluídas na aplicação e atualizadas semanalmente. A janela indica os campos preenchidos; guarde para os manter, ou escreva os seus. Os valores que guardou prevalecem sempre.
- **Margem por contrato (paper)**, na moeda do instrumento: use a margem inicial do seu broker. Ou marque **Contar o contrato pelo valor total**: quantidade × preço × valor do ponto.

Uma ordem sobre um futuro sem margem é recusada, e o gráfico propõe **Definir margem** (abre as definições do instrumento) ou **Usar valor total**. Qualquer das duas reenvia a ordem.

Um ticker é reduzido à raiz do seu contrato (`MNQZ6`, `ESZ25`, `@ES`, `ES=F`, `/ES`). Uma raiz cotada em várias bolsas sem bolsa indicada é um erro que nomeia as candidatas: **o mercado nunca é adivinhado**. Um futuro ou opção sem valor do ponto é recusado, tal como qualquer instrumento sem moeda, exceto um par forex, cuja moeda se lê do nome (`EURUSD` está em USD).

## Valores da conta

O separador **Conta** do painel de ordens e o painel das Definições mostram:

| Valor | Cálculo |
|---|---|
| **Caixa** | depósitos − levantamentos + realizado − comissões |
| **Resultado aberto** | Σ (preço − preço médio) × quantidade × valor do ponto |
| **Equity** | caixa + resultado aberto |
| **Realizado** | desde a última reposição, antes de comissões, a custo médio |
| **Comissões** | total das comissões |
| **Margem inicial** | margem por contrato dos futuros + valor das outras posições ÷ alavancagem |
| **Disponível** | equity − margem inicial, nunca abaixo de zero |
| **Valor das posições** | exposição bruta, longa e curta |

O separador atualiza-se a cada 5 segundos enquanto estiver aberto.

### Outras moedas

A conta tem uma só moeda. Um instrumento noutra é convertido à **taxa de câmbio do dia do journal**: resultados realizados e comissões à taxa da sua execução, resultados abertos e margem à de hoje.

- Uma ordem é recusada quando não existe taxa: `no EUR to USD rate: add one in the journal's FX settings, or trade in USD`.
- Uma posição cuja taxa desaparece mais tarde é indicada como excluída dos totais.

## Para o journal

As execuções paper importam-se como as de qualquer broker: **Journal → Importar → De um broker**, escolha a conta paper, o livro e o período, pré-visualize, importe. Veja [Importar trades para o journal](/pt/config/brokers#import-trades-into-the-journal). A **sincronização automática** também funciona.

## Bom saber

- Os montantes são guardados como decimais exatos, como uma bolsa os regista: 0,1 comprado três vezes e 0,3 vendido não deixa nada em aberto, e um preço tem de cair exatamente no tick do instrumento.
- O dinheiro, as ordens e as execuções ficam na base de dados e sobrevivem a reinícios. Os preços em direto não: depois de um reinício, as posições são avaliadas ao preço médio até à cotação seguinte.
- Apagar a conta apaga o seu dinheiro, as suas ordens e as suas execuções.
- Cada chamada de ordem está no [registo de auditoria](/pt/config/trading#audit-log).
- Não modelado: posição na fila, liquidez, execuções parciais, juros, custos de empréstimo, funding, dividendos.
- As ordens e movimentos de dinheiro paper não estão expostos aos [agentes de IA (MCP)](/pt/config/ai-agents), e as contas paper estão desativadas no [modo demo](/pt/guide/demo).
