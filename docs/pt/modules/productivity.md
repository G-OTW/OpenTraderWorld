# Notas e organização

Os módulos do dia a dia: documentos, tarefas, metas, calendário, lembretes, mais alguns feitos para a disciplina de um trader.

## Editor {#editor}

Um editor de documentos rico no estilo Notion. Os documentos vivem em uma árvore de pastas; digite `/` para o menu de blocos.

- **Blocos**: títulos, listas, listas de tarefas, citações, blocos de código, divisores, links, imagens (enviadas ou por URL), cor do texto, destaque, tamanho da fonte, largura normal ou total. Salva automaticamente.
- **Bancos de dados**: um tipo de documento com colunas tipadas (texto, seleção, URL…), visualizável como **tabela**, **kanban** (agrupado por uma coluna de seleção) ou **galeria** (com uma coluna de imagem de capa). Arraste para reordenar linhas e colunas.
- **Enviar para publicação**: envie um documento à fila de revisão do [Community Docs](/pt/modules/news-research#community-docs), com a formatação preservada, idioma, categorias e crédito de autor opcional.
- **Versões** (opcional): ligue o versionamento em [Configurações → Versões](/pt/config/settings#versioning), depois por página ou banco de dados no seu menu de histórico. **Salvar versão** armazena um snapshot com sua data e uma nota opcional; abra uma para vê-la somente leitura, restaure-a (o estado restaurado é salvo como uma nova versão, anotada com a data da versão restaurada) ou exclua-a. O histórico abre em uma janela com busca em notas e datas; a versão que corresponde ao arquivo atual é marcada. As notas são limitadas a 500 caracteres. Imagens e vídeos não são copiados: uma versão aponta para os mesmos uploads. Desligar o versionamento de um arquivo pergunta se você quer manter ou excluir suas versões. Excluir um arquivo exclui suas versões também, após uma confirmação. Agents com acesso de escrita ao Editor podem salvar, restaurar e excluir versões por [MCP](/pt/config/ai-agents).

## ToDo {#todos}

Uma lista de tarefas que não atrapalha: tarefas com prazo, hora, categoria e notas. Filtre por pendentes/concluídas/atrasadas, ordene por prazo; as sinalizações de atrasada/hoje/em breve fazem a cobrança. Um widget do dashboard mostra o que está aberto.

## Goals {#goals}

Metas com **métricas mensuráveis**. Dê a cada meta um prazo, uma categoria e uma ou mais métricas com valor atual, alvo e pontos. Incremente-as conforme avança, e a conclusão da meta segue os pontos. Filtre abertas/atingidas/atrasadas; arraste para ordenar.

## Calendar {#calendar}

Um calendário pessoal (ano/mês/semana/dia) para eventos com categoria, cor, local e notas. O truque dele são as **sobreposições**: ele também pode exibir seus **Reminders**, **ToDos com prazo** e **prazos de Goals**, cada um alternável: um só lugar para ver a semana. Criar um evento também pode criar um lembrete sincronizado na hora de início.

## RemindMe {#remindme}

Lembretes, únicos ou recorrentes (com data de início, data de fim ou contagem máxima), que disparam como **notificações no app** com uma caixa de notificações.

- **Lembretes vinculados**: anexe um lembrete a um item de outro módulo (uma meta, a cobrança de uma assinatura, uma revisão do journal…) e ele leva de volta até ele. A maioria dos módulos tem um botão *Adicionar lembrete* que preenche isso.
- **Canais**: entregue também por **email, Telegram, Slack ou Discord**, escolhidos entre os [canais de notificação](/pt/config/settings#notifications) compartilhados. Um lembrete lista os canais concedidos ao RemindMe; as credenciais e as concessões vivem nas Configurações, uma vez para o app inteiro.

## Webhooks {#webhooks}

Dê a qualquer serviço externo uma URL privada para **enviar alertas por POST ao OpenTraderWorld**: plataformas de gráficos e alertas, notificações de broker, monitores de uptime, scripts, qualquer coisa que dispare uma requisição HTTP. O payload é recebido e roteado para um módulo.

- **URL privada, sem cabeçalhos**: cada endpoint carrega um **token de 256 bits no caminho da URL** (`/api/hooks/<token>`), porque muitos remetentes de alertas não conseguem definir um cabeçalho `Authorization`. Os tokens são armazenados com **hash** e mostrados **uma vez** na criação; buscas falhas são limitadas.
- **Payloads liberais**: envie texto simples ou JSON; o parser aceita nomes de campo soltos, então a maioria dos remetentes funciona sem formatação especial.
- **Roteamento**: cada endpoint redireciona seu payload para um módulo de destino. O destino da v1 é o **[RemindMe](#remindme)**: um payload recebido vira uma notificação no app, enviada também aos seus canais ativados (email/Telegram/Slack/Discord).
- **Log de entregas**: as entregas mais recentes por endpoint são mantidas para você confirmar que um remetente está chegando e ver o que ele enviou.

Gerencie os endpoints em **/webhooks**.

::: warning O remetente precisa conseguir alcançar você
Um webhook só é útil se o serviço remetente consegue abrir uma conexão com o seu host. No modo de rede `local` (e LAN simples) nada de fora consegue, e a página avisa quando o modo atual não é acessível pela internet. Mude o modo em [Configurações → Rede](/pt/config/network), ou aponte um túnel (por exemplo Cloudflare Tunnel) para o host e mantenha o app privado no resto.
:::

## Trading Routines {#routines}

**Checklists de sessão** recorrentes devidos nos dias da semana que você escolher: preparação pré-mercado, disciplina durante a sessão, revisão pós-mercado. Marque itens por dia, navegue por dias passados e acompanhe a **faixa de consistência de 14 dias** para ver se você realmente segue seu processo. Checklists iniciais estão incluídos.

## Time Tracker {#time}

Projetos com **cronômetros** de iniciar/parar (ou intervalos adicionados manualmente), **orçamentos de tempo** opcionais com avisos de estouro, datas de fim planejadas e um **valor por hora** para dar valor ao tempo. A aba **Resumo** plota as horas registradas por dia/semana/mês, filtrável por projeto e categoria. Se um cronômetro ficou rodando enquanto o app estava fechado, ele pergunta se você quer manter ou reverter esse tempo.

## Mindset {#mindset}

Um **check-in** diário para a psicologia do trader. Responda a alguns prompts antes ou depois da sessão: escalas (foco, disciplina), escolhas (calmo / ansioso / FOMO), texto livre. Os prompts são **totalmente personalizáveis**; um conjunto inicial está incluído. A visão **Tendências** plota suas respostas ao longo dos check-ins recentes, e o Histórico permite reler qualquer dia.

## Prompt Store {#prompt-store}

Uma biblioteca para os **prompts de IA** que você reutiliza: resumos de mercado, perguntas de journaling, modelos de pesquisa. Os prompts aparecem como uma grade de vinhetas (nome, tags, último salvamento) com busca em nome, tags e corpo.

- **Tags**: adicione tags livres no editor; filtre a grade com a barra de tags.
- **Avaliar e filtrar**: dê a um prompt um polegar para cima ou para baixo e filtre rapidamente por qualquer um.
- **Histórico de versões**: cada salvamento é mantido; abra o **Histórico** de um prompt para visualizar qualquer revisão anterior e **voltar** a ela (a restauração é salva como uma nova versão, então nada se perde).
- **Duplicar**: bifurque um prompt para criar uma variante.
