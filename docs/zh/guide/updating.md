# 更新

有新版本时，OpenTraderWorld 会在**设置 → 更新应用**中提示你（它会检查 GitHub）。出于设计，应用**无法自行更新**（它运行时没有 shell 或 Docker 访问权限，以缩小攻击面），因此更新就是在主机上执行几条命令。

## 更新之前

1. **备份数据库**：参见[备份与恢复](/zh/guide/backup-restore)。
2. 浏览发布说明，留意破坏性变更。

## 你是哪种安装？

查看你的安装目录：

- 里面只有 `deploy/`，没有 `.git`：**镜像安装**（一条命令安装程序，或不带 `--build` 的
  `setup.sh`）。这是默认方式。
- 有 `core/`、`frontend/` 和 `.git`：**源码构建**（`install.sh --build`，或克隆仓库
  再运行 `./setup.sh --build`）。

## 镜像安装

不构建任何东西，也没有 git 检出，所以要从发布版本刷新 `deploy/`
（新的镜像标签固定在那里），然后拉取并重启：

```bash
cd /path/to/opentraderworld
TMP=$(mktemp -d)
curl -fsSL https://codeload.github.com/G-OTW/OpenTraderWorld/tar.gz/master | tar -xz -C "$TMP"
cp -R "$TMP"/*/deploy/. deploy/
rm -rf "$TMP"
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  pull
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d
```

你的配置会保留：`.env`、`network.env` 和 `dns.env` 不属于发布内容，因此复制时不会覆盖它们。
`deploy/` 中的其他所有内容都会被新版本替换，这正是目的所在：对 `docker-compose.yml` 或
`Caddyfile` 的本地修改会丢失，如有需要请保存为补丁。

## 源码构建

刷新检出的代码，然后重新构建：

```bash
cd /path/to/OpenTraderWorld
git fetch origin
git reset --hard origin/master
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env \
  up -d --build
```

::: warning 请使用 `git reset --hard`，而不是 `git pull`
每个发布版本都是仓库的全新快照，所以 `git pull` 会报告
“divergent branches” 并失败。`git reset --hard origin/master` 会让你的检出与新版本完全一致。
你的数据和配置不受影响：它们保存在 Docker 卷以及 `.env` / `network.env` 中，这些文件不受 git 跟踪。
如果你在本地编辑过受跟踪的文件，请先暂存（`git stash`）。
:::

如果想拉取已发布的镜像，而不是从你的检出重新构建，请添加
`-f deploy/docker-compose.images.yml`，并像镜像安装那样使用 `pull` + `up -d`。

就这些：

- 容器会以新版本重新创建并重启。
- **数据库迁移会自动运行**，在新 core 容器首次启动时执行。
- 你的数据不受影响：它保存在 Docker 卷中，与镜像无关。

容器重新创建期间，应用会短暂离线。确切的命令（含你配置的路径）也会显示在**设置 → 更新应用**中。

## 一次性操作：AI agent的 OAuth（0.0.16） {#oauth-caddyfile}

MCP 客户端的 OAuth 登录需要 `/.well-known/oauth-*` 下的发现文档能够到达 core。
上面的镜像安装流程和源码构建会带上新的 `deploy/Caddyfile`，新安装也是如此。
通过 `otw update` 更新的安装会保留旧的 Caddyfile：除 OAuth 外一切正常，直到你添加下面这段，
放在 `# Everything else → static frontend` 这一行之前：

```
	handle /.well-known/oauth-* {
		header Content-Security-Policy "default-src 'none'; frame-ancestors 'none'"
		reverse_proxy core:8080
	}
```

然后重新创建 Caddy（仅重新加载不够：大多数编辑器会替换文件，而容器仍保留旧文件）：

```bash
docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d --force-recreate caddy
```

## 一次性操作：停用引导密码 {#retire-bootstrap-password}

如果 core 在启动时记录了下面这条日志，它是在提醒你：

```
OTW_ADMIN_PASSWORD is still set but the admin account already exists.
```

`setup.sh` 会在首次启动时根据 `deploy/.env` 创建管理员，随后清空该行。在此之前创建的安装中，该值仍留在文件里，
也留在容器的环境变量中，任何能访问 Docker 守护进程的人都能通过 `docker inspect` 看到。
此时它不再授予任何权限，只不过是磁盘上多出一份密码副本而已。

```bash
sed -i.bak 's/^OTW_ADMIN_PASSWORD=.*/OTW_ADMIN_PASSWORD=/' deploy/.env
chmod 600 deploy/.env && rm -f deploy/.env.bak
```

然后重新创建 core，使其也从环境变量中移除：

```bash
docker compose -f deploy/docker-compose.yml \
  --env-file deploy/.env --env-file deploy/network.env up -d core
```

保留 `OTW_ADMIN_USER`：密码为空会让整个引导流程不执行任何操作，这正是你想要的，
并且该行仍然说明了无界面安装是如何创建第一个账号的。
