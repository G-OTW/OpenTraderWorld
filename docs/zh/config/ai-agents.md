# AI agents (MCP)

OpenTraderWorld 内置了一个 **MCP 服务器**，让 AI agent（任何兼容 [MCP](https://modelcontextprotocol.io) 的客户端）可以通过受控的 gateway 读取和更新你的模块。agent 可以替你记录 journal 交易、总结你的新闻源、添加待办、查询你的 backtest 结果，等等。

**默认关闭。**在你启用之前，不会有任何东西监听 agent。

::: tip 想找的是应用内的聊天助手？
本页讲的是**外部** agent *接入* OpenTraderWorld。如果你想要的是应用内置的聊天助手（自带服务商），请参阅 [Agent 模块](/zh/modules/agent)：它可以*使用*同一个 gateway 来访问你的数据。
:::

## 安全模型 {#security-model}

多层机制，必须全部通过：

1. **全局开关**：在你于**设置 → MCP** 中启用之前，MCP 端点是禁用的。关闭时你也可以先准备 token；启用之前，每个 agent 请求都会被拒绝。
2. **Bearer token**：每个 agent 或用例一个。token 以**哈希**形式存储，创建时只显示**一次**；失败的尝试会被限速。可随时吊销 token。
3. **按 token 的模块权限**：每个 token 针对**每个模块**授予*无权限*、*读取*、*读取 + 写入*或*完全（读取 + 写入 + 删除）*。agent 只能发现你授权的模块。
4. **硬性允许列表**：账户、网络、机密、文件存储和数据清除操作，无论权限如何，**绝不会向 agent 暴露**。

::: tip 允许列表是有意设计的，而非自动生成
agent 能访问某个端点，仅仅是因为有人手动把它加入了目录。新模块，或现有模块上的新路由，在该条目存在之前**对所有 agent 都不可见**，因此随着应用的增长，gateway 绝不会意外扩大。persona 和技能管理被有意排除在外：没有任何 agent、也没有任何 agent 读到的内容，能编辑 persona 或扩大其技能架。
:::

::: tip 版本就像 commit
在**设置 → 版本管理**中开启版本管理后，被授予 **Editor** 或 **Backtest** 的 agent 可以在更新文档或策略之后保存一个版本，并附上说明改了什么的备注，还可以列出、读取、恢复或删除版本。它可以按文件或按策略切换版本管理，但不能更改设置中的全局开关。
:::

::: warning Automator 授予的是编写，而不是启用
授予 **Automator** 后，agent 可以读取你的 workflow、创建一个、编写其图并测试它。但它**不能**让其投入使用：agent 保存的图会作为草稿落地，由你在编辑器中采纳；它无法附加 workflow 的访问 token，也无法运行 workflow 或触碰计划任务。workflow 使用它自己的 token 而非调用者的 token 运行，所以编写图和启用它是两项独立的授权。参见[模块页面](/zh/modules/automator#letting-an-agent-build-a-workflow)。
:::

## 启用并创建 token

1. 前往**设置 → MCP** 并将其打开。
2. **新建 token**：以客户端命名（例如 `My Agent`），设置按模块的权限（或以 *全部读取* / *全部读取+写入* / *全部完全* 作为起点）。
3. **立即复制 token**：它只显示一次。

**允许外部访问**是同一对话框中的一个独立勾选项。它不会额外授予任何权限：只是让该 token 可以支撑[外部控制](/zh/config/external-control)中的聊天绑定，来自 Telegram、Slack 或 Discord 的消息会按这些相同的按模块级别运行。

创建对话框还会显示一段**可直接粘贴的配置片段**，每个客户端系列一个标签页：原始端点 + 请求头、一个 `mcpServers` JSON 块（Cursor、Cline、Windsurf、VS Code…），以及一行 `claude mcp add` 命令。同样的片段会一直显示在 token 表下方，以 `<TOKEN>` 作为占位符，方便你之后配置第二台机器。

## 连接客户端

该端点在以下地址使用 **基于 Streamable HTTP 的 MCP**：

```
POST http://<your-host>/api/mcp
Authorization: Bearer <TOKEN>
```

任何合规的客户端都可以使用。MCP 配置示例：

```json
{
  "mcpServers": {
    "opentraderworld": {
      "type": "http",
      "url": "http://localhost:5454/api/mcp",
      "headers": { "Authorization": "Bearer <TOKEN>" }
    }
  }
}
```

如果你使用局域网/HTTPS 模式，请把 URL 替换为你的域名。

::: tip 仅限 localhost 的安装
如果应用只能通过 `localhost` 访问（默认网络模式），agent 必须运行在**同一台机器上**。
:::

## 通过 OAuth 连接（claude.ai、ChatGPT）

有些客户端无法持有固定 token：claude.ai 和 ChatGPT 的 connector 只支持用 OAuth 登录。对于它们，请在**设置 → MCP** 底部打开 **OAuth sign-in**（会要求输入你的密码），并只把服务器 URL `https://<your-domain>/api/mcp` 提供给客户端，不带 token。

1. 客户端自行注册，并在你的实例上打开授权页面（如有需要请先登录）。
2. 页面会显示客户端的名称以及**你的回应会被发送到哪里**。选择模块和级别，然后点击**允许**：每次批准都会要求输入你的密码。
3. 该连接会出现在 token 表中，带有 **OAuth** 标记。可以像对待任何 token 一样在那里编辑其权限或吊销；吊销会断开客户端。

访问 token 有效期为一小时，会在后台续期；30 天未使用的连接需要重新登录。如果续期 token 被其他人重放，该连接会被吊销，并通知你。

::: warning 远程客户端需要公网 HTTPS
claude.ai 和 ChatGPT 从它们自己的服务器发起连接，因此实例必须可通过公网 HTTPS 访问（[Web 模式](/zh/config/network)）。只批准你自己刚刚打开的授权页面：别人发来的链接可以自称为任何客户端。
:::

::: tip 从 0.0.15 或更早版本通过 `otw update` 更新？
OAuth 需要在 `deploy/Caddyfile` 中添加一条新路由。参见[更新](/zh/guide/updating#oauth-caddyfile)。
:::

## agent 如何看到应用

agent 获得四个 gateway 工具：

- **`otw_catalog`**：列出该 token 可调用的模块和操作。只显示已授权的模块。模块列表会显示每个操作的方法、路径、查询参数和顶层请求体字段；`endpoint`（`POST /api/backtest/run`）会返回该操作的完整请求体 schema。
- **`otw_read`**：读取操作（需要该模块至少 *读取* 权限）。
- **`otw_compute`**：目录中标记为 *(compute)* 的操作，它们只回答问题而不存储任何内容：backtest、参数扫描、风险指标。它们和任何 POST 一样需要 *读取 + 写入*，但你的客户端不会要求你批准一次计算。
- **`otw_write`**：创建和更新操作（需要 *读取 + 写入*）；**删除**操作需要该模块的 *完全* 权限。

携带外部文本（订阅源文章、收到的邮件）的响应，会放在带标签的块中交给 agent，提示它将内容视为数据，并忽略其中隐藏的任何指令。

设置中的 token 表会显示每个 token 最后一次使用的时间，方便你发现并吊销陈旧的 token。创建或编辑 token 时还可以给它设置过期日期：过了该日期，它在所有地方都会失效，包括应用内的 agent。
