# Social sign-in

使用 Google、Microsoft、GitHub 或 OpenID Connect 账号（Authentik、Keycloak、
Authelia、Zitadel…）登录。

登录被锁定在你关联的账号上，依据的是服务商的用户 ID，而不是电子邮件。同一服务商的其他
账号会被拒绝。如果开启了双因素认证，仍会要求输入验证码。

所有设置都在**设置 → 安全 → Social sign-in**中。每次更改都会要求输入你的
密码。

## 要求 {#requirements}

- **重定向 URI**：`<OTW 的地址>/auth/social`，例如 `https://otw.example.com/auth/social`。
  设置页会显示你当前所用地址对应的确切值（点击复制）。它必须与
  浏览器地址完全一致：协议、主机和端口。
- **Google**：`https://` 和域名。纯 `http://` 和裸 IP 地址会被拒绝，
  `localhost` / `127.0.0.1` 除外。
- **Microsoft**：`https://`，或 `http://localhost`。
- **GitHub** 和自托管服务商：按它们允许的来。

通过局域网地址以纯 HTTP 访问的实例，要使用 Google 和 Microsoft，必须先切换到 `lan_https` 或 `web`
（[网络](/zh/config/network)）。

## 设置 {#setup}

### 1. 在服务商处注册 OTW

::: details Google
1. [Google Cloud 控制台](https://console.cloud.google.com/) → **Google Auth Platform**。
   首次使用时，填写应用名称和支持邮箱，受众选择 **External**。
2. **受众**：应用处于 *Testing* 状态时，只有列出的测试用户可以登录。请在那里添加你的
   Google 账号。
3. **客户端 → 创建客户端**，类型选择 **Web 应用**。
4. **已获授权的重定向 URI**：填设置页中的重定向 URI。*已获授权的 JavaScript
   来源*留空。
5. 复制客户端 ID 和客户端密钥。Google 可能需要几分钟才能使新的
   重定向 URI 生效。
:::

::: details Microsoft
1. [Microsoft Entra 管理中心](https://entra.microsoft.com/) → **应用注册 → 新
   注册**。
2. **受支持的帐户类型**：如果你用个人账号登录（Outlook.com、Hotmail、Xbox），
   请包含个人账号。
3. **身份验证 → 添加平台 → Web**：填设置页中的重定向 URI。
4. **证书和密码 → 客户端密码 → 新客户端密码**。复制**值**，它
   只显示一次。它会过期（最长 24 个月），参见[密钥已过期](#secret-expired)。
5. 从**概述**中复制**应用程序（客户端）ID**。
6. OTW 中的租户：`common`（默认，任意账号）、`consumers`（仅个人）、
   `organizations`（仅工作或学校），或你的租户 ID 或域名。
:::

::: details GitHub
1. GitHub → **Settings → Developer settings → OAuth Apps → New OAuth App**。
2. **Authorization callback URL**：填设置页中的重定向 URI。
3. 复制客户端 ID，然后 **Generate a new client secret** 并复制它。
:::

::: details OpenID Connect（自托管）
1. 创建一个机密 OpenID Connect 客户端：授权码流程，范围
   `openid email profile`，重定向 URI 填设置页中的值。
2. 复制客户端 ID、客户端密钥和 **issuer URL**：即提供
   `/.well-known/openid-configuration` 的地址，例如 Authentik 上的 `https://auth.example.com/application/o/otw`。
3. issuer 必须是 `https://`（仅在 localhost 上可用 `http://`），并且必须与该文档的 `issuer`
   字段完全一致。
:::

### 2. 在 OTW 中填写

选择服务商，粘贴客户端 ID 和密钥（以及租户或 issuer URL），**保存**。
OTW 会在保存时联系服务商：错误的租户或 issuer 会在这里失败，而不是在登录时。

### 3. 关联你的账号

**关联账号**会带你到服务商处选择账号。返回设置页后，
卡片会显示*已锁定到 …*。登录页现在有一个**使用 … 继续**按钮。

### 4. 生成恢复码 {#recovery-codes}

**恢复码 → 生成**：十个一次性代码，只显示一次。当服务商账号被锁定、删除或无法访问时，
它们可让你登录。请存放在这台服务器之外。生成新的一组会使上一组
失效。

### 5. 关闭密码登录（可选） {#password-off}

关联账号并生成恢复码后即可使用。**密码登录 → 关闭**：
登录表单会拒绝密码，只有已关联的账号和恢复码可用。

在取消关联、移除、使用恢复码登录以及主机上重置
密码时，它会自动重新开启。

## 回滚 {#rollback}

### 仍处于登录状态

- **密码登录 → 开启**：密码重新可用，Social sign-in保留。
- **取消关联**：Social sign-in停止，密码重新可用。服务商设置会保留。
- **移除**：服务商设置和关联被删除，密码重新可用。

### 服务商账号无法使用

登录页 → **使用恢复码**。你会登录，密码登录重新开启，并打开
设置 → 安全。在那里取消关联或关联另一个账号，接下来五分钟内不会要求输入
密码。

### 恢复码也丢了

在主机上：

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

这会移除关联、开启密码登录并让所有会话退出登录。如果密码
也丢了，接着运行 `reset-password`（[忘记
密码](/zh/guide/troubleshooting#forgot-password)）。`list-users` 会打印用户名。

### 回到上一个版本 {#downgrade}

此版本新增了迁移 `0145_social_login`。旧版本在遇到含有它不认识的迁移的
数据库时会拒绝启动，因此请先将其移除：

1. 备份数据库（[备份与恢复](/zh/guide/backup-restore)）。
2. 在 `deploy/` 中：

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

3. 安装上一个发布版本（[更新](/zh/guide/updating)）：源码构建时，
   `git reset --hard <previous release commit>` 然后 `up -d --build`；镜像安装时，使用
   上一个版本的 `deploy/` 和镜像。

无论该开关如何设置，密码在旧版本上都能使用。服务商设置和
恢复码会被删除。

::: warning
这只会移除迁移 145。如果已安装了包含更多迁移的更新版本，
请改为恢复更新之前所做的备份。
:::

只做第 2 步而不做第 3 步会重置Social sign-in：当前版本在下次启动时会重新创建空表。

## 错误 {#errors}

| 消息 | 解决办法 |
|---|---|
| `redirect_uri_mismatch`（Google）、`AADSTS50011`（Microsoft） | 登记的重定向 URI 与浏览器地址不同。请从设置页重新复制。 |
| *The OAuth client was not found* / `invalid_client` | 客户端 ID 或密钥错误，或密钥已过期。 |
| *the provider calls itself …* | issuer URL 与服务商 `/.well-known/openid-configuration` 中的 `issuer` 不同。 |
| *this … account is not the one linked to this instance* | 在服务商处选择了另一个账号。 |
| *this sign-in was started in another browser or has expired* | 已超过十分钟，或登录在另一个浏览器中结束了。请重新开始。 |
| *the ID token has expired* | 服务器时钟不正确。请修正主机时间（NTP）。 |

### 密钥已过期 {#secret-expired}

登录失败，出现 `invalid_client` 或关于密钥过期的消息。在服务商处创建新密钥，
然后在设置中点击**编辑**，粘贴并**保存**。关联会保留。被锁在外面了？
请先用恢复码登录。
