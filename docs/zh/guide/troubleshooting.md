# 故障排查

| 现象 | 可能原因 / 解决办法 |
|---|---|
| `port is already allocated` | 该端口（80/443/5454）被其他程序占用。重新运行 `./setup.sh` 并选择其他端口。 |
| Chrome/Edge 打不开应用但 Safari 可以 | 浏览器强制把地址升级为 `https://`，而纯 HTTP 模式不提供 HTTPS。请明确输入 `http://`，或切换到[局域网 + HTTPS 模式](/zh/config/network#lan-https)。 |
| 用 `localhost` 可以访问，但通过本机 IP 不行（macOS） | macOS 防火墙阻止了 Docker 的入站连接。系统设置 → 网络 → 防火墙 → 选项… → 将 **Docker** 设为 *允许传入连接*。 |
| 局域网 + HTTPS：证书未签发 | 查看 `docker compose logs caddy`。DuckDNS/Cloudflare token必须有效，域名也必须拼写准确。 |
| 局域网 + HTTPS：部分设备无法解析域名 | 你的解析器会屏蔽私有 IP 的应答（DNS 重绑定保护）。参见[解决办法](/zh/config/network#dns-rebind)。 |
| 设置向导始终不出现 / 顶栏显示 `core: offline` | Core 无法连接 Postgres。查看 `docker compose logs core` 和 `logs postgres`；确认 `deploy/.env` 中的 `DATABASE_URL` 与 `POSTGRES_PASSWORD` 一致。 |
| 启动时出现 `POSTGRES_PASSWORD` 错误 | `deploy/.env` 缺失或为空。运行 `./setup.sh`，或将 `.env.example` 复制为 `.env` 并填写。 |
| 公网模式：HTTPS 证书未签发 | 域名的 DNS 必须解析到这台服务器，且 80/443 端口必须能从互联网访问。 |
| 代码修改没有生效 | 重新构建：`docker compose up --build -d`。 |
| 被锁在外面，忘记密码 | 在主机 shell 中重置，参见[忘记密码](#forgot-password)。 |
| 选错网络模式后无法访问应用 | 手动编辑 `deploy/network.env` 并重启，参见[通过 CLI 更改模式](/zh/config/network#change-mode-cli)。 |

## 忘记密码 {#forgot-password}

没有重置邮件，也没有无需认证的重置表单：OTW 运行在你自己的服务器上，任何能在未登录状态下更改密码的端点都会成为入侵途径。因此重置放在**主机 shell** 中，登录页的*忘记密码？*链接给出的也是相同的步骤。

在运行 OTW 的机器上打开 shell，并打印一次性密码：

```bash
docker exec -it opentraderworld-core-1 /app/otw-core reset-password USERNAME
```

用它登录；应用会立即要求你设置新密码。

| 情况 | 运行什么 |
|---|---|
| 连用户名也忘了 | `docker exec opentraderworld-core-1 /app/otw-core list-users` |
| 自己选择密码 | `printf '%s' 'my-new-password' \| docker exec -i opentraderworld-core-1 /app/otw-core reset-password USERNAME --stdin` |
| 容器名称不同 | 运行 `docker ps`，然后用 core 容器的名称替换 `opentraderworld-core-1`。 |
| 没有运行 Docker | 以相同参数运行 `otw-core` 二进制文件，并设置 `DATABASE_URL`。 |

切勿把密码作为命令行参数传入：进程的命令行在主机上是可读的，这正是存在 `--stdin` 的原因。

说明：重置会**让所有设备退出登录**，不会丢失任何数据。保险库和已存储的服务商密钥是用 `OTW_SECRET_KEY` 加密的，而不是你的密码。

## 丢失验证器 {#lost-authenticator}

原则与密码相同：在主机 shell 中恢复。

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-totp USERNAME
```

第二重验证因素会被移除，所有会话都会退出登录。用密码登录，
然后在**设置 → 安全**中于新设备上重新设置。参见
[双因素认证](/zh/config/security#totp)。

## 被Social sign-in锁在外面 {#social-locked}

关联的服务商账号被锁定、删除或无法访问：在登录页点击**使用恢复码**。密码登录会重新启用。

恢复码也丢了，在主机上执行：

```bash
docker exec -it opentraderworld-core-1 /app/otw-core disable-social USERNAME
```

这会移除关联、开启密码登录并让所有会话退出登录。密码也丢了：
[忘记密码](#forgot-password)。详情：[Social sign-in](/zh/config/social-login#rollback)。

## 长时间请求被中断 {#request-timeout}

backtest、参数扫描或大型导入如果返回 *this request took longer than the
Ns limit*，说明触发了请求超时，而不是出现了 bug。在**设置 → 安全**中调高它；立即生效，
无需重启。参见[请求超时](/zh/config/security#timeout)。

## 查看日志

```bash
cd deploy
docker compose ps              # are all containers up?
docker compose logs -f core    # API server
docker compose logs -f caddy   # proxy / certificates
docker compose logs -f postgres
```

应用也在**设置 → 日志**中提供自己的日志视图（可搜索，捕获级别可配置）。

## 完全重新开始

::: danger 这会删除所有数据
```bash
cd deploy
docker compose down -v
./setup.sh
```
:::

## 仍然解决不了？

在 [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues) 上提交 issue，附上现象和相关日志行。
