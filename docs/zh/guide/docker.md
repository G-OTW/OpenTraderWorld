# 获取 Docker

OpenTraderWorld 以一组 Docker 容器的形式发布，这也是目前**唯一受支持的运行方式**。如果你清楚自己在做什么，也可以原生安装（直接在主机上运行 Rust core、PostgreSQL 和前端），但**不推荐，也没有文档**。优先使用 Docker 是有意为之：

- **不侵入系统**：除 Docker 本身外，系统上不会安装任何东西。应用、数据库和代理都在容器中，你的数据保存在命名卷里。清除一切只需 `docker compose down -v`，再删除文件夹。
- **处处一致**：同一套堆栈在 macOS、Linux 和 Windows 上原样运行。
- **更新快速**：刷新仓库并拉取新镜像即可（参见[更新](/zh/guide/updating)）；损坏的容器几秒钟内即可重建，且不会触及你的数据。

如果你已经装好了 Docker，请直接跳到[安装](/zh/guide/install)。

## macOS

安装 **Docker Desktop**：

- 从 [docker.com](https://www.docker.com/products/docker-desktop/) 下载（选择 Apple Silicon 或 Intel），打开 `.dmg` 并将 Docker 拖入“应用程序”，或使用 Homebrew：

  ```bash
  brew install --cask docker
  ```

- 从“应用程序”中启动一次 **Docker**，等待其启动完成（菜单栏的鲸鱼图标停止动画）。

已包含 Docker Compose。

## Windows

安装使用 WSL 2 后端的 **Docker Desktop**：

1. 要求：64 位 Windows 10/11 并启用 **WSL 2**，必要时在管理员 PowerShell 中启用：`wsl --install`，然后重启。
2. 从 [docker.com](https://www.docker.com/products/docker-desktop/) 安装 Docker Desktop，或：

   ```powershell
   winget install Docker.DockerDesktop
   ```

3. 启动 Docker Desktop，并保持默认的 *Use WSL 2* 设置。

OpenTraderWorld 的命令可以在任意终端（PowerShell 或 WSL shell）中运行。已包含 Docker Compose。

## Linux

无论桌面版还是无界面服务器，都安装 **Docker Engine**（无需 Desktop）。便捷脚本适用于所有主流发行版：

```bash
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER   # run docker without sudo
newgrp docker                    # or log out and back in
sudo systemctl enable --now docker
```

更想用发行版自带的软件包？请参阅[官方按发行版划分的说明](https://docs.docker.com/engine/install/)。较新的 Engine 安装已包含 Compose 插件。

## 验证

```bash
docker --version
docker compose version
docker run --rm hello-world
```

三条命令都成功 → 准备就绪。

## 部署 OpenTraderWorld

一条命令，然后按提示操作：

```bash
curl -fsSL https://raw.githubusercontent.com/G-OTW/OpenTraderWorld/master/install.sh | bash
```

完整的流程说明（提示的含义、选项、手动替代方式、结果验证）见[安装](/zh/guide/install)页面。

::: info 预构建镜像
安装时会从 Docker Hub **拉取预构建镜像**：无需构建，也无需 Rust/Node 工具链。开发时仍可从源码构建（`install.sh --build`，或在克隆的仓库中运行 `./setup.sh --build`）。
:::
