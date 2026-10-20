# Dashboard e navegação

A tela inicial do app, mais as duas coisas que ficam acima de todo módulo: a caixa de busca e a caixa de notificações.

## Páginas do dashboard

O dashboard abre em uma página **Módulos** integrada: um bloco por módulo instalado, reconstruído automaticamente conforme você instala e desconecta. Ela nunca é editada nem excluída; apenas reflete o que você tem.

Além dela, você cria **suas próprias páginas**. Cada uma tem um nome, uma descrição opcional e uma **tag** curta exibida no seu chip. Uma página é a **padrão**: aquela em que o dashboard abre, e a que o chip aparece primeiro.

Use-as do jeito que um dia de trading se divide: uma página *Manhã* com o feed de notícias, o calendário econômico e o checklist da rotina; uma página *Posições* com a carteira e a watchlist; uma página *Admin* com tarefas e cronômetros.

## Editando um layout

**Editar layout** transforma uma página em uma grade de linhas sobre 12 colunas. No modo de edição você pode:

- **adicionar linhas** e soltar nelas **blocos de módulo** (um link para um módulo, e o mesmo módulo pode aparecer em quantas páginas quiser) ou **widgets**;
- **redimensionar** qualquer bloco pela largura em colunas, e arrastar blocos entre linhas;
- definir a **predefinição de altura** de um widget (compacta, padrão ou alta) e abrir sua **configuração** (a engrenagem no bloco);
- inserir **linhas espaçadoras** para respirar entre os blocos.

Os blocos são links, não cópias: removê-los de uma página nunca toca o módulo nem seus dados.

## Widgets

Um widget é uma prévia ao vivo e interativa de um módulo: ele lê e escreve pela API do próprio módulo, então o que você faz no widget é real. Widgets cujo módulo não está instalado simplesmente não são oferecidos.

| Widget | O que ele faz |
|---|---|
| **Free text** | Uma nota ou título que você mesmo escreve, markdown-lite. |
| **News feed** | Últimos itens de um feed escolhido, em lista ou grade. |
| **Mailbox** | Os e-mails não lidos mais recentes, os mais novos primeiro. |
| **Time tracker** | Inicie/pare um cronômetro de projeto sem sair da página. |
| **Quick trade** | Escolha uma categoria + modelo e abra o formulário de adicionar trade. |
| **Goals** | Uma lista curta de metas com progresso; adicione uma na hora. |
| **ToDo** | Tarefas abertas, marcáveis ali mesmo. |
| **Trading routine** | O checklist de hoje, marcável ali mesmo. |
| **Mindset** | O check-in do dia. |
| **Reminder** | Um formulário rápido de adicionar lembrete. |
| **Calendar** | Hoje e esta semana num relance. |
| **Economic calendar** | Próximos eventos macro, compactados. |
| **Portfolio** | Um resumo da carteira com valor ao vivo. |
| **Subscriptions** | Gasto recorrente mensal, depois o que renova a seguir. |
| **Net worth** | Patrimônio líquido atual, sua variação numa janela que você define e um sparkline. |
| **Watchlist** | Cotações ao vivo de uma lista escolhida: preço, variação de 24h e 7d. |
| **Fundamentals** | Séries e quadros macro, um snapshot de empresa, uma linha de demonstração por trimestre, ano ou TTM, empresas ordenáveis, registros filtrados, próximos resultados e valuation frente a pares armazenados. Lido de dados armazenados sem gastar cota do provedor. |
| **Quant** | Conjuntos de dados e backtests disponíveis, risco e drawdown de um ativo, correlação, sazonalidade, volatilidade realizada frente ao seu intervalo histórico e o regime de mercado estimado. |
| **Prompt store** | Seus prompts por tag, clique em um para copiá-lo. |
| **Resources** | Favoritos de uma categoria escolhida. |
| **Agent** | Pergunte ao assistente: escolha modelo e ferramentas, envie, e caia na conversa. |

Os widgets Fundamentals e Quant se atualizam a cada cinco minutos enquanto a página está visível. Eles mantêm o resultado anterior durante uma atualização e explicam uma atualização que falhou. O Fundamentals lê apenas snapshots armazenados; carregue ou atualize dados faltantes na página do módulo correspondente.

Nas **configurações do widget**, escolha um conjunto de dados Quant ou um basket de dois a vinte conjuntos compatíveis. Os membros de um basket devem compartilhar um timeframe; os membros intradiários também devem compartilhar um provedor. O risco oferece confiança de VaR histórico de 90%, 95% ou 99%, a sazonalidade oferece retornos, volatilidade, volume ou amplitude da barra, e os cartões de volatilidade e regime expõem sua janela ou número de estados. Cada análise exibe seu histórico real e o tamanho da amostra.

As configurações do Fundamentals permitem escolher e ordenar as séries de um quadro macro, escolher até quatro métricas de empresa ou colunas de tabela, selecionar empresas pares e filtrar registros e resultados para as empresas seguidas. O TTM da demonstração soma quatro trimestres consecutivos e está disponível para linhas de resultado e fluxo de caixa; os valores do balanço continuam sendo observações de fim de período. Uma comparação ano contra ano exige o mesmo período fiscal no ano anterior e uma base de comparação positiva.

Passe o mouse, foque ou toque em uma célula de heatmap para inspecionar o valor e a contagem de amostras. As células ausentes são hachuradas, distintas de um zero medido. Cartões estreitos mostram resumos de sazonalidade mensal ou as correlações de pares mais fortes. Os links do widget abrem a aba relevante do módulo com a série, conjunto de dados ou basket selecionado.

Em telas de celular de até 480px de largura, os cartões do dashboard se empilham em uma única coluna. O arranjo salvo continua disponível em telas maiores e no editor de layout.

## Busca global

A caixa de busca na barra superior, focada de qualquer lugar com <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd>, ou simplesmente <kbd>/</kbd> quando você não está digitando em um campo.

Por padrão ela encontra **nomes de módulos**, **seções de Configurações** e entradas de **Resources**. O **botão de camadas** ao lado da caixa a amplia para o seu conteúdo: páginas do Editor, Goals, eventos do Calendar, ToDos, Routines, Reminders, Prompts e Community Docs.

Duas coisas que ela deliberadamente não faz: encontra **apenas títulos e nomes, nunca o corpo**, e só pesquisa módulos que você instalou. Os resultados voltam agrupados por tipo, com correspondências de prefixo primeiro; <kbd>↑</kbd>/<kbd>↓</kbd> e <kbd>Enter</kbd> navegam por eles.

## Notificações

O sino na barra superior mostra uma contagem de não lidas e abre a **caixa de notificações**, onde os lembretes do [RemindMe](/pt/modules/productivity#remindme) chegam quando disparam, junto com tudo que um [webhook](/pt/modules/productivity#webhooks) de entrada redireciona para lá. Uma notificação que dispara enquanto você está no app também desliza como um banner.

A entrega por **email, Telegram, Slack ou Discord** passa pelos [canais de notificação](/pt/config/settings#notifications) compartilhados nas Configurações, onde você também decide quais módulos podem enviar para cada um. A caixa em si está sempre ligada e não precisa de configuração.
