# 模块概览

OpenTraderWorld 由一组**模块**构成，这些功能包可在**设置 → 模块**中单独开启。所有模块都随应用一起提供；安装模块会让它出现在模块切换器（左上角）和 dashboard 中。卸载模块可再次隐藏它（除非你同时删除，否则其数据会保留）。

[dashboard、搜索和通知](/zh/modules/dashboard)位于所有模块之上，始终存在。

## 依赖关系

有些模块建立在 **Historical Data** 的数据集目录之上，需要先安装它：

```
Historical Data ──▶ Historical Data Visualization
                ──▶ Backtest
                ──▶ Quant Tools
```

其他一切都是独立的，不过当两个模块都已安装时，有些会相互集成（例如 Tax Calculator 可以导入 Trading Journal 的盈亏；MyWealth 可以导入 Portfolio Tracker 的持仓；Calendar 可以显示 ToDo、Goals 和 Reminders）。

Historical Data、Visualization、Watchlists、Fundamentals 和 Trading Journal 还共用一份 **[data connector](/zh/config/connectors)** 列表：服务商账户只需创建一次，然后授权给可以使用它的模块。

## 所有模块

### 交易

| 模块 | 作用 |
|---|---|
| [Trading Journal](/zh/modules/journal) | 带模板、手续费方案、多币种汇率和绩效统计的交易记录。 |
| [Trading Routines](/zh/modules/productivity#routines) | 周期性的交易时段检查清单：盘前准备、盘中纪律、盘后复盘。 |
| [Mindset](/zh/modules/productivity#mindset) | 每日情绪与纪律打卡，附带趋势。 |

### 行情数据与分析

| 模块 | 作用 |
|---|---|
| [Historical Data](/zh/modules/market-data#histdata) | 从多个服务商下载 OHLCV 历史数据到本地数据集。 |
| [Historical Data Visualization](/zh/modules/market-data#histviz) | K线/OHLC/折线/Renko 图表的工作区，带指标、绘图、对比和服务端监控的警报，可实时或按需查看 connector 所支持的任何品种。 |
| [Backtest](/zh/modules/market-data#backtest) | 基于规则的策略 backtester，支持仓位管理、成本和完整统计。 |
| [Quant Tools](/zh/modules/market-data#quant) | 数据集的风险、统计、波动率和行情状态；配对、篮子和因子回归；对已保存 backtest 的过拟合检验；仓位管理、计算器、期货曲线和期权波动率曲面。 |
| [Fundamentals](/zh/modules/fundamentals) | 宏观序列、公司财报、SEC 申报文件、电话会议记录、ETF、市场日历，以及来自一手来源和你所连接聚合器的另类数据。 |

### 投资组合与资金

| 模块 | 作用 |
|---|---|
| [Watchlists](/zh/modules/portfolio#watchlists) | 带实时价格、当日涨跌、迷你走势图和备注的品种自选清单。 |
| [Portfolio Tracker](/zh/modules/portfolio#portfolios) | 实时市值、现金和收入账本、相对所承担风险的绩效、配置偏离和压力测试。 |
| [MyWealth](/zh/modules/portfolio#wealth) | 你拥有和欠下的所有内容的净资产，投资组合实时读取而不是复制。 |
| [Managers' Portfolios](/zh/modules/portfolio#mportfolios) | 超级投资者的 13F 持仓，可浏览并可生成快照。 |
| [Tax Calculator](/zh/modules/portfolio#taxcalc) | 基于各国模板的交易与投资税务估算。 |
| [Subscriptions](/zh/modules/portfolio#subscriptions) | 周期性订阅与支出概览。 |

### 新闻与研究

| 模块 | 作用 |
|---|---|
| [News](/zh/modules/news-research#news) | 带轮询 dashboard 的 RSS 与 JSON-API 新闻聚合器。 |
| [Mailbox](/zh/modules/news-research#mailbox) | 从你自己的 IMAP 邮箱读取资讯简报、市场新闻和 broker 邮件，无追踪器。 |
| [Economic Calendar](/zh/modules/news-research#economics) | 即将到来的宏观事件。 |
| [FinanceDatabase](/zh/modules/news-research#findb) | 在本地搜索 300,000+ 个品种；用文件夹整理收藏。 |
| [Resources](/zh/modules/news-research#resources) | 书籍、链接和参考资料的书签库。 |
| [Community Docs](/zh/modules/news-research#community-docs) | 社区撰写的指南，可同步并离线阅读。 |

### 笔记与组织

| 模块 | 作用 |
|---|---|
| [Editor](/zh/modules/productivity#editor) | 带文件夹和表格/看板/画廊数据库的富文本文档编辑器。 |
| [ToDo](/zh/modules/productivity#todos) | 带截止日期和分类的任务列表。 |
| [Goals](/zh/modules/productivity#goals) | 带指标跟踪和截止日期的目标。 |
| [Calendar](/zh/modules/productivity#calendar) | 个人事件日历；叠加显示提醒、待办和目标。 |
| [RemindMe](/zh/modules/productivity#remindme) | 带应用内通知以及邮件/Telegram/Slack/Discord 渠道的提醒。 |
| [Time Tracker](/zh/modules/productivity#time) | 带预算和时薪价值的项目计时器。 |
| [Prompt Store](/zh/modules/productivity#prompt-store) | 可搜索的可复用 AI 提示词库，支持标签、评分和版本管理。 |
| [Webhooks](/zh/modules/productivity#webhooks) | 私有的入站 URL，把外部警报转为通知。 |
| [Automator](/zh/modules/automator) | 基于你自己的 API 和外部世界的 workflow，可手动或按计划运行。 |

### AI

| 模块 | 作用 |
|---|---|
| [Agent](/zh/modules/agent) | 内置 AI 聊天助手（自带服务商），还可通过 MCP 操作你的数据，具备记忆、技能和外部 MCP 服务器。 |
