# Social login

Entre com uma conta Google, Microsoft, GitHub ou OpenID Connect (Authentik, Keycloak,
Authelia, Zitadel…).

O login fica travado na conta que você vincular, pelo id de usuário do provedor, não por e-mail. Outra
conta no mesmo provedor é recusada. Se a autenticação em dois fatores estiver ligada, o código
continua sendo pedido.

Todas as configurações ficam em **Configurações → Segurança → Social login**. Cada alteração pede sua
senha.

## Requisitos {#requirements}

- **URI de redirecionamento**: `<address of OTW>/auth/social`, por exemplo `https://otw.example.com/auth/social`.
  As Configurações mostram o valor exato para o endereço em que você está (clique para copiar). Ele deve coincidir
  exatamente com o endereço do navegador: esquema, host e porta.
- **Google**: `https://` e um nome de domínio. `http://` simples e endereços IP puros são recusados,
  exceto `localhost` / `127.0.0.1`.
- **Microsoft**: `https://`, ou `http://localhost`.
- **GitHub** e provedores auto-hospedados: o que eles permitirem.

Uma instância acessada por HTTP simples em um endereço de LAN deve primeiro mudar para `lan_https` ou `web`
para Google e Microsoft ([Rede](/pt/config/network)).

## Configuração {#setup}

### 1. Registre o OTW no provedor

::: details Google
1. [Console do Google Cloud](https://console.cloud.google.com/) → **Google Auth Platform**.
   No primeiro uso, preencha o nome do app e o e-mail de suporte, público **Externo**.
2. **Público**: enquanto o app estiver em *Teste*, apenas os usuários de teste listados podem entrar. Adicione sua
   conta Google ali.
3. **Clientes → Criar cliente**, tipo **Aplicativo da Web**.
4. **URIs de redirecionamento autorizados**: a URI de redirecionamento das Configurações. Deixe *Origens JavaScript
   autorizadas* vazio.
5. Copie o ID do cliente e o segredo do cliente. O Google pode levar alguns minutos para aplicar uma nova
   URI de redirecionamento.
:::

::: details Microsoft
1. [Centro de administração do Microsoft Entra](https://entra.microsoft.com/) → **Registros de aplicativo → Novo
   registro**.
2. **Tipos de conta com suporte**: inclua contas pessoais se você entra com uma
   (Outlook.com, Hotmail, Xbox).
3. **Autenticação → Adicionar uma plataforma → Web**: a URI de redirecionamento das Configurações.
4. **Certificados e segredos → Segredos do cliente → Novo segredo do cliente**. Copie o **Valor**, ele
   é mostrado uma vez. Ele expira (24 meses no máximo), veja [Segredo expirado](#secret-expired).
5. Copie o **ID do aplicativo (cliente)** em **Visão geral**.
6. Tenant no OTW: `common` (padrão, qualquer conta), `consumers` (somente pessoais),
   `organizations` (somente trabalho ou escola) ou o ID ou domínio do seu tenant.
:::

::: details GitHub
1. GitHub → **Settings → Developer settings → OAuth Apps → New OAuth App**.
2. **Authorization callback URL**: a URI de redirecionamento das Configurações.
3. Copie o ID do cliente, depois **Generate a new client secret** e copie-o.
:::

::: details OpenID Connect (auto-hospedado)
1. Crie um cliente OpenID Connect confidencial: fluxo de código de autorização, escopos
   `openid email profile`, a URI de redirecionamento das Configurações.
2. Copie o ID do cliente, o segredo do cliente e a **URL do emissor**: o endereço que serve
   `/.well-known/openid-configuration`, por exemplo `https://auth.example.com/application/o/otw` no
   Authentik.
3. O emissor deve ser `https://` (`http://` apenas em localhost) e deve coincidir exatamente com o campo `issuer`
   desse documento.
:::

### 2. Informe-o no OTW

Escolha o provedor, cole o ID e o segredo do cliente (e o tenant ou a URL do emissor), **Salvar**.
O OTW contata o provedor ao salvar: um tenant ou emissor errado falha aqui, não no login.

### 3. Vincule sua conta

**Vincular uma conta** envia você ao provedor para escolher a conta. De volta às Configurações, o
cartão mostra *Travado em …*. A página de login agora tem um botão **Continuar com …**.

### 4. Gere códigos de recuperação {#recovery-codes}

**Códigos de recuperação → Gerar**: dez códigos de uso único, mostrados uma vez. Eles fazem você entrar quando a
conta do provedor está bloqueada, excluída ou inacessível. Guarde-os fora deste servidor. Um novo conjunto
cancela o anterior.

### 5. Desligue o login por senha (opcional) {#password-off}

Possível depois que uma conta está vinculada e os códigos de recuperação existem. **Login por senha → Desligar**:
o formulário de login recusa senhas, só funcionam a conta vinculada e os códigos de recuperação.

Ele volta a ligar sozinho ao desvincular, ao remover, ao entrar com um código de recuperação e em uma redefinição
de senha pelo host.

## Reversão {#rollback}

### Ainda conectado

- **Login por senha → Ligar**: as senhas voltam a funcionar, o social login permanece.
- **Desvincular**: o social login para, as senhas voltam a funcionar. As configurações do provedor são mantidas.
- **Remover**: as configurações do provedor e o vínculo são excluídos, as senhas voltam a funcionar.

### Conta do provedor inutilizável

Página de login → **Usar um código de recuperação**. Você entra, o login por senha volta a ficar ligado e
Configurações → Segurança abre. Desvincular ou vincular outra conta ali não pede a
senha pelos próximos cinco minutos.

### Códigos de recuperação perdidos também

No host:

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

Remove o vínculo, liga o login por senha e encerra todas as sessões. Se a senha também
foi perdida, execute `reset-password` em seguida ([Esqueci minha
senha](/pt/guide/troubleshooting#forgot-password)). `list-users` imprime o nome de usuário.

### Voltar à versão anterior {#downgrade}

Esta versão adiciona a migração `0145_social_login`. Uma versão anterior se recusa a iniciar em um
banco com uma migração que ela não conhece, então remova-a primeiro:

1. Faça backup do banco ([Backup e restauração](/pt/guide/backup-restore)).
2. A partir de `deploy/`:

   ```bash
   docker compose --env-file .env --env-file network.env exec -T postgres \
     psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -c "
       BEGIN;
       DROP TABLE IF EXISTS recovery_codes;
       DROP TABLE IF EXISTS social_login;
       ALTER TABLE users DROP COLUMN IF EXISTS password_login;
       DELETE FROM _sqlx_migrations WHERE version = 145;
       COMMIT;"
   ```

3. Instale a versão anterior ([Atualização](/pt/guide/updating)): build a partir do código-fonte,
   `git reset --hard <previous release commit>` e depois `up -d --build`; instalação por imagens, o
   `deploy/` e as imagens da versão anterior.

As senhas funcionam na versão anterior seja qual for a posição do interruptor. As configurações do provedor e
os códigos de recuperação são excluídos.

::: warning
Isto remove apenas a migração 145. Se uma versão posterior com mais migrações estiver instalada,
restaure o backup feito antes dessa atualização.
:::

O passo 2 sem o passo 3 redefine o social login: a versão atual recria as tabelas vazias
na próxima inicialização.

## Erros {#errors}

| Mensagem | Correção |
|---|---|
| `redirect_uri_mismatch` (Google), `AADSTS50011` (Microsoft) | A URI de redirecionamento registrada difere do endereço do navegador. Copie-a de novo das Configurações. |
| *The OAuth client was not found* / `invalid_client` | ID ou segredo do cliente errado, ou o segredo expirou. |
| *the provider calls itself …* | A URL do emissor difere do `issuer` no `/.well-known/openid-configuration` do provedor. |
| *this … account is not the one linked to this instance* | Outra conta foi escolhida no provedor. |
| *this sign-in was started in another browser or has expired* | Passaram mais de dez minutos, ou o login terminou em outro navegador. Comece de novo. |
| *the ID token has expired* | O relógio do servidor está errado. Corrija a hora do host (NTP). |

### Segredo expirado {#secret-expired}

O login falha com `invalid_client` ou uma mensagem sobre segredo expirado. Crie um novo segredo
no provedor, depois **Editar** nas Configurações, cole-o e **Salvar**. O vínculo é mantido. Trancado para fora?
Entre antes com um código de recuperação.
