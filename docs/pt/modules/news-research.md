# Notícias e pesquisa

## News {#news}

Um agregador de notícias auto-hospedado. Monte **dashboards** (por exemplo *Cripto*, *Macro*), adicione **fontes** a cada um e deixe o agendador consultá-las em segundo plano.

### Fontes

- **RSS / Atom**: cole a URL de um feed, pronto.
- **API (JSON)** para qualquer coisa sem RSS: defina o endpoint, o método, os cabeçalhos e os parâmetros de query, depois mapeie caminhos JSON para campos do item (array de itens, título, URL, data, resumo, id único para deduplicação). As chaves de API vão em **segredos** por feed, armazenados criptografados e referenciados como <code v-pre>{{secret:NAME}}</code> em cabeçalhos ou parâmetros, e nunca mais são mostrados.

A URL, o cabeçalho ou o parâmetro de um feed também pode carregar um placeholder <code v-pre>{{vault.item}}</code> apontando para o [Cofre](/pt/config/settings#vault) compartilhado. O agendador o resolve no momento da consulta, então uma chave é reutilizada entre feeds e módulos sem nunca ser armazenada na configuração do feed.

Cada fonte tem seu próprio **intervalo de polling**; fontes duplicadas são detectadas para que o mesmo feed não seja buscado duas vezes entre dashboards. Inicie/pare o polling por dashboard, ou atualize uma fonte sob demanda.

### Leitura

Filtre itens por busca, fonte, tipo e intervalo de datas; visão compacta ou completa; auto-atualização opcional de 60 segundos com um banner "{n} atualizações, clique para carregar". Um widget de notícias também pode ficar na página inicial do seu dashboard.

## Mailbox {#mailbox}

Suas newsletters, e-mails de notícias de mercado e e-mails de broker, lidos da **sua própria caixa de e-mail**: nada passa por terceiros.

### Conectando uma caixa

Escolha seu provedor (Fastmail, Gmail, iCloud, Zoho, mailbox.org, Posteo, Migadu, Proton Bridge ou qualquer outro servidor IMAP) e as configurações do servidor vêm preenchidas; você fornece uma **senha de app**, que é armazenada no [Cofre](/pt/config/settings#vault) compartilhado e em nenhum outro lugar.

O **Outlook.com / Microsoft 365** não aceita mais senha para IMAP, então entram com OAuth: uma aba abre na Microsoft, você aprova o acesso, e ele volta direto para este app (código de autorização + PKCE, nenhum segredo é armazenado em lugar algum). Isso exige um registro de app único e gratuito seu: Entra ID → App registrations → new registration, depois Authentication → *Mobile and desktop applications* com a URI de redirecionamento que o formulário mostra (`http://localhost:5454/mailbox/oauth` em uma instalação local padrão), public client flows permitidos, e API permissions → `IMAP.AccessAsUser.All` delegada. Cole o Application (client) ID no formulário. O login resultante é criptografado no cofre e renovado automaticamente a cada busca.

A Microsoft só aceita uma URI de redirecionamento que seja `https://…`, ou `http://` em localhost, e seu portal recusa uma URI `http` digitada como `127.0.0.1`, então abra o app em `http://localhost:5454` (a porta é ignorada ao comparar um redirecionamento localhost) ou coloque-o atrás de HTTPS em [Configurações → Rede](/pt/config/settings#rede). Se o seu for um endereço de LAN em HTTP simples, o login recorre a um código que você digita em `microsoft.com/devicelogin`; isso ainda funciona para contas pessoais do Outlook.com, mas os tenants do Microsoft 365 agora bloqueiam por padrão o login por device code.

Essa renovação é a única coisa a saber sobre manutenção: a Microsoft descarta um login após **90 dias sem uso**, então uma caixa que você pausou por meses pedirá para ser reconectada. O app avisa após 60 dias ociosos e, se o login for revogado (troca de senha, redefinição de MFA, política do admin), a caixa mostra **Login necessário** com um botão Reconectar em vez de falhar em silêncio.

O acesso é estritamente **somente leitura**: a pasta é aberta somente leitura, e nada nunca é marcado, movido ou excluído no seu servidor. Conecte várias caixas se você mantém mais de uma.

> Considere um **endereço dedicado** para newsletters. Seu e-mail pessoal fica então fora do app por completo, a senha de app é revogável com um clique, e no dia em que um remetente vazar sua lista você sabe exatamente qual foi.

### O que é mantido

E-mails de listas de discussão (qualquer coisa com `List-Unsubscribe`, `List-Id` ou `Precedence: bulk`) são mantidos automaticamente. Todo o resto é apenas *registrado como um remetente aguardando sua decisão*, e nenhum conteúdo é armazenado até você arquivá-lo. É assim que os extratos de um broker entram: um clique no novo remetente, arquivado como **Broker**.

Os remetentes são arquivados em quatro categorias (**News**, **Newsletter**, **Broker**, **Other**), alteráveis a qualquer momento, e a tela de leitura tem um interruptor de um clique por categoria mais um filtro por caixa quando você tem várias.

### Leitura

As mensagens são sanitizadas na chegada (scripts, estilos, formulários e frames removidos) e exibidas em um frame isolado. **Imagens remotas continuam bloqueadas** até você pedi-las, então o pixel de rastreamento de uma newsletter nunca dispara e o remetente não consegue saber que você a abriu. Os anexos (extratos de broker, PDFs) podem ser baixados da mensagem.

Por mensagem: estrela, marcar como não lida, arquivar, **Lembre-me** (hoje à noite / amanhã / neste fim de semana, direto no [RemindMe](/pt/modules/productivity#remindme)) e **Cancelar inscrição**, enviado por você quando o remetente suporta um clique, aberto em uma aba caso contrário.

### A loja

A aba **Store** é a sua própria lista de newsletters: um cartão por publicação com um nome, um link, uma descrição curta e um tópico (mentalidade, finanças, trading, geopolítica, economia, outros), agrupados por domínio e abríveis com um clique. Ela funciona sozinha, útil mesmo sem caixa conectada.

## Economic Calendar {#economics}

Próximos eventos macro (decisões de bancos centrais, divulgações de CPI, dados de emprego) em uma visão de calendário, para você saber o que vem pela frente na sua sessão. Um clique adiciona um lembrete para um evento.

## FinanceDatabase {#findb}

Um catálogo pesquisável de **mais de 300.000 instrumentos**: ações, ETFs, fundos, índices, moedas e criptomoedas.

No primeiro uso você **instala o catálogo** (um download único de ~15 MB, importado em segundo plano). Depois disso ele vive localmente e **as buscas nunca tocam a rede**. Busque por símbolo ou nome, filtre por tipo de ativo e atributos, e marque instrumentos com estrela em **favoritos**, organizados em pastas com notas (por exemplo uma pasta *Watchlist*).

O catálogo tem seu próprio ciclo de lançamentos, separado do app: o cabeçalho mostra qual **snapshot** está instalado e um botão **Verificar atualizações** pergunta ao publicador se existe um mais novo. Atualizar reimporta o catálogo no lugar; seus favoritos sobrevivem e se revinculam às novas linhas.

O catálogo é construído a partir do projeto de código aberto [FinanceDatabase](https://github.com/JerBouma/FinanceDatabase) de Jeroen Bouma, um conjunto de dados de instrumentos financeiros mantido pela comunidade.

## Resources {#resources}

Uma biblioteca de favoritos para livros de trading, artigos, vídeos e ferramentas: nome, link opcional, descrição, organizados em categorias. Simples de propósito.

Três exibições: **cartões**, **lista** e uma **galeria** com uma miniatura por favorito. Uma miniatura é enviada, colada como URL, ou buscada da pré-visualização social do próprio link com um clique; um favorito sem miniatura recebe um bloco de iniciais em vez de um buraco na grade.

## Community Docs {#community-docs}

Guias escritos pela comunidade, sincronizados da [biblioteca em opentraderworld.com](https://opentraderworld.com/docs) e **legíveis offline** dentro do app. Navegue por categoria, busque e marque favoritos com estrela.

Os docs aparecem como **cartões ou lista**, à sua escolha, e um cartão de categoria mostra uma prévia dos docs que ela contém para você saber o que há dentro antes de abri-la.

Você pode contribuir: escreva um documento no [Editor](/pt/modules/productivity#editor) e use **Enviar para publicação**. Ele vai para uma fila de revisão e aparece na biblioteca de todos depois de aprovado.
