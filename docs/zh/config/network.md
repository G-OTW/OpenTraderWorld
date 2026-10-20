# 网络与远程访问

OpenTraderWorld 通过四种网络模式控制**谁可以访问应用**。安装后它**仅限 localhost 访问**：在你更改之前，网络上的任何设备都无法连接。

受支持的切换模式的方式是在应用内操作：**设置 → 网络**。保存后会显示需要在主机上运行的确切重启命令（应用无法重启自己的容器）；容器重新创建期间堆栈会短暂离线。

## 四种模式

| 模式 | 可访问者 | 协议 | 适用场景 |
|---|---|---|---|
| **仅限 localhost** | 这台机器 | HTTP | 默认。最安全，网络上的任何设备都无法连接。 |
| **本地网络（LAN）** | 你网络上的设备，通过机器的 IP | 明文 HTTP | 快速的局域网访问；适用于可信的家庭网络。浏览器可能会警告或强制升级到 HTTPS，并且会拒绝使用麦克风，因此[语音控制](/zh/config/voice#https)在其他设备上无法使用。 |
| **LAN + HTTPS** | 你网络上的设备，通过真实域名 | HTTPS，受信任的证书 | 无浏览器警告的局域网访问。不向互联网暴露任何内容。 |
| **公网（Web）** | 任何人，通过你的域名 | HTTPS（Let's Encrypt） | 你希望随时随地访问，并接受公开暴露。 |

## LAN + HTTPS（无浏览器警告） {#lan-https}

浏览器越来越多地拒绝或警告纯 HTTP 站点。该模式使用**真实的、公开受信任的证书**向你网络上的每台设备提供应用：没有警告，客户端设备无需安装任何东西，并且**不向互联网暴露任何内容**。域名所有权通过 DNS 记录（ACME DNS-01 挑战）证明，而不是入站连接，域名会解析到你机器的私有局域网 IP。

可在安装时设置（`./setup.sh`，模式 `3`），或之后在**设置 → 网络 → 本地网络（LAN）+ HTTPS** 中设置：

1. 登录 [duckdns.org](https://www.duckdns.org)（免费），添加一个子域名（例如 `myotw.duckdns.org`）并复制你的账户token；或者使用你自己托管在 Cloudflare 的域名，配合具有 DNS 编辑权限的 API token。
2. 输入域名 + token，以及你机器的局域网 IP（使用 DuckDNS 时记录会自动指向它；使用 Cloudflare 时需自行创建 A 记录）。
3. 用显示的重启命令应用。证书签发期间，第一次请求可能需要约 30 秒。

然后在你网络上的任何设备上打开 `https://myotw.duckdns.org`。

::: warning 注意事项
- 签发的证书会出现在公开的证书透明度日志中，因此域名的**名称**是公开可见的（应用本身仍仅限局域网访问）。
- DNS token存放在 `deploy/dns.env` 中：切勿提交或分享该文件。
- 该模式使用 **80 + 443** 端口，而不是自定义端口。
:::

### 如果域名在某些设备上无法解析 {#dns-rebind}

一些运营商的路由器/解析器会悄悄丢弃指向私有 IP 的 DNS 应答（“DNS 重绑定保护”）。解决办法，按推荐程度排序：

1. 在路由器/DNS 设置中**允许该域名**。
2. 在浏览器中**启用安全 DNS（DNS over HTTPS）**。Chrome：设置 → 隐私和安全 → 安全 → *使用安全 DNS* → Cloudflare；Firefox：设置 → 隐私 → *DNS over HTTPS* → 最大保护。
3. **hosts 文件变通办法**（每台机器单独设置，手机无法这样做）。把域名映射到服务器的局域网 IP：

   ```bash
   # macOS / Linux, then flush the cache (macOS only):
   echo "192.168.1.50 myotw.duckdns.org" | sudo tee -a /etc/hosts
   sudo dscacheutil -flushcache && sudo killall -HUP mDNSResponder
   ```

   在 Windows 上，以管理员身份编辑 `C:\Windows\System32\drivers\etc\hosts`，添加同样的一行，然后运行 `ipconfig /flushdns`。hosts 文件始终优先于 DNS，因此如果服务器的 IP 改变，请删除这一行。

## 公网（Web） {#public}

在你自己的域名上暴露应用，并自动启用 HTTPS。

**前提条件：**

1. 一个**域名**，其公网 **A / AAAA 记录**指向你服务器的公网 IP。
2. 入站 TCP **80** 和 **443** 能到达服务器：在路由器/云防火墙中开放它们，如果位于 NAT 之后则需要端口转发。80 端口是证书（HTTP-01 挑战）所必需的，并会重定向到 HTTPS。

在 `./setup.sh` 中选择模式 `4`，或在**设置 → 网络**中切换。Caddy 会在第一次请求时（约 30 秒）获取并自动续期 Let's Encrypt 证书。

::: danger 一旦开启，任何人都能访问登录页
只有在管理员账号已存在且使用强密码之后才开启它。开启之前先打开
[双因素认证](/zh/config/security#totp)。请保管好 `deploy/.env`。
随时可以在设置 → 网络中切换回私有模式。
:::

## 设置token {#setup-token}

在 **LAN + HTTPS** 和**公网**模式下，首次运行向导在创建第一个账号之前会要求输入**设置token**。

原因是竞争条件：这些模式会响应网络请求，而向导在账号存在之前一直是开放的。没有token的话，谁先打开页面谁就成为管理员，这在 DNS 传播的空档期是真实的风险，数据库卷被重新创建时也是如此。

Core 在启动时生成 token并打印到日志中：

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env logs core | grep "setup token"
```

将其粘贴到向导显示的额外字段中。每次重启都会生成新的token，因此被遗弃的token在容器重启后即失效。

如果你是用 `./setup.sh` 安装的，就不会看到这一步：它在首次启动时根据 `deploy/.env` 创建账号，早于任何请求能到达向导。只有在尚无账号时才会出现token。

## 通过 CLI 更改模式 {#change-mode-cli}

如果安装时选错了模式、完全无法访问应用（例如在无界面服务器上选了 *localhost*），请直接编辑 `deploy/network.env` 并重启：

```bash
cd deploy
# make it reachable on your LAN over plain HTTP (mode 2):
#   OTW_BIND=0.0.0.0     (was 127.0.0.1)
#   OTW_HTTP_PORT=5454   (or your chosen port)
$EDITOR network.env
docker compose --env-file .env --env-file network.env up -d
```

`network.env` 不含机密：它保存绑定接口（`127.0.0.1` = 仅本机，`0.0.0.0` = 所有接口）和端口，由 Compose 插值。LAN + HTTPS 需要的不止一行（证书 + DNS token），所以请在设置中或通过 `./setup.sh` 模式 `3` 来设置。一旦能打开应用，就使用**设置 → 网络**。
