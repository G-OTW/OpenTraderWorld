# Rede e acesso remoto

O OpenTraderWorld controla **quem pode acessar o app** por meio de quatro modos de rede. Após a instalação ele fica **somente em localhost**: nada na sua rede consegue se conectar até você mudar isso.

A forma suportada de trocar de modo é dentro do app: **Configurações → Rede**. Ao salvar, é exibido o comando exato de reinício a executar no host (o app não consegue reiniciar seus próprios contêineres); a stack fica brevemente offline enquanto os contêineres são recriados.

## Os quatro modos

| Modo | Acessível por | Protocolo | Use quando |
|---|---|---|---|
| **Somente localhost** | esta máquina | HTTP | Padrão. O mais seguro, nada na rede consegue se conectar. |
| **Rede local (LAN)** | dispositivos da sua rede, pelo IP da máquina | HTTP simples | Acesso rápido na LAN; adequado para redes domésticas confiáveis. Os navegadores podem avisar ou forçar a mudança para HTTPS, e recusam o microfone, então o [controle por voz](/pt/config/voice#https) não funciona a partir de outros dispositivos. |
| **LAN + HTTPS** | dispositivos da sua rede, por um domínio real | HTTPS, certificado confiável | Acesso na LAN sem avisos do navegador. Nada exposto à internet. |
| **Público (Web)** | qualquer pessoa, no seu domínio | HTTPS (Let's Encrypt) | Você quer acesso de qualquer lugar e aceita a exposição pública. |

## LAN + HTTPS (sem avisos do navegador) {#lan-https}

Os navegadores recusam ou avisam cada vez mais sobre sites em HTTP simples. Este modo serve o app a todo dispositivo da sua rede com um **certificado real, publicamente confiável**: sem avisos, nada instalado nos dispositivos clientes e **nada exposto à internet**. A propriedade do domínio é provada com um registro DNS (desafio ACME DNS-01), não com uma conexão de entrada, e o domínio resolve para o IP privado da LAN da sua máquina.

Configure na instalação (`./setup.sh`, modo `3`) ou depois em **Configurações → Rede → Rede local (LAN) + HTTPS**:

1. Entre em [duckdns.org](https://www.duckdns.org) (gratuito), adicione um subdomínio (por exemplo `myotw.duckdns.org`) e copie o token da sua conta, ou use seu próprio domínio no Cloudflare com um token de API com permissão de edição de DNS.
2. Informe o domínio + token, mais o IP da LAN da sua máquina (com o DuckDNS o registro é apontado para ele automaticamente; no Cloudflare crie você mesmo o registro A).
3. Aplique com o comando de reinício exibido. A primeira requisição pode levar ~30 s enquanto o certificado é emitido.

Depois abra `https://myotw.duckdns.org` de qualquer dispositivo da sua rede.

::: warning Notas
- Os certificados emitidos aparecem em logs públicos de Certificate Transparency, então o **nome** do domínio fica publicamente visível (o app em si continua só na LAN).
- O token de DNS é armazenado em `deploy/dns.env`: nunca versione nem compartilhe esse arquivo.
- Este modo usa as portas **80 + 443** em vez da porta personalizada.
:::

### Se o domínio não resolve em alguns dispositivos {#dns-rebind}

Alguns roteadores/resolvedores de provedores descartam silenciosamente respostas DNS que apontam para um IP privado ("proteção contra DNS rebind"). Correções, da melhor para a pior:

1. **Permita o domínio** nas configurações do seu roteador/DNS.
2. **Ative o DNS seguro (DNS sobre HTTPS)** no navegador. Chrome: Configurações → Privacidade e segurança → Segurança → *Usar DNS seguro* → Cloudflare; Firefox: Configurações → Privacidade → *DNS sobre HTTPS* → Proteção máxima.
3. **Contorno com o arquivo hosts** (por máquina, e celulares não conseguem fazer isso). Mapeie o domínio para o IP da LAN do servidor:

   ```bash
   # macOS / Linux, then flush the cache (macOS only):
   echo "192.168.1.50 myotw.duckdns.org" | sudo tee -a /etc/hosts
   sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder
   ```

   No Windows, edite `C:\Windows\System32\drivers\etc\hosts` como Administrador, adicione a mesma linha e execute `ipconfig /flushdns`. O arquivo hosts sempre vence o DNS, então remova a linha se o IP do servidor mudar.

## Público (Web) {#public}

Expõe o app no seu próprio domínio com HTTPS automático.

**Pré-requisitos:**

1. Um **domínio** com um **registro A / AAAA** público apontando para o IP público do seu servidor.
2. TCP **80** e **443** de entrada chegando ao servidor: abra-as no firewall do roteador/nuvem e faça o redirecionamento de portas se estiver atrás de NAT. A porta 80 é necessária para o certificado (desafio HTTP-01) e redireciona para HTTPS.

Escolha o modo `4` no `./setup.sh` ou troque em **Configurações → Rede**. O Caddy obtém e renova automaticamente um certificado Let's Encrypt na primeira requisição (~30 s).

::: danger Qualquer pessoa alcança a página de login quando isto está ligado
Ative apenas depois que sua conta de administrador existir e com uma senha forte. Ative a
[autenticação em dois fatores](/pt/config/security#totp) antes. Mantenha o `deploy/.env` em segredo.
Volte a um modo privado quando quiser em Configurações → Rede.
:::

## O token de configuração {#setup-token}

Em **LAN + HTTPS** e **Público**, o assistente de primeira execução pede um **token de configuração** antes de
criar a primeira conta.

O motivo é uma corrida: esses modos respondem à rede, e o assistente fica aberto até existir uma
conta. Sem um token, quem carregar a página primeiro vira o administrador, o que é um
risco real no intervalo enquanto o DNS propaga, e de novo se um volume de banco for recriado.

O core gera o token na inicialização e o imprime no log:

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env logs core | grep "setup token"
```

Cole-o no campo extra que o assistente mostra. Um novo é gerado a cada reinício, então um
token abandonado para de funcionar assim que o contêiner reinicia.

Você não verá isso se instalou com `./setup.sh`: ele cria a conta a partir de
`deploy/.env` na primeira inicialização, antes que qualquer coisa possa alcançar o assistente. O token só aparece
quando ainda não existe conta.

## Alterando o modo pela CLI {#change-mode-cli}

Se você escolheu o modo errado na instalação e não consegue acessar o app de jeito nenhum (por exemplo, escolheu *localhost* em um servidor sem interface), edite `deploy/network.env` diretamente e reinicie:

```bash
cd deploy
# make it reachable on your LAN over plain HTTP (mode 2):
#   OTW_BIND=0.0.0.0     (was 127.0.0.1)
#   OTW_HTTP_PORT=5454   (or your chosen port)
$EDITOR network.env
docker compose --env-file .env --env-file network.env up -d
```

O `network.env` não contém segredos: ele guarda a interface de bind (`127.0.0.1` = somente esta máquina, `0.0.0.0` = todas as interfaces) e as portas, interpoladas pelo Compose. LAN + HTTPS precisa de mais de uma linha (certificado + token de DNS), então configure esse a partir das Configurações ou do `./setup.sh` modo `3`. Quando conseguir abrir o app, use **Configurações → Rede**.
