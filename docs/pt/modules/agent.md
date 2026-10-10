# Agent

Um **assistente de chat com IA** integrado: um painel de chat dentro do app que também pode agir sobre os seus dados do OpenTraderWorld. Abra-o pelo módulo **Agent**, pelo atalho de faíscas na barra superior (ao lado da busca), ou pelo [botão flutuante](#o-assistente-flutuante) no canto de todas as páginas.

O assistente é **bring-your-own-provider**: nada vem ativado nem com um fornecedor padrão até você adicionar um provedor e uma chave seus.

::: tip Dois "AI agents" diferentes
Esta página trata do **assistente de chat que vive no app** e conversa com um provedor que *você* configura. Isso não é o mesmo que a página de [AI agents (MCP)](/pt/config/ai-agents), que trata de agents *externos* se conectando **ao** OpenTraderWorld pelo servidor MCP. O assistente de chat pode *usar* esse mesmo gateway para alcançar seus dados: veja [Ferramentas sobre os seus dados](#ferramentas-sobre-os-seus-dados) abaixo.
:::

## Adicione um provedor

Em **Configurações** (a engrenagem na barra lateral do chat) → **Geral**, adicione um ou mais provedores. Dois formatos de comunicação são suportados:

- **Anthropic**: a Claude Messages API.
- **Compatível com OpenAI**, ou seja, qualquer endpoint que fale o formato de chat da OpenAI: OpenRouter, OpenAI, DeepSeek, Moonshot, Groq, Mistral, o endpoint de compatibilidade do Gemini, um proxy local e assim por diante.

Cada provedor tem sua própria **base URL** (somente compatível com OpenAI), **chave de API** e **modelo padrão**. A chave é **somente escrita**: é criptografada em repouso com a chave mestra do app e nunca mais é mostrada depois de salva. Deixe o campo da chave em branco ao editar para manter a atual.

Um provedor pode ser desativado sem ser excluído. O assistente só fica "pronto" quando tem um provedor ativado com uma chave e um modelo.

## Configure o assistente

No mesmo painel de configurações você define o **prompt de sistema**, o **provedor / modelo** ativo, **max tokens** e **temperature**. Um campo **Parâmetros avançados (JSON)** repassa qualquer campo extra de requisição literalmente ao provedor. Defina um valor como `null` para *remover* uma chave que o app enviaria de outra forma (por exemplo `max_completion_tokens` para modelos OpenAI mais novos, ou para descartar `stream_options`).

## Chat

- As respostas chegam em **streaming ao vivo** e são renderizadas como Markdown. Modelos que expõem o raciocínio ganham uma dobra **Thinking** opcional.
- As conversas são salvas na **barra lateral**: nova, selecionar, renomear, excluir e **exportar para Markdown** com um clique.
- **Despejar no Editor** transforma uma conversa em uma página do Editor: escolha uma pasta (ou crie uma) e um nome de arquivo, depois **Salvar** para ficar no agent ou **Salvar e abrir**. Cada mensagem mantém sua data e autor (Você, ou a persona e o modelo), o raciocínio fica numa citação acima da resposta, e **Incluir detalhes** adiciona chamadas de ferramentas e contagens de tokens.
- Você pode **interromper** uma execução no meio do streaming.
- O cabeçalho do chat mostra uma **contagem de tokens** acumulada (entrada + saída) da conversa, para você ver quanto um thread está custando.
- Um botão de **modo largo** remove o limite de largura de leitura. O thread é dimensionado para prosa, que é o formato errado para as tabelas que o assistente produz: um ledger de tentativas ou um detalhamento de estatísticas precisa de espaço. A escolha fica por navegador.
- Falhas do provedor aparecem como uma frase simples em um banner dispensável: uma chave rejeitada, um limite de taxa (com o retry-after do provedor quando ele envia), um modelo ou base URL errado, uma recusa do filtro de conteúdo, ou uma resposta cortada no limite de tokens.

### Limites do chat

Um chat pode ter um teto de **tokens de saída**, de **dólares**, ou ambos. Defina os padrões em **Configurações → Geral → Limites de novos chats**; todo novo chat começa com eles, e alterá-los depois não afeta os chats existentes. Deixe um campo vazio para não ter limite.

A barra fina ao lado do botão do Prompt Store na caixa de mensagem se enche de baixo para cima conforme o chat gasta. Passe o mouse para ver os números, clique para alterar os limites deste chat. Quando um limite é atingido, a execução para antes do próximo passo pago e o chat não aceita novas mensagens até você aumentar ou remover o limite, o que pode ser feito pela mesma barra. Enviar no limite mostra um aviso com dois atalhos: **Atualizar limite** abre esse editor, **Novo chat** abre um chat novo com a mesma persona e mantém o que você digitou.

O limite em dólares precisa de um provedor que devolva o preço a cada resposta (o OpenRouter devolve, a Anthropic não). Sem isso o gasto aparece como "nenhum preço retornado" e só o limite de tokens se aplica.

### Troque de provedor ou modelo por chat

O cabeçalho do chat mostra o **provedor · modelo** ativo. Abri-lo dá uma **tela de seleção completa**: os provedores de um lado e a **lista de modelos ao vivo** do provedor selecionado do outro, pesquisável, consultada no servidor para que sua chave nunca chegue ao navegador. Texto livre ainda funciona para proxies que não expõem uma lista. Nada muda até você confirmar com **Usar este modelo**, então navegar pela lista não custa nada, e um único botão devolve a conversa ao padrão herdado.

A escolha pertence a **essa conversa**, não ao assistente: um modelo barato e rápido pode revisar seu journal em uma aba enquanto o modelo de raciocínio mais forte discute um backtest em outra. Uma conversa que não fez escolha própria herda a da persona, depois o que você definiu em **Configurações → Geral**. Um ponto no seletor marca as que rodam com algo próprio, e um clique as devolve à configuração herdada. Trocar de provedor limpa o id do modelo junto, já que um nome de modelo só faz sentido para o fornecedor de onde veio.

### Envie um prompt salvo

O compositor pode puxar do seu [Prompt Store](/pt/modules/productivity#prompt-store) em vez de redigitar um prompt que você guarda. O seletor lista seus prompts com busca por nome, tags e corpo, mostra o selecionado por inteiro e o joga no compositor em **Inserir prompt** (ou com um duplo clique na linha), onde você ainda pode editá-lo antes de enviar.

## O assistente flutuante

Um botão no **canto inferior direito de todas as páginas** abre um chat compacto sobre suas conversas existentes, sem sair do que você estava fazendo. É o mesmo assistente, não um paralelo: mesmas conversas, personas, provedores e ferramentas, então um thread iniciado no canto está na página Agent depois e vice-versa. Persona, modelo e ferramentas ficam em uma linha sob o título, e os seletores de modelo e prompt abrem como uma visão sobre o thread em vez de um diálogo.

Ele também sabe **em que página você está**. Cada mensagem carrega o módulo atual, e quando o token da conversa concede esse módulo, sua lista de endpoints é carregada no prompt de antemão, então um pedido feito a partir de Historical Data não gasta sua primeira rodada de ferramentas descobrindo onde olhar. Todo o resto fica a uma consulta de distância, e uma página com a qual o assistente não tem o que fazer (Configurações, o dashboard) não envia nada.

## Memória e skills

Duas abas nas configurações permitem que o assistente carregue conhecimento entre conversas:

- **Memória**: fatos pequenos e duráveis (uma preferência, um detalhe estável) que persistem entre chats. Apenas o **índice** (slug + descrição de uma linha) vai no prompt; o conteúdo completo é puxado sob demanda. Você navega, edita e exclui cada memória, nada fica oculto. Cada memória registra **qual persona a escreveu**, mostrado tanto no gerenciador quanto no índice que o assistente lê: a memória é um só armazenamento compartilhado, então uma restrição que o Day Trader escreveu seria lida pelo Analyst como sua. O assistente também pode podar memórias quando o armazenamento enche, e não consegue sobrescrever em silêncio uma que você escreveu à mão.
- **Skills**: conjuntos reutilizáveis de instruções em Markdown que você define. O **nome + descrição** de uma skill estão sempre no contexto; o assistente carrega o corpo completo sob demanda quando uma tarefa pede. Ative/desative cada skill individualmente.

Conversas longas também recebem um **resumo contínuo**: quando um chat cresce, as interações mais antigas são comprimidas em um resumo corrente para que o thread continue barato, mantendo literalmente apenas as mensagens mais recentes.

## Personas

Uma **persona** é uma versão do assistente moldada por um papel: um prompt de sistema com uma postura e um limite explícito de recusa, mais uma **prateleira de skills** curada. Cinco vêm integradas (**Quant**, **Portfolio Manager**, **Day Trader**, **Researcher**, **Financial Analyst**) e você escolhe uma ao abrir uma conversa, no seletor de persona no cabeçalho do chat.

A ideia é a estreiteza. Um assistente genérico com duzentos endpoints é pior em qualquer tarefa específica do que um que conhece alguns a fundo e recusa o resto. O Quant não reporta um backtest sem a contagem de tentativas e um número fora da amostra; o Day Trader não nomeia uma entrada; o Analyst informa os fundamentos que *não conseguiu* obter em vez de preenchê-los.

### O que uma persona não é

**Uma persona não é um conjunto de permissões.** O que o assistente alcança é o token MCP da conversa: com escopo por módulo, definido por você, idêntico seja qual for a persona falando. Trocar de persona estreita a *postura e a prateleira*, nunca o acesso aos dados. Para mudar o que ele pode tocar, mude o token.

### Trocando no meio da conversa

Você pode trocar de persona no meio do thread. Vale **a partir da próxima mensagem**, e um marcador aparece na transcrição registrando a passagem: as interações acima dele foram produzidas pela persona anterior e continuam atribuídas a ela.

### Editando as suas

**Configurações → Personas** lista cada persona com a prateleira que ela realmente receberá. De lá você pode:

- **criar** uma do zero, ou **duplicar** uma integrada e reescrever a cópia;
- editar o **prompt**, a **prateleira** e se ela **aprova escritas automaticamente**;
- **redefinir** uma integrada ao original (suas edições nela são perdidas, nada mais é tocado);
- **excluir** uma que você criou. Suas conversas são **mantidas**: elas passam para o assistente padrão, e cada transcrição recebe uma nota dizendo isso. As integradas não podem ser excluídas: o app as recria no próximo reinício, então excluir só pareceria funcionar.

As skills **não** são criadas aqui. Há um catálogo, gerenciado na aba Skills, e as personas escolhem dele. Isso significa que editar o corpo de uma skill a altera para toda persona que a tem, e a lista de skills mostra quantas, então a edição nunca é às cegas.

### Exportar e importar

Qualquer persona exporta como um **arquivo JSON com os corpos das skills embutidos**, então um arquivo a reproduz em outra máquina. Você também pode exportar uma única skill, ou a prateleira inteira de uma vez. A importação aceita qualquer um dos formatos; um nome existente é ignorado em vez de sobrescrito. Não há portão de revisão: esta é a sua máquina, e o que você carrega no seu próprio assistente é decisão sua.

O gerenciamento de personas e skills é deliberadamente **ausente do catálogo MCP**: nenhum agent, e nenhum conteúdo que um agent leia, pode editar uma persona ou ampliar uma prateleira.

## Confirmação de escrita

Quando o assistente quer alterar seus dados, a execução **pausa** e mostra a chamada exata (método, caminho e corpo) com Aprovar e Recusar. Nada é gravado até você responder, e recusar é informado ao modelo como uma recusa e não como um erro a contornar. Se você se afastar, a espera expira e a escrita não acontece.

Uma persona pode ser configurada para **aprovar escritas automaticamente**, o que pula o pedido para mudanças comuns. **Exclusões sempre perguntam**, qualquer que seja essa configuração: marcar a caixa foi uma decisão sobre escritas de rotina, não uma permissão para apagar um journal.

Endpoints que só *calculam* (um backtest, um Sharpe ratio, um Monte-Carlo) não pedem confirmação. Eles não mudam nada de que você sentiria falta, e um diálogo de confirmação a cada cálculo é como as pessoas aprendem a clicar em Aprovar sem ler.

## Ferramentas sobre os seus dados

Anexe um **token MCP** a uma conversa e o assistente pode ler e atualizar seus módulos pelo [mesmo gateway em processo](/pt/config/ai-agents) que os clientes MCP externos usam. Os **níveis de permissão por módulo** do token (Leitura / Leitura+escrita / Total, definidos em **Configurações → MCP**) se aplicam **diretamente**: o token *é* o envelope de permissões; não há um segundo portão do lado do agent. Operações de configurações, segredos, rede e apagar dados nunca são expostas, e não há acesso a shell nem a sistema de arquivos por construção.

As chamadas de ferramenta aparecem inline como **chips recolhíveis** mostrando os argumentos e o resultado. Uma execução é limitada a 15 rodadas de ferramentas, e cada conversa carrega um **orçamento de simulação**, já que backtests e varreduras são a única coisa que um assistente pode gastar sem limite, então quando o orçamento acaba ele é instruído a parar de buscar e relatar o que tem, incluindo quantas tentativas rodou.

### Escrevendo a sua própria skill

Uma skill é um procedimento, não um manual. O formato que funciona:

- uma **descrição** que diga *quando* recorrer a ela, já que essa linha é a chave de recuperação e vai em todo prompt, então "Use quando o usuário propuser uma estratégia" é melhor que "Sobre estratégias";
- um **corpo** que nomeie os endpoints exatos, passo a passo, com as formas específicas como a tarefa dá errado neste app;
- um passo de **verificação**: como provar o resultado antes de relatá-lo;
- um **formato de relatório**: o que a resposta deve conter.

Mantenha o corpo curto. Ele entra inteiro na janela de contexto quando carregado, então um longo expulsa a tarefa que ele deveria ajudar. O editor avisa quando passa de cerca de duas mil palavras.

### Ferramentas por conversa

Cada conversa carrega **o seu próprio** token MCP (o token definido nas configurações é só o padrão para novas conversas), alternável por um **menu de ferramentas** no cabeçalho do chat. Duas conversas podem rodar com escopos de dados diferentes lado a lado. O menu:

- tem uma **caixa de busca** para filtrar tokens e servidores externos por nome;
- avisa quando o token selecionado concede **escrita/exclusão**;
- oferece ações inline para **adicionar um servidor MCP** e atalhos para **Configurações → MCP** (criar/gerenciar tokens) e para a **loja MCP**.

## Loja MCP: conecte plataformas externas

**Agent → Gerenciar servidores** é uma seção de página inteira para adicionar servidores MCP remotos para que o assistente alcance plataformas externas:

- um **catálogo curado** de servidores conhecidos (DeepWiki, Context7, GitHub, Hugging Face, traga sua própria chave), mais **servidores personalizados** por URL;
- **somente Streamable-HTTP**: nada nunca executa localmente;
- os valores de autenticação são **criptografados em repouso e somente escrita**;
- um botão **Testar** conecta e lista as ferramentas do servidor;
- ative um servidor por conversa no menu de ferramentas.

As ferramentas externas têm **namespace** (por exemplo `deepwiki__ask_question`) e são rotuladas com seu servidor, as chamadas têm tempo limitado, e um servidor inacessível **degrada para um aviso** em vez de bloquear o chat.

::: warning Conteúdo externo não é confiável
Um servidor MCP externo vê sua conversa, e o que ele devolve é conteúdo de terceiros. Combinar um servidor externo com um token que concede **acesso de escrita** aos seus dados significa que conteúdo injetado poderia tentar disparar alterações, e o menu de ferramentas avisa quando essa combinação está ativa. Adicione apenas servidores em que você confia e fique de olho nos chips de chamadas de ferramentas.
:::
