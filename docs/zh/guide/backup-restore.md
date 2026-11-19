# 备份与恢复

一切都存放在一个 PostgreSQL 数据库中，所以备份就是一次 `pg_dump`。应用中的**设置 → 备份与恢复**会显示相同的命令。请在部署堆栈的主机上、在安装目录（包含 `deploy/` 的目录）中运行；它们使用现有的 Postgres 容器，无需额外权限。

该分区有两个标签页，每个都分为**备份**和**恢复**：

- **完整**：整个数据库，在主机上执行，用于机器损坏的情况。
- **部分**：你勾选的模块，打包为一个 zip，用于迁移或保留可读副本。

## 完整备份

普通转储：

```bash
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner > otw-backup-$(date +%F).sql
```

### 加密（推荐）

转储中的数据是明文。通过管道交给 `gpg`（或 `age`），让磁盘上的文件处于加密状态。系统会提示你输入口令；`GPG_TTY` 和 `--pinentry-mode loopback` 让该提示在转储填充 gpg 输入的同时仍能到达你的终端：

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml exec -T postgres \
  pg_dump -U otw -d opentraderworld --no-owner \
  | gpg --symmetric --cipher-algo AES256 --pinentry-mode loopback \
  > otw-backup-$(date +%F).sql.gpg
```

### 自动备份 {#automatic-backup}

当主机上的备份脚本向应用报告时（[服务器安装程序](#server-installs)会设置一个），**完整**标签页会以一张**自动备份**卡片开头：上次备份距今多久、副本发送到了哪里、是否经过检查、计划时间和大小。上次运行失败，或超过 48 小时没有备份时，它会变为警告。没有这样的脚本就没有卡片，上面的命令就是全部。

## 安全说明

- **API 密钥和服务商凭据**（新闻源、行情数据服务商）已使用 `OTW_SECRET_KEY` 静态加密，因此在转储中只以密文形式出现。
- **单独**备份 **`OTW_SECRET_KEY`**（来自 `deploy/.env`），不要放在同一个转储里，否则这些加密的密钥无法恢复。
- 转储包含有效的**会话token**。请把该文件当作机密对待，或在恢复后清空 `sessions` 表（`DELETE FROM sessions;`）并重新登录。不要删除该表：登录需要它。
- 将加密后的备份存放在**异地**，并轮换旧副本。

## 部分（按模块）

**部分**标签页会取你勾选的模块，给你**一个 zip 文件**，用于把 journal 迁移到另一个实例或保留可读副本。与完整备份不同，它在应用运行时即可执行。

### 部分备份

1. 打开**设置 → 备份与恢复 → 部分 → 备份**。
2. 勾选你想要的模块。每个模块都会显示其行数和大小，已勾选的合计显示在列表下方，*所选内容明细*会按表逐一列出。历史 K 线默认不勾选：它们是迄今为止最大的表，并且可以从你的服务商重新下载。
3. 除非你清楚为什么需要，否则请保持**包含已存储的服务商凭据**关闭。这些值使用本实例的 `OTW_SECRET_KEY` 加密，在其他任何地方都无法读取。
4. 点击**下载所选数据**。你会得到 `otw-data-YYYY-MM-DD.zip`。

zip 内部：`manifest.json`（包含的内容、由哪个版本写入）以及每张表一个 `tables/<name>.jsonl`，每行一个 JSON 对象。任何工具都能读取。

### 部分恢复

1. 在目标实例上打开**设置 → 备份与恢复 → 部分 → 恢复**并选择文件。
2. 选中文件后会立即读取，但不会写入任何内容：你会看到写入它的版本，以及按模块和按表列出的该文件所含行数与当前已有行数的对比。很大的表会以估算值显示，标有 `~`。本实例会拒绝的文件（已损坏，或来自更新的版本）会在这一步被拒绝，不会让你做出任何确认。
3. 选择它与现有数据的合并方式：
   - **添加缺失项**保留所有现有内容，只添加尚不存在的行。不会覆盖任何内容。
   - **替换**会清除文件中每个模块的数据，然后载入文件中的版本。它会要求你输入 `REPLACE`，并先保存当前数据的副本。

   选项下方的那一行会把这些计数转换为将要发生的事情。替换是精确的：*清除此处的 N 行，用文件中的 M 行取而代之*。合并只能给出上限，*最多添加 M 行*：键已存在的行会被跳过，具体数量只有加载过程本身才知道。
4. 点击**加载此文件**。

一切都在一个事务中完成：任何部分失败，都不会有任何改动。

::: warning 来自更新版本的文件会被拒绝
加载由更新版本写出的包会被直接拒绝，而不是尝试加载。请先更新实例，再加载。
:::

行会保留其原始标识符，整个文件会在内存中读取，因此很大的选择（通常是历史 K 线）会被拒绝，并附带消息指向上面的 `pg_dump` 备份。对于“所有内容，包括我从不查看的部分”，那才是合适的工具。

## 完整恢复

转储只能恢复到空数据库中。core 每次启动都会运行迁移，因此它已经启动过的数据库（任何正在运行的堆栈，包括 `setup.sh` 刚创建的）会与转储冲突。停止 core，用一个空数据库替换现有数据库，载入转储，再启动 core：

```bash
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q < otw-backup-YYYY-MM-DD.sql
docker compose -f deploy/docker-compose.yml start core
```

`DROP DATABASE` 这一行会清除堆栈当前保存的内容：如果这些内容重要，请先转储一份。`ON_ERROR_STOP=1` 会在第一个错误处停止，而不是只载入一半转储。

从加密备份恢复：

```bash
export GPG_TTY=$(tty)
docker compose -f deploy/docker-compose.yml stop core
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "DROP DATABASE IF EXISTS opentraderworld"
docker compose -f deploy/docker-compose.yml exec -T postgres \
  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"
gpg -d otw-backup-YYYY-MM-DD.sql.gpg \
  | docker compose -f deploy/docker-compose.yml exec -T postgres \
    psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q
docker compose -f deploy/docker-compose.yml start core
```

请确保恢复后的堆栈使用与备份时**相同的 `OTW_SECRET_KEY`**，否则已存储的服务商凭据将无法读取。

## 服务器安装 {#server-installs}

用[服务器安装程序](/zh/guide/hosting/when-it-breaks#installer-options)设置的机器有 `otw` 命令，它会替你完成上述所有操作：

| 命令 | 作用 |
|---|---|
| `otw backup` | 立即备份到 `/var/backups/otw`，并检查文件是否完整。 |
| `otw restore` | 列出机器上的备份，询问要用哪一个，在你输入 `RESTORE` 后将其恢复。它会先对当前数据做一次完整备份，然后重启 core。 |

安装程序还会安排 `otw backup` 每晚大约 03:20 运行，[自动备份](#automatic-backup)卡片正是由此点亮。每晚的副本不包含两张可以重新下载的表（品种目录和历史 K 线）；周日的副本是完整的。保留最近 10 份每晚副本和 2 份完整副本。它们留在这台机器上：请复制到别处，以便在服务器损坏时仍能保留。
