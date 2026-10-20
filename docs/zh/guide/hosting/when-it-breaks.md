# 出问题时

人们实际遇到的问题，按它们通常出现的顺序排列。
每一项：你看到什么、原因、该怎么做。

::: tip 永远先试这个
如果安装中断了，**再运行一次同样的那一行命令**。
它会记住已经成功的步骤，并从中断处继续。
它绝不会重复问同一个问题。
:::

## 安装期间

### “The name … does not lead anywhere yet”

**原因：**你的地址（例如 `app.example.com`）尚未与你的机器连接。

1. 登录你购买该地址的公司。
2. 打开它的 **DNS** 页面（有时叫 **Zone** 或 **DNS records**）。
3. 按消息打印的内容原样添加一条记录：

   | 类型 | 名称 | 值 |
   |---|---|---|
   | `A` | 消息中显示的那个词（裸地址用 `@`） | 消息中显示的那串数字 |

4. 保存。
5. 等待五分钟。
6. 再运行一次同样的那一行命令。

::: details “名称”框是最常见的出错点
对于 `example.com`，输入 `@`（有些网站要求留空）。
对于 `app.example.com`，只输入 `app`，而不是完整地址。
:::

### “The name … does not lead to this machine”

**原因：**地址指向了别处：旧机器，或者出售该地址的公司的停放页面。

1. 打开同一个 **DNS** 页面。
2. 删除该名称下所有其他的 `A` 记录。
3. 只保留值为消息所打印内容的那一条。
4. 等待五分钟，然后再运行一次同样的那一行命令。

如果你只是几分钟前才修改的，当安装程序提示等待时回答 **yes**。
它每 20 秒检查一次，最多 10 分钟。

### “The address https://… is not answering yet”

**原因：**几乎总是下面两种情况之一。

- 你只在几分钟前才把地址指向这台机器。**请等待十分钟**，然后再运行一次同样的那一行命令。
- 你的服务商在机器前面有自己的防火墙，而且是关闭的。

要打开服务商的防火墙：

1. 打开服务商的控制面板。
2. 找到这台机器的**防火墙**或**安全**设置。
3. 允许传入的 **TCP 80** 和 **TCP 443**。
4. 再运行一次同样的那一行命令。

### “Something on this machine is already answering on port 80 / 443”

**原因：**你的服务商替你在机器上安装了 Web 服务器，占用了 OpenTraderWorld 需要的位置。

粘贴下面的命令，然后再运行一次同样的那一行命令：

```bash
systemctl disable --now apache2 nginx caddy 2>/dev/null; true
```

### “This machine has … MB of memory” 或 “Only … MB of disk space is free”

**原因：**机器配置太小。OpenTraderWorld 需要大约 **2 GB 内存**和 **8 GB 可用磁盘**。

1. 在服务商处把机器升级到更大的套餐。
2. 再运行一次同样的那一行命令。

### “This installer only knows Ubuntu and Debian”

**原因：**这台机器是用其他系统创建的。

1. 在服务商处用 **Ubuntu 24.04** 或 **Debian 13** **重装**（或**重建**）机器。
2. 再运行一次同样的那一行命令。

### “This needs the machine's administrator rights”

**原因：**你登录的账号无权安装软件。

在中间加上 `sudo` 再运行一次该命令，完全按消息所示：

```bash
curl -fsSL https://get.opentraderworld.com/configure_install.sh | sudo bash -s -- --domain app.example.com
```

## 安装之后

### 页面打不开了

1. 登录你的机器。
2. 输入：

   ```bash
   otw status
   ```

3. 如果显示 **Nothing is running** 或 **is not answering**，输入：

   ```bash
   otw restart
   ```

4. 等待一分钟，然后重新加载页面。

### 我无法再登录机器本身

**原因：**安装程序收紧了机器的登录方式。

- 如果你选择了**密钥**：请从安装当天使用的同一台电脑登录。密码会被有意拒绝。
- 如果你选择了**密码**：请使用卡片上显示的账号名登录，**而不是** `root`。直接用 `root` 登录会被有意拒绝。

丢了那台电脑或那个密码？请使用服务商控制面板中的**控制台**（有时叫 **VNC**、**救援**或 **Web 终端**）。即使正常的登录途径被关闭，它也能用。

### 我忘记了 OpenTraderWorld 的密码

1. 登录你的机器。
2. 输入（如果你改过登录名，请把 `admin` 替换为你的登录名）：

   ```bash
   docker exec -it opentraderworld-core-1 /app/otw-core reset-password admin
   ```

3. 用它打印出的密码登录应用。应用会要求你选择新密码。

要再次查看你的地址和登录名，输入 `otw card`。

### 浏览器显示“不安全”或拒绝打开页面

- 在地址前加上 `https://`。
- 如果是在安装后几分钟才开始出现，请等待十分钟：小锁证书仍在签发中。
- 如果是家庭安装（而不是租用的服务器），参见[故障排查](/zh/guide/troubleshooting)。

### 机器满了

**原因：**每晚的备份和下载的价格历史会随时间占用空间。

1. 输入 `otw status` 查看剩余空间。
2. 在服务商处给机器更大的磁盘。
3. 输入 `otw restart`。

## 仍然解决不了？

1. 登录你的机器。
2. 输入：

   ```bash
   otw report
   ```

3. 它会写出一个文件并打印其位置。该文件不含任何密码。
4. 在 [GitHub](https://github.com/G-OTW/OpenTraderWorld/issues) 上提交 issue，说明你当时在做什么，并附上该文件。
