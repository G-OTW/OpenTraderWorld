# Solução de problemas

| Sintoma | Causa provável / correção |
|---|---|
| `port is already allocated` | A porta (80/443/5454) está em uso por outra coisa. Execute `./setup.sh` de novo e escolha outra porta. |
| O Chrome/Edge não abre o app, mas o Safari abre | O navegador força o endereço para `https://`, que os modos HTTP simples não servem. Digite `http://` explicitamente, ou mude para o [modo LAN + HTTPS](/pt/config/network#lan-https). |
| Funciona em `localhost`, mas não pelo IP da máquina (macOS) | O firewall do macOS bloqueia as conexões de entrada do Docker. Ajustes do Sistema → Rede → Firewall → Opções… → defina o **Docker** como *Permitir conexões de entrada*. |
| LAN + HTTPS: certificado não emitido | Verifique `docker compose logs caddy`. O token do DuckDNS/Cloudflare deve ser válido e o domínio escrito exatamente. |
| LAN + HTTPS: o domínio não resolve em alguns dispositivos | Seu resolvedor bloqueia respostas com IP privado (proteção contra DNS rebind). Veja [as correções](/pt/config/network#dns-rebind). |
| O assistente de configuração nunca aparece / `core: offline` na barra superior | O core não alcança o Postgres. Verifique `docker compose logs core` e `logs postgres`; confirme que `DATABASE_URL` corresponde a `POSTGRES_PASSWORD` em `deploy/.env`. |
| Erro de `POSTGRES_PASSWORD` na inicialização | O `deploy/.env` está ausente ou vazio. Execute `./setup.sh`, ou copie `.env.example` para `.env` e preencha. |
| Modo público: certificado HTTPS não emitido | O DNS do domínio deve resolver para este servidor, e as portas 80/443 devem estar acessíveis pela internet. |
| Mudanças no código não aparecem | Recompile: `docker compose up --build -d`. |
| Bloqueado, senha esquecida | Redefina-a pelo shell do host, veja [Esqueci minha senha](#forgot-password). |
| Não consigo acessar o app após escolher o modo de rede errado | Edite `deploy/network.env` à mão e reinicie, veja [alterar o modo pela CLI](/pt/config/network#change-mode-cli). |

## Esqueci minha senha {#forgot-password}

Não há e-mail de redefinição nem formulário de redefinição sem autenticação: o OTW roda no seu próprio servidor, então qualquer endpoint que pudesse alterar uma senha sem estar conectado seria uma porta de entrada. A redefinição fica no **shell do host**, e o link *Esqueceu a senha?* da página de login descreve os mesmos passos.

Abra um shell na máquina que roda o OTW e gere uma senha de uso único:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core reset-password USERNAME
```

Entre com ela; o app pede uma nova imediatamente.

| Caso | O que executar |
|---|---|
| Esqueci o nome de usuário também | `docker exec opentraderworld-core-1 /app/otw-core list-users` |
| Escolher a senha eu mesmo | `printf '%s' 'my-new-password' \| docker exec -i opentraderworld-core-1 /app/otw-core reset-password USERNAME --stdin` |
| Contêiner com outro nome | `docker ps`, depois use o do core no lugar de `opentraderworld-core-1`. |
| Sem usar Docker | Execute o binário `otw-core` com os mesmos argumentos e `DATABASE_URL` definida. |

Nunca passe uma senha como argumento de linha de comando: a linha de comando de um processo é legível no host, e é por isso que `--stdin` existe.

Notas: uma redefinição **desconecta todos os dispositivos**, e nada se perde. O cofre e as chaves de provedores armazenadas são seladas com `OTW_SECRET_KEY`, não com a sua senha.

## Perdi meu autenticador {#lost-authenticator}

Mesmo princípio da senha: recupere pelo shell do host.

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

O segundo fator é removido e todas as sessões são encerradas. Entre com a sua senha,
depois configure-o de novo no novo dispositivo em **Configurações → Segurança**. Veja
[Autenticação em dois fatores](/pt/config/security#totp).

## Bloqueado do social login {#social-locked}

A conta do provedor vinculado está bloqueada, excluída ou inacessível: na página de login, **Usar um
código de recuperação**. O login por senha volta a funcionar.

Perdeu os códigos de recuperação também? No host:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Remove o vínculo, ativa o login por senha e encerra todas as sessões. Perdeu a senha
também: [Esqueci minha senha](#forgot-password). Detalhes: [Social login](/pt/config/social-login#rollback).

## Uma requisição longa é cortada {#request-timeout}

Um backtest, varredura ou importação grande que responde com *this request took longer than the
Ns limit* atingiu o timeout de requisição, não é um bug. Aumente-o em **Configurações → Segurança**; vale
imediatamente, sem reiniciar. Veja [Timeout de requisição](/pt/config/security#timeout).

## Lendo os logs

```bash
cd deploy
docker compose ps              # are all containers up?
docker compose logs -f core    # API server
docker compose logs -f caddy   # proxy / certificates
docker compose logs -f postgres
```

O app também mantém sua própria visualização de logs em **Configurações → Registros** (com busca e nível de captura configurável).

## Recomeçar do zero

::: danger Isto apaga todos os dados
```bash
cd deploy
docker compose down -v
./setup.sh
```
:::

## Ainda travado?

Abra uma issue no [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues) com o sintoma e as linhas de log relevantes.
