# Dashboard e navegação

A tela inicial do app, mais as duas coisas que ficam acima de todo módulo: a caixa de busca e a caixa de notificações.

## Páginas do dashboard

O dashboard traz cinco páginas predefinidas, uma por forma de trabalhar: **Investor** (posições de longo prazo), **Trader** (execução e sessão de mercado), **Quant** (pesquisa, backtests e trabalho algorítmico), **Portfolio** (posições) e **Utilities** (automação, notas e ferramentas). Cada uma começa com um conjunto de widgets pronto. Elas são criadas uma vez e depois são páginas comuns: reorganize-as como quiser. Só as suas próprias páginas podem ser excluídas.

Você também cria **suas próprias páginas** com **Custom +** na seção **Espaço de trabalho** da barra lateral, que lista seus dashboards. Cada uma tem um nome, uma descrição opcional e uma **tag** curta exibida na barra de favoritos. A **barra de favoritos** no topo guarda de uma a cinco páginas, adicionadas ou removidas com a estrela ao lado; a primeira entrada é a página em que o dashboard abre. Quando a barra está cheia, uma nova estrela pergunta qual favorito substituir.

Use-as do jeito que um dia de trading se divide: uma página *Manhã* com o feed de notícias, o calendário econômico e o checklist da rotina; uma página *Posições* com a carteira e a watchlist; uma página *Admin* com tarefas e cronômetros.

## Editando um layout

**Personalizar** abre o modo **Editar layout**. Uma página é uma grade livre de 12 colunas: os blocos fluem da esquerda para a direita e quebram para a linha seguinte, e nenhuma linha é armazenada. No modo de edição você pode:

- adicionar **blocos de módulo** (um link para um módulo, e o mesmo módulo pode aparecer em quantas páginas quiser) ou **widgets** com o bloco **Adicionar**; **Adicionar widget** no cabeçalho também funciona fora do modo de edição;
- arrastar um bloco para qualquer lugar: os blocos seguintes se deslocam e a grade se reorganiza; **redimensionar** qualquer bloco pela largura em colunas;
- definir a altura de uma linha (**Compacto**, **Padrão** ou **Alto**) e abrir a **configuração** de um widget (a engrenagem no bloco), onde a altura **Título** transforma o widget numa faixa de cabeçalho de largura total, para um título de seção em Free text;
- inserir um **Espaçador**: um espaço de largura total com altura ajustável.

Os blocos são links, não cópias: removê-los de uma página nunca toca o módulo nem seus dados.

## Widgets

Um widget é uma prévia ao vivo e interativa de um módulo: ele lê e escreve pela API do próprio módulo, então o que você faz no widget é real. Widgets cujo módulo não está instalado simplesmente não são oferecidos.

A maioria dos widgets vem em várias **variantes**, cada uma uma visão diferente dos mesmos dados: o Net worth, por exemplo, oferece o valor atual, sua variação, uma tendência, ativos versus passivos, uma divisão por categoria, ativos a revisar e os maiores ativos. **Adicionar widget** lista cada variante como um cartão próprio, e **Widget type** na configuração do widget a troca depois. A tabela mostra o que cada widget cobre no conjunto de suas variantes.

| Widget | O que ele faz |
|---|---|
| **Free text** | Uma nota ou título que você mesmo escreve, markdown-lite. |
| **Notes** | Documentos do Editor editados recentemente, quantos você tem e um campo de uma linha para criar uma nota. |
| **News feed** | Últimos itens de um feed escolhido, em lista ou grade. |
| **Mailbox** | Os e-mails não lidos mais recentes, os mais novos primeiro. |
| **Time tracker** | Inicie/pare um cronômetro de projeto sem sair da página. |
| **Quick trade** | Escolha uma categoria + modelo e abra o formulário de adicionar trade. |
| **Journal analytics** | PnL líquido, taxa de acerto, profit factor, curva de patrimônio, posições abertas, exposição, insights de comportamento e um heatmap do PnL diário. |
| **Goals** | Uma lista curta de metas com progresso; adicione uma na hora. |
| **ToDo** | Tarefas abertas, marcáveis ali mesmo. |
| **Trading routine** | O checklist de hoje, marcável ali mesmo. |
| **Mindset** | O check-in do dia. |
| **Reminder** | Um formulário rápido de adicionar lembrete. |
| **Reminder stats** | Notificações não lidas, lembretes próximos e atrasados, e o que mais dispara. |
| **Calendar** | Hoje e esta semana num relance. |
| **Economic calendar** | Próximos eventos macro, compactados. |
| **Portfolio** | Um resumo da carteira com valor ao vivo. |
| **Subscriptions** | Gasto recorrente mensal, depois o que renova a seguir. |
| **Net worth** | Patrimônio líquido atual, sua variação numa janela que você define e um sparkline. |
| **Watchlist** | Cotações ao vivo de uma lista escolhida: preço, variação de 24h e 7d. |
| **Fundamentals** | Séries e quadros macro, um snapshot de empresa, uma linha de demonstração por trimestre, ano ou TTM, empresas ordenáveis, registros filtrados, próximos resultados e valuation frente a pares armazenados. Lido de dados armazenados sem gastar cota do provedor. |
| **Automator** | Workflows em execução, próximas execuções agendadas, taxa de sucesso por workflow, falhas recentes e execuções rápidas favoritas. |
| **Webhooks** | Eventos recebidos, eventos recentes, taxa de erro por endpoint e endpoints que ficaram em silêncio. |
| **Historical data** | Conjuntos de dados armazenados, tamanho em disco, a fila de downloads, lacunas de cobertura, atualidade e downloads com falha. |
| **Backtest** | Melhor execução salva, o ranking, a biblioteca de estratégias e as execuções rápidas favoritas. |
| **Finance database** | Favoritos por classificação e pasta, a atualização do catálogo e favoritos desatualizados. |
| **Charts** | Layouts e listas salvos, alertas de gráfico ativos e atividade recente de alertas. |
| **Managers' portfolios** | Gestores acompanhados, as maiores carteiras, a deriva das posições e as principais posições por variação. |
| **Community Docs** | Referências salvas por categoria, adições recentes e acesso rápido. |
| **Quant tools** | Conjuntos de dados e backtests disponíveis, risco e drawdown de um ativo, correlação, sazonalidade, volatilidade realizada frente ao seu intervalo histórico e o regime de mercado estimado. |
| **Tax calculator** | Imposto estimado do ano, a alíquota efetiva e uma tabela ano a ano. |
| **Prompt store** | Seus prompts por tag, clique em um para copiá-lo. |
| **Resources** | Favoritos de uma categoria escolhida. |
| **Agent** | Pergunte ao assistente: escolha modelo e ferramentas, envie, e caia na conversa. |
| **Agent activity** | Conversas recentes e consumo de tokens. |

Os widgets Fundamentals e Quant se atualizam a cada cinco minutos enquanto a página está visível. Eles mantêm o resultado anterior durante uma atualização e explicam uma atualização que falhou. O Fundamentals lê apenas snapshots armazenados; carregue ou atualize dados faltantes na página do módulo correspondente.

Nas **configurações do widget**, escolha um conjunto de dados Quant ou um basket de dois a vinte conjuntos compatíveis. Os membros de um basket devem compartilhar um timeframe; os membros intradiários também devem compartilhar um provedor. O risco oferece confiança de VaR histórico de 90%, 95% ou 99%, a sazonalidade oferece retornos, volatilidade, volume ou amplitude da barra, e os cartões de volatilidade e regime expõem sua janela ou número de estados. Cada análise exibe seu histórico real e o tamanho da amostra.

As configurações do Fundamentals permitem escolher e ordenar as séries de um quadro macro, escolher até quatro métricas de empresa ou colunas de tabela, selecionar empresas pares e filtrar registros e resultados para as empresas seguidas. O TTM da demonstração soma quatro trimestres consecutivos e está disponível para linhas de resultado e fluxo de caixa; os valores do balanço continuam sendo observações de fim de período. Uma comparação ano contra ano exige o mesmo período fiscal no ano anterior e uma base de comparação positiva.

Passe o mouse, foque ou toque em uma célula de heatmap para inspecionar o valor e a contagem de amostras. As células ausentes são hachuradas, distintas de um zero medido. Cartões estreitos mostram resumos de sazonalidade mensal ou as correlações de pares mais fortes. Os links do widget abrem a aba relevante do módulo com a série, conjunto de dados ou basket selecionado.

Em telas de até 767px de largura, os cartões do dashboard se empilham em uma única coluna, e no modo de edição os botões **Mover para cima** / **Mover para baixo** substituem o arrastar. O arranjo salvo continua disponível em telas maiores.

## Busca global

A caixa de busca na barra superior, focada de qualquer lugar com <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, ou simplesmente <kbd>/</kbd> quando você não está digitando em um campo.

Por padrão ela encontra **nomes de módulos**, **seções de Configurações** e entradas de **Resources**. O **botão de camadas** ao lado da caixa a amplia para o seu conteúdo: páginas do Editor, Goals, eventos do Calendar, ToDos, Routines, Reminders, Prompts, assuntos de e-mail, Community Docs, e empresas, séries e documentos do Fundamentals.

Duas coisas que ela deliberadamente não faz: encontra **apenas títulos e nomes, nunca o corpo**, e só pesquisa módulos que você instalou. Os resultados voltam agrupados por tipo, com correspondências de prefixo primeiro; <kbd>↑</kbd>/<kbd>↓</kbd> e <kbd>Enter</kbd> navegam por eles.

## Notificações

O sino na barra superior mostra uma contagem de não lidas e abre a **caixa de notificações**, onde os lembretes do [RemindMe](/pt/modules/productivity#remindme) chegam quando disparam, junto com as notificações de outros módulos: workflows do Automator, [webhooks](/pt/modules/productivity#webhooks) de entrada, tarefas em segundo plano como as verificações da caixa de e-mail, os downloads de dados e as sincronizações da corretora, e eventos de segurança como um login a partir de um endereço que esta conta nunca usou. Uma notificação que dispara enquanto você está no app também desliza como um banner.

A entrega por **email, Telegram, Slack ou Discord** passa pelos [canais de notificação](/pt/config/settings#notifications) compartilhados nas Configurações, onde você também decide quais módulos podem enviar para cada um. A caixa em si está sempre ligada e não precisa de configuração.
