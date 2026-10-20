# Controle por voz

Controle o OpenTraderWorld com a voz, ou dite em qualquer campo de texto. **Somente push to talk**: o microfone abre quando você pressiona e fecha quando solta. Nada escuta no meio-tempo.

**Vem desligado por padrão.** Ative em **Configurações → Voz e atalho → Fala e atalho**.

## O microfone precisa de HTTPS ou localhost {#https}

Os navegadores só dão o microfone (e o reconhecimento de fala embutido) a uma página em uma **origem segura**: uma página servida por **HTTPS**, ou aberta em **localhost**. Isto é uma regra do navegador, não do OpenTraderWorld, e nenhuma configuração do app pode suspendê-la.

| Como você abre o OTW | A voz funciona? |
|---|---|
| Na máquina que o executa, `http://127.0.0.1:5454` ou `http://localhost:5454` | Sim |
| Modo [LAN + HTTPS](/pt/config/network#lan-https) ou [Público](/pt/config/network#public) | Sim |
| Modo [Rede local (LAN)](/pt/config/network) em HTTP simples, a partir de outro dispositivo | **Não**, o navegador esconde o microfone |

Para usar a voz a partir de um celular ou de outro computador da sua rede, troque **Configurações → Rede** para **LAN + HTTPS**. Em HTTP simples a página de configurações de voz mostra um aviso e o botão do microfone explica por que não consegue iniciar.

## Dois atalhos, dois modos

| | Padrão | O que acontece com o que você diz |
|---|---|---|
| **Comando** | `Alt+V` (`⌥V` no Mac) | Vira um plano de ações, mesmo com um campo de texto em foco. |
| **Ditado** | `Alt+Shift+V` (`⌥⇧V`) | É digitado, palavra por palavra, no campo de texto em foco. Nunca é lido como comando. |

Segure o atalho enquanto fala, solte para terminar, `Esc` para cancelar. O **microfone na barra superior** sempre executa comandos: clique para começar, clique de novo para parar, ou segure-o como um walkie-talkie.

Os dois atalhos podem ser alterados em **Configurações → Voz e atalho → Fala e atalho**. Cada um precisa de `Ctrl`, `Alt` ou `⌘` (ou uma tecla de função `F1` a `F12`), para nunca atrapalhar a digitação normal, e os dois devem ser diferentes.

## Motor de fala

O motor transforma sua gravação em texto. Escolha um em **Configurações → Voz e atalho → Fala e atalho**.

| Motor | Configuração | Para onde vai o áudio |
|---|---|---|
| **Este navegador** | nenhuma | O serviço de fala do fabricante do navegador (Google para Chrome, Microsoft para Edge, Apple para Safari). Não disponível no Firefox. |
| **Whisper auto-hospedado** | o serviço incluído, veja [Exemplo: Whisper auto-hospedado](#whisper-example) | Fica na sua máquina |
| **Servidor whisper.cpp** | seu próprio servidor, o endpoint `/inference` dele | Fica no seu servidor |
| **OpenAI / Groq** | uma chave de API (colada ou conectada do [Cofre](/pt/config/settings#vault)) | Enviado a esse provedor |
| **Outro** | qualquer servidor que exponha a API `/audio/transcriptions` da OpenAI | Esse servidor |

Com um motor de servidor, o navegador grava, converte o áudio em um pequeno arquivo WAV e o OTW o repassa ao motor. **Testar** envia meio segundo de silêncio para verificar a URL, a chave e o modelo. As chaves são criptografadas em repouso e nunca são devolvidas ao navegador.

### Exemplo: Whisper auto-hospedado, do zero {#whisper-example}

O serviço Whisper incluído é opcional e não é iniciado por um simples `up`. Os passos 1 a 3 são feitos uma vez: o modelo é mantido no volume `whisper-cache` entre reinícios.

**1. Inicie o serviço**, a partir da raiz do repositório:

```bash
docker compose -f deploy/docker-compose.yml --env-file deploy/.env --env-file deploy/network.env --profile voice up -d whisper
docker logs -f opentraderworld-whisper-1
```

Espere por `Uvicorn running on http://0.0.0.0:8000`, depois `Ctrl+C`. A porta só é acessível pelo OTW dentro do Docker, não pelo seu navegador, e isso é intencional.

**2. Baixe o modelo** (cerca de 500 MB, alguns minutos):

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen(urllib.request.Request('http://localhost:8000/v1/models/Systran/faster-whisper-small',method='POST'),timeout=1800).read())"
```

**3. Verifique se está instalado**: a resposta deve listar `Systran/faster-whisper-small`.

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen('http://localhost:8000/v1/models').read())"
```

**4. Conecte-o ao OTW.** Abra o app em `http://localhost:5454` (ou por HTTPS, veja [acima](#https)), depois **Configurações → Voz e atalho → Fala e atalho**:

1. **Adicionar motor**, predefinição **Whisper auto-hospedado**: ela preenche `http://whisper:8000/v1` e `Systran/faster-whisper-small`.
2. **Salvar e testar**: o motor mostra **Funciona · … ms**.
3. Selecione o motor, depois ligue o interruptor no topo. Opcionalmente defina o **Idioma**.

**5. Experimente**: segure `⌥V` / `Alt+V`, diga *"open journal"*, solte, pressione `Enter`. Para ditado, clique em um campo de texto e segure `⌥⇧V` / `Alt+Shift+V`.

A primeira transcrição após um reinício é mais lenta enquanto o modelo carrega na memória.

## Comandos de voz

**Configurações → Voz e atalho → Comandos de voz**: um comando é uma **frase** (mais outras formas de dizê-la) e uma lista ordenada de **passos**:

- **Abrir uma página**: um módulo, o dashboard ou as Configurações.
- **Perguntar ao agent**: um prompt enviado ao assistente flutuante, que age com suas próprias ferramentas.
- **Executar um workflow**: iniciar um workflow do [Automator](/pt/modules/automator).
- **Tema**, **Ocultar valores**: o mesmo que os botões da barra superior.
- **Falar**: ler uma frase em voz alta.

Uma frase só dispara quando é dita **sozinha**, nunca como uma palavra dentro de uma frase: ditar "a blue turtle" não executa o seu comando *turtle*.

### Embutidos, sem configuração

- **"open &lt;page&gt;"** (*"ouvre &lt;page&gt;"*, *"öffne &lt;Seite&gt;"*, *"abre &lt;página&gt;"*, *"apri &lt;pagina&gt;"*, *"打开 &lt;页面&gt;"*) abre qualquer módulo instalado, o dashboard ou as Configurações. Um nome que corresponda a várias páginas é recusado com a lista, nunca adivinhado.
- Com **Entregar o restante ao agent** ligado, tudo que nenhum comando reconhece vai ao assistente como um único pedido.

### Encadeamento

Diga várias coisas de uma vez, ligadas por *e* ou *depois* (*and*, *then* em inglês, e assim por diante, seguindo a configuração de **Idioma**). Uma vírgula na transcrição também divide:

> "turtle, then open settings and compare AAPL and MSFT"

executa o seu comando *turtle*, abre as Configurações e depois pede ao agent para "compare AAPL and MSFT". Os pedaços restantes lado a lado são mantidos juntos, então o agent recebe o pedido inteiro.

## Confirmação

Todo plano é **mostrado antes de rodar**: o que foi ouvido, cada passo e de onde veio. `Enter` executa, `Esc` cancela. Os passos rodam em ordem e o plano para na primeira falha, nomeando o passo.

Um comando pode ser marcado como **Executar sem confirmação**. Vem **desligado por padrão**, e só vale quando aquela frase é dita sozinha: encadeada com qualquer outra coisa, o plano ainda é mostrado antes. Os passos do agent mantêm a confirmação própria do assistente para toda escrita.

## Bom saber

- O assistente fica oculto na página **Agent**, então um plano com um passo de agent não pode rodar a partir dali.
- **Ler o resultado em voz alta** usa as vozes do próprio navegador: sem configuração, nenhum áudio sai do navegador.
- A demo pública mostra as páginas de voz somente para leitura: ela não salva configurações nem envia áudio.
- **Seu próprio proxy reverso** na frente do OTW não deve bloquear o microfone: o cabeçalho `Permissions-Policy` dele precisa de `microphone=(self)`. Com `microphone=()` o navegador recusa de imediato, sem perguntar.
