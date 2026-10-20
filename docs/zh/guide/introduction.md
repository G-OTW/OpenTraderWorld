# 什么是 OpenTraderWorld？

> 项目网站：**[opentraderworld.com](https://opentraderworld.com)**：模块导览、
> [在线演示](https://demo.opentraderworld.com)、[社区指南](https://opentraderworld.com/docs)
> 和[路线图投票](https://opentraderworld.com/suggestions)。

OpenTraderWorld 是一个面向交易者和投资者的**自托管 Web 平台**。你只需用 Docker 在自己的电脑或服务器上安装一次，在浏览器中打开，就能获得一个由模块组成的私有工作区：Trading Journal、带图表和backtest的历史行情数据、投资组合与净资产跟踪、新闻聚合器、笔记、检查清单等。

**对所有人免费，无论个人还是商业用途。源码可见（FSL-1.1-MIT）。** 唯一不允许的是转售，或将其作为付费服务提供。指导原则：*先盈利，再花钱。*

## 为什么选择自托管？

- **数据始终属于你。** 交易、投资组合、笔记和journal 条目都保存在你自己机器上的 PostgreSQL 数据库中，而不是别人的服务器上。
- **默认私有。** 安装后，应用只监听 `localhost`。是否向局域网或互联网开放，由你在[设置 → 网络](/zh/config/network)中明确选择。
- **无需订阅。** 核心工具零成本运行。部分模块可以选择使用外部数据服务商（许多提供免费额度），API 密钥由你自己提供。

## 工作原理

一个 `docker compose` 堆栈，四个服务：

| 服务 | 作用 |
|---|---|
| **core** | Rust API 服务器（Axum）：全部业务逻辑、调度器、后台任务 |
| **postgres** | PostgreSQL：你的数据唯一存放的地方 |
| **frontend** | SvelteKit 单页应用，部署时构建一次 |
| **caddy** | 反向代理：提供应用页面、代理 `/api`、处理 HTTPS 证书 |

该应用是**单用户**的：一个管理员账号，在安装时创建。没有多租户模式，也没有需要配置的共享或用户管理功能。

Docker 目前是**唯一受支持的部署方式**：它让安装不侵入系统，并且能快速重建（[原因以及如何获取 Docker](/zh/guide/docker)）。原生安装可行，但不推荐。

## 模块

模块是功能包，可在**设置 → 模块**中安装或卸载。所有模块都随应用一起提供，安装模块只是将其启用。重点模块：

- **[Trading Journal](/zh/modules/journal)**：用模板记录交易，支持手续费方案、多币种盈亏和完整的绩效统计。
- **[行情数据与backtest](/zh/modules/market-data)**：从多个服务商下载 OHLCV 历史数据，带指标地实时或按需绘制任意品种的图表，backtest基于规则的策略，并对数据集、已保存的backtest、期货曲线和期权链运行量化分析。
- **[投资组合与财富](/zh/modules/portfolio)**：实时投资组合跟踪、净资产历史、超级投资者 13F 持仓、税务估算。
- **[新闻与研究](/zh/modules/news-research)**：RSS/API 新闻dashboard、经济日历、包含 30 万个品种的搜索目录。
- **[笔记与组织](/zh/modules/productivity)**：带数据库的富文本编辑器、待办、目标、日历、提醒、交易例程和心态打卡。
- **[AI agent](/zh/modules/agent)**：内置聊天助手（自带服务商），可通过 MCP 操作你的数据，具备记忆、技能和外部 MCP 服务器。

查看[完整模块列表](/zh/modules/)。

## 后续步骤

1. [安装 OpenTraderWorld](/zh/guide/install)：使用 Docker 约需 5 分钟。
2. [迈出第一步](/zh/guide/first-steps)：登录、选择默认设置、安装模块。
3. [配置网络访问](/zh/config/network)：如果你想从其他设备访问。

还没准备好安装？试试[在线演示](https://demo.opentraderworld.com)，这是一个预置数据的共享实例，每 15 分钟重置一次。[什么是演示模式](/zh/guide/demo)，以及它会限制哪些操作。
