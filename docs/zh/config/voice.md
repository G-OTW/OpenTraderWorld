# 语音控制

用声音操控 OpenTraderWorld，或在任意文本字段中听写。**仅限按键说话**：按下时麦克风打开，松开时关闭。两者之间不会监听任何内容。

**默认关闭。**在**设置 → 语音与快捷键 → 语音识别与快捷键**中开启。

## 麦克风需要 HTTPS 或 localhost {#https}

浏览器只会在**安全来源**上向网页提供麦克风（以及其内置语音识别）：即通过 **HTTPS** 提供的页面，或在 **localhost** 上打开的页面。这是浏览器的规则，不是 OpenTraderWorld 的，应用内的任何设置都无法解除。

| 你如何打开 OTW | 语音能用吗？ |
|---|---|
| 在运行它的机器上，`http://127.0.0.1:5454` 或 `http://localhost:5454` | 可以 |
| [LAN + HTTPS](/zh/config/network#lan-https) 或[公网](/zh/config/network#public)模式 | 可以 |
| 在另一台设备上通过纯 HTTP 访问[本地网络（LAN）](/zh/config/network)模式 | **不能**，浏览器会隐藏麦克风 |

要在手机或你网络上的另一台电脑上使用语音，请将**设置 → 网络**切换为 **LAN + HTTPS**。在纯 HTTP 下，语音设置页会显示警告，麦克风按钮会说明为何无法启动。

## 两个快捷键，两种模式

| | 默认值 | 你说的话会怎样 |
|---|---|---|
| **命令** | `Alt+V`（Mac 上为 `⌥V`） | 变成一个操作计划，即使焦点在文本字段中也是如此。 |
| **听写** | `Alt+Shift+V`（`⌥⇧V`） | 逐字输入到当前获得焦点的文本字段中。绝不会被当作命令。 |

说话时按住快捷键，松开即结束，按 `Esc` 取消。**顶栏的麦克风**始终运行命令：点击开始，再次点击停止，也可以像对讲机那样按住。

两个快捷键都可以在**设置 → 语音与快捷键 → 语音识别与快捷键**中更改。每个都需要 `Ctrl`、`Alt` 或 `⌘`（或功能键 `F1` 到 `F12`），这样不会妨碍正常输入，且两者必须不同。

## 语音识别引擎

引擎负责把你的录音转成文字。在**设置 → 语音与快捷键 → 语音识别与快捷键**中选择一个。

| 引擎 | 设置 | 音频去向 |
|---|---|---|
| **此浏览器** | 无 | 浏览器厂商的语音服务（Chrome 为 Google，Edge 为 Microsoft，Safari 为 Apple）。Firefox 不支持。 |
| **自托管 Whisper** | 内置服务，参见[示例：自托管 Whisper](#whisper-example) | 保留在你的机器上 |
| **whisper.cpp 服务器** | 你自己的服务器，其 `/inference` 接口 | 保留在你的服务器上 |
| **OpenAI / Groq** | API 密钥（粘贴，或从[保险库](/zh/config/settings#vault)接入） | 发送给该服务商 |
| **其他** | 任何提供 OpenAI `/audio/transcriptions` API 的服务器 | 该服务器 |

使用服务器引擎时，浏览器负责录音，将音频转换成小的 WAV 文件，再由 OTW 转发给引擎。**测试**会发送半秒钟的静音，以检查 URL、密钥和模型。密钥静态加密，绝不会回传给浏览器。

### 示例：从零开始的自托管 Whisper {#whisper-example}

内置的 Whisper 服务是可选的，普通的 `up` 不会启动它。第 1 到 3 步只需做一次：模型保存在 `whisper-cache` 卷中，重启后仍然保留。

**1. 启动服务**，在仓库根目录执行：

```bash
docker compose -f deploy/docker-compose.yml --env-file deploy/.env --env-file deploy/network.env --profile voice up -d whisper
docker logs -f opentraderworld-whisper-1
```

等待出现 `Uvicorn running on http://0.0.0.0:8000`，然后按 `Ctrl+C`。该端口只有 Docker 内部的 OTW 能访问，浏览器无法访问，这是有意为之。

**2. 下载模型**（约 500 MB，需要几分钟）：

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen(urllib.request.Request('http://localhost:8000/v1/models/Systran/faster-whisper-small',method='POST'),timeout=1800).read())"
```

**3. 检查是否已安装**：返回结果必须列出 `Systran/faster-whisper-small`。

```bash
docker exec opentraderworld-whisper-1 python -c "import urllib.request;print(urllib.request.urlopen('http://localhost:8000/v1/models').read())"
```

**4. 接入 OTW。**在 `http://localhost:5454` 上打开应用（或通过 HTTPS，参见[上文](#https)），然后进入**设置 → 语音与快捷键 → 语音识别与快捷键**：

1. **添加引擎**，模板选择**自托管 Whisper**：它会填入 `http://whisper:8000/v1` 和 `Systran/faster-whisper-small`。
2. **保存并测试**：引擎会显示**正常 · … 毫秒**。
3. 选中该引擎，然后把顶部的开关打到**开启**。也可以设置**语言**。

**5. 试一试**：按住 `⌥V` / `Alt+V`，说 *“open journal”*，松开，按 `Enter`。要听写，请点进某个文本字段，按住 `⌥⇧V` / `Alt+Shift+V`。

重启后的第一次转写会较慢，因为模型需要加载到内存中。

## 语音命令

**设置 → 语音与快捷键 → 语音命令**：一个命令由一个**短语**（外加其他说法）和一个有序的**步骤**列表组成：

- **打开页面**：某个模块、dashboard或设置。
- **询问agent**：发送给浮动助手的提示词，助手使用它自己的工具执行。
- **运行workflow**：启动一个 [Automator](/zh/modules/automator) workflow。
- **主题**、**隐藏数字**：与顶栏中的按钮相同。
- **朗读**：大声读出一句话。

短语只有在**单独说出**时才会触发，而不会作为句子里的一个词触发：听写“a blue turtle”不会运行你的 *turtle* 命令。

### 内置，无需设置

- **“open &lt;page&gt;”**（*“ouvre &lt;page&gt;”*、*“öffne &lt;Seite&gt;”*、*“abre &lt;página&gt;”*、*“apri &lt;pagina&gt;”*、*“打开 &lt;页面&gt;”*）可打开任何已安装的模块、dashboard或设置。如果名称匹配多个页面，会拒绝并列出这些页面，绝不会猜测。
- 开启**其余交给agent**后，没有命令匹配的内容都会作为一个请求交给助手。

### 串联

一次说出几件事，用*并且*或*然后*连接（法语中为 *et*、*puis*，依此类推，取决于**语言**设置）。转写文本中的逗号同样会拆分：

> “turtle, then open settings and compare AAPL and MSFT”

会运行你的 *turtle* 命令，打开设置，然后请agent“compare AAPL and MSFT”。相邻的剩余片段会合并在一起，因此agent会得到完整的请求。

## 确认

每个计划都会在**运行前显示**：听到了什么、每个步骤以及它的来源。`Enter` 运行，`Esc` 取消。步骤按顺序执行，计划在第一次失败时停止，并指出是哪一步。

命令可以标记为**无需确认直接运行**。它**默认关闭**，且只有在该短语单独说出时才适用：与其他内容串联时，仍会先显示计划。agent步骤的每次写入仍保留助手自己的确认。

## 须知

- 助手在**agent**页面上是隐藏的，因此包含agent步骤的计划无法从那里运行。
- **朗读结果**使用浏览器自带的声音：无需设置，音频不会离开浏览器。
- 公开演示以只读方式显示语音页面：不保存设置，也不发送音频。
- 位于 OTW 前面的**你自己的反向代理**不得阻止麦克风：其 `Permissions-Policy` 头需要包含 `microphone=(self)`。若为 `microphone=()`，浏览器会立即拒绝，而不会询问。
