# 安装

在你自己的机器或服务器上自托管 OpenTraderWorld。大约需要 5 分钟。

::: info 仅支持容器化（目前）
OpenTraderWorld 以 Docker Compose 堆栈运行，这是唯一受支持的部署方式。原生安装可行但不推荐：Docker 让安装不侵入系统（一切都在容器和卷中），并且能快速重建。原因以及各操作系统的安装步骤见[获取 Docker](/zh/guide/docker)。
:::

## 要求

- 带 Docker Compose 的 **Docker**。还没有？[获取 Docker](/zh/guide/docker) 用几条命令涵盖了 macOS、Windows 和 Linux。
- Linux、macOS 或 Windows。
- 一个空闲端口（默认 **5454**；HTTPS 模式使用 **80** + **443** 端口）。可在设置过程中更改。localhost 模式和局域网明文 HTTP 模式还会在 `127.0.0.1` 上绑定 **8443**。

检查 Docker 是否就绪：

```bash
docker --version
docker compose version
```

## 一条命令安装（推荐）

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

安装程序会检查 Docker 是否就绪，将部署文件（仅 `deploy/` 目录，不含源代码和工具链）下载到 `./opentraderworld`，然后交给下面的引导式设置，它会从 Docker Hub **拉取预构建镜像**。

选项写在 `bash -s --` 之后：

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash -s -- --dir ~/otw
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--dir <path>` | `./opentraderworld` | 安装目录。目录非空或已有安装时拒绝执行。 |
| `--ref <ref>` | `master` | 要安装的分支或标签。 |
| `--build` | 关闭 | 克隆完整源码并在本地构建镜像，而不是拉取（需要 `git` 和工具链）。 |
| `--yes` | 关闭 | 传给 `setup.sh`：没有可用于提问的终端时，全部采用默认值。 |
| `--wipe-volumes` | 关闭 | 传给 `setup.sh`：不经询问就删除上次安装的卷。**这会删除那次安装的数据。** |

`OTW_DIR` 和 `OTW_REF` 设置与 `--dir` 和 `--ref` 相同的值（选项优先于对应变量），`OTW_REPO` 设置 GitHub 仓库（默认 `G-OTW/OpenTraderWorld`）。

## 通过 git 克隆（替代方式）

```bash
git clone https://github.com/G-OTW/OpenTraderWorld.git
cd OpenTraderWorld/deploy
./setup.sh
```

## 引导式设置

上面两种方式都会运行 `deploy/setup.sh`。它会问几个问题、生成高强度密钥、写入配置并启动所有服务。

它会提示：

| 问题 | 默认值 | 说明 |
|---|---|---|
| **网络模式** | `1`（localhost） | `1` 仅本机 · `2` 局域网明文 HTTP · `3` 局域网 HTTPS（真实证书） · `4` 使用你自己域名的公网。之后可更改，参见[网络与远程访问](/zh/config/network)。 |
| **管理员用户名** | `admin` | 会为你创建管理员账号；生成强密码并**仅显示一次**。 |
| **HTTP 端口** | `5454` | 仅模式 1–2；模式 3 和 4 使用 80 + 443。 |
| **DuckDNS 域名 / token / 局域网 IP** | 无 | 仅模式 3。 |
| **公网域名** | 无 | 仅模式 4，且必须已解析到这台服务器。 |
| **日志级别** | `info` | `trace` / `debug` / `info` / `warn` / `error`。 |

它还接受以下选项：

| 选项 | 说明 |
|---|---|
| `--build` | 从源码构建镜像，而不是拉取（见下文）。 |
| `--yes` | 没有可用于提问的终端时，全部采用默认值。 |
| `--wipe-volumes` | 不经询问就删除上次安装的卷。**这会删除那次安装的数据。** |
| `--non-interactive` | 等同于 `OTW_NONINTERACTIVE=1`：所有答案都从环境变量读取，从不读取终端。未设置的变量采用默认值。 |

非交互模式的变量，每个问题一个：`NET_CHOICE`（`1` 到 `4`）、`OTW_HTTP_PORT`、`ADMIN_USER`、`OTW_ADMIN_PASSWORD`（未设置时自动生成）、`OTW_LOG`、`DUCK_DOMAIN`、`DUCK_TOKEN` 和 `LAN_IP`（模式 3）、`OTW_DOMAIN`（模式 4），以及用于各项确认的 `OVERWRITE`、`CLEAN_VOLS`、`WIPE_VOL`、`START_NOW`（`y` 或 `N`）。

数据库和会话**密钥会自动生成**，无需手动输入。它们写入 `deploy/.env`（文件权限 `600`，绝不会提交到 git）。

最后，让脚本启动堆栈：它会等待 API 就绪，**创建你的管理员账号**，并**仅打印一次**生成的密码，所以请在关闭终端前复制。

默认情况下，设置会从 Docker Hub **拉取预构建镜像**：无需 Rust 或 Node 工具链，首次启动只需几分钟。运行 `./setup.sh --build` 则改为从源码构建这三个服务（用于开发、本地修改）。

::: tip 无界面服务器
管理员由 core 自身在首次启动时创建（根据 `deploy/.env`），因此服务器上不需要浏览器。记下打印出的密码，从任何能访问该应用的机器登录即可。
:::

::: warning 覆盖安装旧数据
如果存在上次安装遗留的 Docker 卷，设置会询问是否清除。所有提示的默认选项都是 **No**：只有明确输入 `y` 才会清除（并丢失之前的数据库）。新密钥无法配合旧的数据库卷使用，所以拒绝清除会中止设置，而不是启动一个有问题的堆栈。
:::

## 手动安装（替代方式）

如果你更想手动配置：

```bash
cd deploy
cp .env.example .env
```

编辑 `.env`，至少设置：

- `POSTGRES_PASSWORD`：高强度密码
- `DATABASE_URL`：必须包含相同的密码，例如 `postgres://otw:YOUR_PASSWORD@postgres:5432/opentraderworld`
- `SESSION_SECRET`：一长串随机字符串
- `OTW_SECRET_KEY`：另一长串随机字符串，用于加密已存储凭据的密钥。请保持不变：之后更改会让这些凭据无法读取。

Compose 还需要 `setup.sh` 本会写入的两个网络文件。对于端口 5454 上的 localhost 模式：

```bash
cat > network.env <<'EOF'
OTW_BIND=127.0.0.1
OTW_HTTPS_BIND=127.0.0.1
OTW_HTTP_PORT=5454
OTW_HTTPS_PORT=8443
OTW_DOMAIN=:80
EOF
printf 'OTW_ACME_GLOBAL=\n' > dns.env
```

之后两者都会由**设置 → 网络**重写，参见[网络与远程访问](/zh/config/network)。

然后启动堆栈。默认会从 Docker Hub **拉取预构建镜像**（无需 Rust 或 Node 工具链）：

```bash
docker compose -f docker-compose.yml -f docker-compose.images.yml \
  --env-file .env --env-file network.env up -d
```

::: details 改为从源码构建
用于开发或运行本地修改时，省略镜像覆盖文件，自行构建这三个服务（需要工具链；Rust 构建较慢）：

```bash
docker compose --env-file .env --env-file network.env up --build -d
```
:::

## 创建管理员（仅限手动安装）

如果你使用了 `./setup.sh` 并让它启动了堆栈，则**管理员已存在**，请跳过此步。

否则，在浏览器中打开应用（默认 `http://localhost:5454`）。首次访问时，OpenTraderWorld 会检测到尚无管理员，并显示**设置向导**：选择用户名和密码，提交后进入dashboard。密码至少需要 12 个字符，由重复字符或连续序列组成、包含用户名或出现在已知泄露列表中的密码都会被拒绝。密码使用 argon2 哈希存储。

在 HTTPS 模式（局域网 HTTPS、公网）下，向导还会要求输入**设置令牌**，这样仅仅发现地址的人无法抢先认领账号。core 会在启动时把它打印到日志中：`docker compose logs core`。

::: details 通过 CLI 创建管理员（无界面，无需浏览器）
从堆栈内部访问首次运行的端点：无论你的绑定接口如何都能使用，如果管理员已存在则会拒绝（HTTP 409）。在 HTTPS 模式下，把 core 日志中的设置令牌作为额外的请求头加上，`--header=X-OTW-Setup-Token:<token>`，否则它会返回 403 `setup_token_required`：

```bash
cd deploy
docker compose --env-file .env --env-file network.env exec -T caddy \
  wget -qO- --header=Content-Type:application/json \
  --post-data='{"username":"admin","password":"CHOOSE-A-STRONG-ONE"}' \
  http://core:8080/api/setup
```
:::

## 验证是否在运行

- 应用：`http://localhost:5454`（或你选择的端口/域名）
- 健康检查：`http://localhost:5454/api/health` → `{"status":"ok"}`（登录后还会返回版本、运行时长和每个服务的状态）

```bash
cd deploy
docker compose ps            # container status
docker compose logs -f core  # follow core logs
```

## 日常操作

在 `deploy/` 中运行。每条命令都要带上两个 env 文件，镜像安装还要带上镜像覆盖文件，所以请从与你的安装方式对应的那一行开始：

```bash
# image install (the default)
docker compose -f docker-compose.yml -f docker-compose.images.yml --env-file .env --env-file network.env
# source build
docker compose --env-file .env --env-file network.env
```

| 操作 | 追加 |
|---|---|
| 启动 | `up -d` |
| 停止 | `down` |
| 查看日志 | `logs -f` |
| 修改代码后重新构建（源码构建） | `up --build -d` |
| 停止**并清除所有数据** | `down -v` |

要升级到较新的版本，请按照[更新](/zh/guide/updating)操作：镜像标签固定在 `docker-compose.images.yml` 中，所以单纯的 `pull` 只会拉取你已在运行的版本。

你的数据保存在 Docker 命名卷中，在 `up`/`down` 之间**持续保留**。只有 `down -v` 才会删除它。

## 后续步骤

- [第一步](/zh/guide/first-steps)：登录、设置默认值、安装模块。
- [网络与远程访问](/zh/config/network)：从其他设备访问应用、局域网 HTTPS、公网暴露。
- 出问题了？参见[故障排查](/zh/guide/troubleshooting)。
