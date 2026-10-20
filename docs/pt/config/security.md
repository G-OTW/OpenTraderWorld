# Segurança da conta

O OTW tem uma conta e nenhum e-mail de redefinição de senha. O que a protege é a senha (ou uma
conta social vinculada), um segundo fator opcional e o fato de que nada além de você consegue
alcançar a máquina. Esta página
cobre o que você pode ativar e o que fazer quando algo dá errado.

Tudo aqui fica em **Configurações → Segurança**, exceto a própria senha, que fica em
**Configurações → Conta**.

## Autenticação em dois fatores {#totp}

Um código de seis dígitos de um app no seu celular, pedido depois da senha. É o que uma senha
roubada sozinha não consegue superar, e é a coisa mais útil a ativar
antes de abrir o app para a internet.

::: tip Nada é enviado a você
Isto é **TOTP** (RFC 6238), não um código enviado por SMS ou e-mail sob demanda. Seu autenticador e
o servidor compartilham um segredo uma vez, na configuração, e depois cada um calcula o mesmo código a partir da hora
atual. Sem SMS, sem e-mail, sem serviço de terceiros, e funciona com a máquina offline.
:::

### Ativando

1. **Configurações → Segurança → Configurar**. O app mostra um QR code, o link `otpauth://` por trás
   dele e o próprio segredo.
2. Adicione-o a qualquer app autenticador: Google Authenticator, Aegis, Ente Auth, 1Password,
   Bitwarden, Proton Pass, o que você já usa. Escaneie o QR code, cole o link
   ou digite o segredo à mão.
3. Digite os seis dígitos que ele mostra e confirme.

Nada muda na forma de entrar até esse último passo ter sucesso. Um código que você não consegue gerar
nunca se torna uma exigência, então uma configuração pela metade não pode te trancar para fora.

### Entrando depois

Informe seu usuário e senha como antes; o app então pede o código. Cada código funciona
uma vez e dura 30 segundos, com uma pequena tolerância para cada lado para um celular cujo relógio
tenha se desviado.

### Desativando

**Configurações → Segurança → Desativar**, que pede sua senha de novo. Faça isso antes de
formatar um celular e configure de novo no novo.

### Se você perder o autenticador {#totp-lost}

Não há código de backup nem e-mail de recuperação, pelo mesmo motivo que não há formulário
de redefinição de senha. Recupere pelo shell da máquina:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

O segundo fator e seu segredo são removidos e todas as sessões são encerradas. Entre com
sua senha e configure de novo no novo dispositivo.

::: warning Mantenha um caminho de volta
Guarde o segredo no seu gerenciador de senhas ao configurar, ou mantenha ligado o backup
do próprio autenticador. Caso contrário, o único caminho de volta é o acesso ao shell da máquina.
:::

## Social login {#social}

Login com Google, Microsoft, GitHub ou OpenID Connect, travado em uma única conta vinculada, com
códigos de recuperação e um interruptor opcional para desligar as senhas. Veja
[Social login](/pt/config/social-login).

## Sessões ativas {#sessions}

**Configurações → Segurança** lista todo navegador atualmente conectado à conta, com o
endereço de onde veio e quando foi usado pela última vez. O navegador em que você está lendo isto está marcado.

- **Encerrar** finaliza uma delas.
- **Encerrar todas as outras sessões** finaliza todas menos a sua, que é o que clicar se você entrou
  em algum lugar onde não devia, ou se não tem certeza.

Um login a partir de um endereço que a conta nunca usou antes também gera uma notificação. Ela
chega no sino e é enviada a qualquer canal concedido ao produtor **Segurança (logins)**
em **Configurações → Notificações** (uma concessão curinga já a cobre). Dispara uma vez
por endereço, não a cada login.

As sessões duram uma semana. Nos modos expostos à rede (**LAN + HTTPS** e **Público**) elas também
terminam após um dia sem atividade, para que um navegador deixado aberto em uma máquina que você abandonou
não continue conectado indefinidamente. Em localhost e LAN simples vale apenas o limite de uma semana.

## O pedido de senha que volta {#step-up}

Algumas ações pedem sua senha de novo mesmo você já estando conectado:

- gerar um token de acesso para um AI agent (**Configurações → AI agents**)
- mudar o modo de rede (**Configurações → Rede**)
- gravar um valor no **Cofre**
- baixar um backup parcial **com credenciais incluídas**
- desligar o segundo fator

Essas ações criam uma credencial, mudam o que o mundo exterior pode alcançar ou entregam todos os segredos
armazenados em um só arquivo. Uma confirmação vale para todas por cinco minutos, então uma sequência de
mudanças pergunta uma vez, e a confirmação pertence ao navegador que a deu. Se o segundo
fator estiver ligado, o pedido também exige um código.

## Regras de senha {#password}

Definida ou alterada em **Configurações → Conta**, que sempre pede a senha atual primeiro
e encerra todas as outras sessões em caso de sucesso.

Uma senha deve ter **pelo menos 12 caracteres** e não pode ser uma que já apareça em
listas públicas de vazamentos. Essa verificação ignora os enfeites que as pessoas adicionam para passar por uma regra, então
`P@ssw0rd!2024` é recusada pelo mesmo motivo que `password`. Sequências (`abcdefghijkl`) e
qualquer coisa que contenha o nome da sua conta também são recusadas.

A senha mais fácil que passa é um punhado de palavras sem relação: `fennel-ladder-oxide-73` é
aceita, curta e digitável. Não há regra sobre misturar símbolos e dígitos, porque
o que importa é o comprimento.

Esqueceu? Veja [Esqueci minha senha](/pt/guide/troubleshooting#forgot-password).

## Timeout de requisição {#timeout}

**Configurações → Segurança** também define por quanto tempo uma única requisição pode rodar antes de o servidor
interrompê-la. O padrão é 120 segundos.

Ele existe para que uma requisição travada não mantenha uma conexão aberta para sempre. **Não** é um limite
de taxa: nada conta com que frequência você chama o app, e um AI agent usando a API intensamente
não é afetado. Os streams de preço ao vivo e a visualização de logs também não são afetados, porque o limite cobre
a produção de uma resposta, não a vida de um stream.

Aumente-o se um backtest longo ou uma importação grande for cortado com uma mensagem de timeout.

## Primeira execução em uma instância exposta à rede {#setup-token}

Quando a primeira conta é criada em uma instância já acessível pela rede
(**LAN + HTTPS** ou **Público**), o assistente de configuração pede um **token de configuração**. Veja
[Rede e acesso remoto](/pt/config/network#setup-token).
