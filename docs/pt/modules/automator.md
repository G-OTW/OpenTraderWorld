# Automator

*Workflows que rodam o seu próprio app.* Um workflow é uma lista de passos: chamar um endpoint da API do seu OpenTraderWorld, chamar qualquer URL fora dela, fazer uma pergunta ao seu provedor de IA, remodelar a resposta, notificar a si mesmo. Execute à mão ou em agenda.

Usos típicos: um resumo pré-mercado de segunda-feira que lê os eventos econômicos da semana e suas watchlists e envia uma mensagem no Telegram; uma exportação noturna do journal para um serviço externo; um webhook de preço distribuído em uma notificação; um resumo semanal escrito pelo assistente a partir dos seus próprios números.

O módulo fica em **/automator** com quatro seções: **Workflows**, **Agendamentos**, **Agenda** (um calendário das execuções futuras), **Execuções**.

## A grade

O editor é uma grade, não um canvas: sem fios para desenhar.

- **Os passos rodam de cima para baixo, as tarefas dentro de um passo rodam da esquerda para a direita.** Tudo roda uma coisa depois da outra. Um passo agrupa as tarefas que pertencem ao mesmo momento do workflow, não as inicia juntas.
- Arraste um bloco da paleta para um **vão**: o vão entre dois passos abre um novo passo, o vão entre duas tarefas a coloca dentro desse passo. Todo alvo de soltura válido é destacado antes de você soltar.
- Uma tarefa só pode ler o que rodou **antes** dela. Mova um cartão e suas referências são verificadas de novo no próximo salvamento.
- **Autosave** 1,2 s após cada edição, **Ctrl/Cmd+Z** para desfazer. Um bloco ainda sendo preenchido é mantido como **rascunho**: nunca roda e nenhum agendamento o pega até ele validar.

## Blocos

| Bloco | O que ele faz |
|---|---|
| **App call** | Um endpoint da sua própria API, executado em processo. A paleta lista os endpoints que um workflow pode alcançar; clique em um e o bloco chega já apontado para ele, com o formato esperado do corpo à mão. |
| **Web call** | Qualquer URL fora do app: método, query, cabeçalhos, corpo JSON/texto/form, resposta lida como JSON, texto, CSV ou binário, com limite de tamanho da resposta. |
| **AI step** | Um turno de modelo, sem conversa em volta. Escolha uma persona e um prompt armazenado ou escreva as instruções; peça um **objeto JSON** quando o próximo bloco precisa ler campos e não prosa. Ele chama seu provedor e custa tokens a cada execução. |
| **Transform** | Remodele o que veio antes: `pick` um valor, `set` um objeto, `format` uma string, `csv_parse` um CSV, `join` uma lista em uma linha. |
| **Notify** | Uma notificação no app e seus [canais de notificação](/pt/config/settings#notifications) (email, Telegram, Slack, Discord). Nada selecionado significa todo canal ativado concedido ao Automator. |
| **Wait** | Pausa a execução. **Parar** ainda responde durante a espera. |

## Passando dados entre blocos

Cada bloco tem um **id**, mostrado no topo do seu editor. Um bloco posterior lê seu resultado com uma expressão:

```
{{steps.http1.output}}                     the whole answer
{{steps.http1.output.items.0.name}}        one field of it
{{steps.http1.status}}                     ok | simulated | failed | skipped
{{steps.http1.error}}                      the failure message, empty on success
{{run.started_at}} {{run.trigger}} {{workflow.name}} {{input.key}}
```

Os filtros se encadeiam depois de um pipe: `json`, `upper`, `lower`, `trim`, `round:2`, `date:"DD/MM/YYYY"`, `default:"n/a"`. Só `default` salva um valor que não existe.

Isto **não é uma linguagem**: sem aritmética, sem código. Toda referência é verificada **quando o grafo é salvo**, contra os blocos que realmente o precedem, então um vínculo quebrado é recusado no editor e não às três da manhã.

## Segredos

Uma senha ou chave de API pertence ao [Cofre](/pt/config/settings#vault), nunca digitada em um campo. Referencie-a com <code v-pre>{{vault.myvault.mykey}}</code> em um cabeçalho, um valor de query ou um corpo de requisição, os únicos lugares onde é aceita (nunca em uma URL), e o valor resolvido é apagado do histórico de execuções. Um valor de query ainda chega aos logs do site que você chama, então prefira um cabeçalho.

## Permissões

- **App call precisa de um token de acesso.** Escolha um nas **Configurações** do workflow; os tokens são criados em [Configurações → AI agents](/pt/config/ai-agents). Sem token não há chamada interna nenhuma, e um workflow só alcança o que seu token concede, dentro da mesma lista de permissões que o gateway MCP usa.
- **Web call recusa a sua própria rede.** Endereços loopback, privados, CGNAT e link-local são rejeitados, a menos que o bloco permita explicitamente alvos internos. O host é resolvido primeiro e a conexão é fixada no endereço que foi verificado, e cada salto de redirecionamento é verificado de novo.
- **Notify precisa de uma concessão.** Um canal só se oferece aqui depois que o Automator recebeu a concessão dele em Configurações → Notificações.

## Testando, depois executando

- **Execução de teste** realiza as leituras e informa o que uma escrita ou envio *teria* feito, então nada sai do app. Um bloco que lê um valor que só uma escrita simulada poderia ter produzido é informado como **simulado**, em vez de falhar um teste que uma execução real passaria.
- **Testar este bloco** roda um bloco sozinho, mesmas regras.
- **Executar agora** faz de verdade. **Parar** é verificado entre blocos e durante uma espera; uma execução além do **limite de execução** do workflow é encerrada como timeout.

## Execuções

Cada execução guarda **uma linha por bloco**: o que foi enviado, o que voltou, o status e o tempo, então um workflow que falhou às 3 da manhã nomeia o bloco e mostra o payload. Uma saída acima de 256 KB é guardada à parte e buscada sob demanda. O histórico é aparado para as últimas 50 execuções por workflow (10 para execuções de teste).

## Agendamentos

A cada N minutos, diariamente, em alguns dias da semana, mensalmente, ou uma vez em um momento dado, cada um com seu próprio **fuso horário IANA**, então uma regra definida em Europe/Paris segue Paris e não o servidor.

- **Sem sobreposição**: se uma execução ainda está rodando quando a próxima ocorrência vence, essa ocorrência é **descartada**, não enfileirada.
- **Recuperar** (opcional): se o app estava fora do ar no horário planejado, execute uma vez na inicialização.
- O horário de verão é resolvido, não ignorado: uma hora local pulada roda no fim da lacuna, uma repetida roda uma vez. Uma regra mensal definida além do fim de um mês curto roda no último dia.
- Um agendamento pode ser **fixado em uma versão** do workflow. Salvar um novo grafo pergunta se os agendamentos fixados devem segui-lo; o autosave nunca reaponta um sozinho.
- **Pausar / retomar** a partir do cartão do workflow ou da lista de Agendamentos. Um workflow desativado nunca é iniciado por um agendamento, embora executá-lo à mão ainda funcione.

::: warning Um agendamento com escrita escreve
Um App call apontado para um endpoint que altera seus dados fará isso a cada execução, sem supervisão. O editor sinaliza esses endpoints; teste o workflow antes de agendá-lo.
:::

## Deixar um agent montar um workflow

Compor um grafo é a coisa mais difícil que este módulo pede de você, e é exatamente o tipo
de trabalho em que um [AI agent](/pt/config/ai-agents) é bom. Então um agent com um token com a
permissão **Automator** pode ler seus workflows, criar um, escrever seu grafo e testá-lo.

O que ele não pode fazer é colocar um em operação. A divisão é deliberada:

- Um grafo que um agent salva chega como **rascunho**, nunca como o grafo que roda. Abra o
  workflow, leia o que ele escreveu e Salve para adotá-lo. Até você fazer isso, nada muda: um
  workflow já em agendamento continua rodando a versão que você salvou.
- Um agent não pode anexar o **token de acesso** de um workflow. Esse envelope é o que transforma um grafo
  em permissões, então você o concede à mão, depois de ler o grafo que ele vai rodar.
- Um agent não pode **executar** um workflow, restaurar uma revisão, excluir um, nem mexer em um agendamento.
- Suas **execuções de teste** são seladas: notificações e chamadas externas são forçadas a desligado, e sem
  envelope anexado um App call falha fechado. Um teste verifica se o grafo roda, se suas
  expressões se resolvem e se seus transforms fazem o que dizem, sem alcançar nada. Um
  workflow que já carrega um token é recusado: esse você testa por conta própria.

O motivo da linha é que um workflow roda sob **o seu próprio** token, não o de quem chama.
Um agent capaz de escrever um grafo e também iniciá-lo herdaria tudo o que esse token concede,
sejam quais forem as suas próprias permissões. Escrever e armar são duas concessões, e só uma delas
é sua para delegar.

O Automator é desativado no [modo demo](/pt/guide/demo).
