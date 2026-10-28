# Controle externo (chat)

Controle o OpenTraderWorld pelo **Telegram, Slack ou Discord**. Uma mensagem que você envia ao seu bot vira uma execução de uma das suas [personas de agent](/pt/modules/agent), respondida no chat, com exatamente o acesso que o token escolhido permite.

**Vem desligado por padrão**, e não adiciona nenhum sistema de permissões próprio: o teto é um [token MCP](/pt/config/ai-agents), o mesmo que o assistente do app usa.

## Nada novo escuta na sua máquina

Os três transportes **discam para fora**: long polling do Telegram, Socket Mode do Slack, o gateway do Discord. Não há URL pública para publicar, porta para abrir nem rota de entrada para atacar, então isto funciona sem alterações na instalação padrão somente em localhost e atrás de NAT.

O canal que você já usa para notificações só consegue **enviar** (a URL de webhook do Slack ou Discord é somente escrita). Receber exige um bot de verdade, então um vínculo guarda a sua própria credencial:

| Plataforma | Credencial a colar | Na plataforma |
|---|---|---|
| **Telegram** | o token do bot do BotFather | nada mais |
| **Slack** | **os dois** tokens, `xapp-…` e `xoxb-…`, separados por um espaço ou uma quebra de linha | Socket Mode ligado, evento `message.im`, escopo `chat:write` |
| **Discord** | o token do bot | o intent de **Mensagens diretas** (o intent privilegiado de conteúdo de mensagem não é necessário para DMs) |

## Configure um

1. **Configurações → Notificações**: crie o canal se você não tiver nenhum. Este é o caminho de resposta.
2. **Configurações → MCP**: crie ou edite um token, defina seus níveis por módulo e marque **Permitir acesso externo**.
3. **Configurações → Controle externo**: **Novo vínculo**, escolha o canal, a persona e esse token, cole a credencial do bot (ou conecte-a do [Cofre](/pt/config/settings#vault)), opcionalmente defina o provedor e o modelo em que os novos chats começam, ative o vínculo.
4. Ligue o interruptor da seção. O vínculo mostra **Conectado** em poucos segundos.
5. **Emparelhar**: clique em *Emparelhar*, depois envie o código de 6 dígitos ao seu bot **a partir da conta que deve ter permissão de controle**. Funciona uma vez e dura o tempo que *Duração do código de emparelhamento* indicar (uma hora por padrão, de 5 minutos a um dia).

Enquanto ninguém estiver emparelhado, o bot não responde a ninguém, nem a você.

## Qual modelo responde

Cada chat carrega seu próprio provedor e modelo, exatamente como uma conversa no app. O vínculo define o **padrão em que um novo chat começa**: um trecho de celular geralmente merece um modelo mais barato e rápido do que a mesma persona usa no navegador. Deixe vazio e o chat herda o da persona.

Altere o padrão no formulário do vínculo. Altere um chat a partir do próprio chat:

- `/provider` lista os provedores configurados, `/provider 2` ou `/provider openrouter` troca este chat para um deles (o que redefine o modelo para o padrão desse provedor, já que um id de modelo pertence a um fornecedor).
- `/model` lista os modelos desse provedor, `/model 3` escolhe pela posição e `/model haiku` escolhe pelo texto quando exatamente um id corresponde.

A escolha fica naquele chat e não move mais nada. `/new` descarta a conversa, então a próxima começa de novo no padrão do vínculo.

## Direitos

O token decide tudo o que uma resposta pode tocar: *nenhum acesso*, *leitura*, *leitura + escrita*, *total* por módulo, exatamente como na [página do MCP](/pt/config/ai-agents#modelo-de-seguranca). A flag `external` não amplia nada, apenas diz que esse envelope pode ser alcançado de fora.

- **Um token por vínculo, um vínculo por canal.** Revogar um token para aquele vínculo e mais nada, e o registro ainda diz por qual caminho uma chamada entrou.
- **Use um token dedicado**, e comece com somente leitura. Você pode ampliá-lo depois sem emparelhar de novo.
- Um token **expirado** ou que perde a flag para o vínculo na próxima mensagem, não no próximo reinício.

## Quem pode controlar

Um chat é um lugar, não uma identidade, então a autoridade fica presa ao **id do remetente** da plataforma:

- Apenas um remetente emparelhado recebe resposta. Qualquer outro é **ignorado sem resposta**, o que é deliberado: uma recusa diz a um estranho que o bot é real.
- Os remetentes emparelhados são listados no vínculo. Clique em um para removê-lo.
- **Somente mensagens diretas.** Um grupo permitiria que várias pessoas escrevessem no prompt de um agent que pode ter uma concessão de escrita.

## Escritas sempre perguntam

Toda escrita é apresentada a você no chat com o método, o caminho e o corpo exatos, e espera uma palavra. Apenas `yes`, `y`, `ok`, `okay`, `approve`, `oui` ou `go` a aprovam; qualquer outra coisa, ou silêncio, a recusa e o agent é informado de que foi recusada.

A configuração **aprovar escritas automaticamente** da persona não é transferida. Ela foi marcada em uma sessão autenticada no app; não te acompanha até um celular.

## Por onde uma mensagem passa

Em ordem, por todos eles:

1. o interruptor global em **Configurações → Controle externo**
2. o vínculo estar ativado
3. o token ainda carregar `external`, e não estar expirado
4. um chat direto
5. o remetente estar na lista de permitidos
6. um limite de 12 mensagens por minuto por vínculo

Depois a própria execução está sujeita à lista de permissões do catálogo MCP: rotas de conta, rede, segredos, armazenamento de arquivos e apagar dados são inalcançáveis seja qual for o token, assim como o gerenciamento de vínculos. Nenhum agent pode criar um vínculo, gerar um código de emparelhamento ou ampliar o próprio alcance.

## Notas de segurança

- **O token do bot é o acesso.** Quem o possui pode falar com a sua instância no nível daquele vínculo, com a lista de remetentes permitidos ainda no caminho. Mantenha-o no [Cofre](/pt/config/settings#vault) e comece com somente leitura.
- **Injeção de prompt é o risco real**, não o transporte. O conteúdo que o agent lê (e-mail, feeds, webhooks de entrada) pode carregar instruções. O que a contém é o mesmo que a contém no app: a lista de permissões do catálogo, os níveis do token e a confirmação de escrita acima.
- Os **códigos de emparelhamento** têm seis dígitos, são de uso único e com tempo limitado, e só existem entre o clique em *Emparelhar* e o resgate. Encurte a janela no cabeçalho da seção se um código vai ficar na tela.
- As credenciais do bot são seladas em repouso e nunca impressas, inclusive em mensagens de erro.
- Desativado por completo no [modo demo](/pt/guide/demo).

## Limites

- **Somente texto.** Sem gráficos e sem arquivos; um gráfico continua vivendo no app.
- **Sem streaming token a token.** As plataformas de chat só oferecem edição de mensagens, e limitam sua taxa, então a resposta chega em blocos de cerca de um segundo e meio e é dividida no limite da plataforma (4096, 3000 e 2000 caracteres).
- Comandos: `/new` inicia uma conversa nova para aquele chat, `/provider` e `/model` listam e trocam com o que aquele chat responde, `/whoami` mostra o id com que você está emparelhado, `/help`.
- Um reinício descarta uma confirmação de escrita pendente. Nada roda sem confirmação, você apenas é perguntado de novo.
