# Data connectors

所有读取行情数据的模块，都从**一份共享的 connector 列表**中获取数据。服务商账户只需创建一次，然后授权给可以使用它的模块。

可在**设置 → Data connectors**、独立的 **/connectors** 页面，或任何数据模块放在其服务商选择器旁边的 connector 按钮中管理它们。这三处显示的是同一个界面。

## 什么是 connector

**connector 是服务商的一个命名实例**，而不是服务商本身。它包含四项内容：

- **服务商**：Binance、Yahoo Finance、EODHD…；
- **其凭据**：直接输入，或从[保险库](/zh/config/settings#vault)接入。只写：应用只知道设置了*哪些*机密名称；
- **可选的请求限额**：每个周期内的最大调用次数；
- **允许使用它的模块**：一个或多个，或*所有模块*（通配符，也涵盖未来版本中新增的数据模块）。

同一服务商的多个 connector 可以共存。这正是意义所在：一个用于绘图的只读密钥，和另一个用于批量下载的密钥，各有各的限额，各自授权给不同的模块。

## 服务商

| 服务商 | 凭据 | 资产类型 | 品种搜索 | 实时流 |
|---|---|---|---|---|
| **Binance** | 无 | 加密货币 | 是 | 是 |
| **Binance USDⓈ-M Futures** | 无 | 加密货币 | 是 | 是 |
| **Bitget** | 无 | 加密货币 | 是 | 是 |
| **OKX** | 无 | 加密货币 | 是 | 是 |
| **Kraken** | 无 | 加密货币 | 是 | 是 |
| **Coinbase** | 无 | 加密货币 | 是 | 是 |
| **OANDA** | `api_token`（+ 账户 ID） | 外汇和 CFD | 是 | 否 |
| **Yahoo Finance** | 无 | 股票、ETF、指数、加密货币 | 是 | 否 |
| **Alpha Vantage** | `api_key` | 股票、ETF、加密货币、外汇 | 是 | 否 |
| **EODHD** | `api_key` | 股票、ETF、外汇、加密货币 | 是 | 否 |
| **Alpaca** | `api_key`、`api_secret` | 股票、加密货币、期权 | 是 | 日内 |
| **Massive (Polygon.io)** | `api_key` | 股票、ETF、期权、期货、加密货币、外汇、指数 | 是 | 日内，付费计划 |
| **TradeStation** | `client_id`、`client_secret`、`refresh_token` | 股票、ETF、期权、期货、指数 | 按代码 | 否 |
| **FOREX.com (StoneX)** | `username`、`password`、`app_key` | 外汇和 CFD | 是 | 否 |
| **Capital.com** | `api_key`、`identifier`、`api_password` | 外汇、指数、股票、加密货币（均为 CFD） | 是 | 是 |
| **Interactive Brokers** | 无（主机 + 端口） | 股票、ETF、加密货币、外汇、指数、期货、期权 | 是 | 日内 |

无需密钥的服务商在创建 connector 的那一刻即可使用。每个服务商行都链接到它自己的 API 文档，并附有关于其速率限制的说明；支持流式传输的 connector 还附有关于实时数据成本的说明。

### 实时流 {#live-streaming}

实时数据的覆盖范围比下载窄，而且是有意如此。

- 加密货币交易所为每个周期发布一个 K 线频道，所以它们能下载的**每一个**时间框架也都能流式传输，包括日线：在 24/7 市场上，日线 K 线*就是*纪元日。Bitget 和 OKX 把它们自己的日线和周线 K 线锚定在香港时间午夜，因此下载和实时流都会请求它们与 UTC 对齐的变体，这样在那里下载的序列就能与在其他任何地方下载的序列对齐。
- **有三家服务商在这里不支持流式传输。**OANDA 和 TradeStation 通过长连接 HTTP 响应发布实时价格，FOREX.com 通过 Lightstreamer；这三者都不是实时图表使用的 WebSocket。它们的历史下载和账户端可以工作；实时 K 线不行。
- Alpaca、Massive 和 Interactive Brokers 各发布一种粒度（分别是一分钟柱、一分钟聚合和五秒柱），图表的时间框架由它折叠而来。这使得每个**日内**时间框架都可用，而把**日线和周线留给下载**：股票交易时段不是 1440 个与纪元对齐的分钟，因此以那种方式构建的日线 K 线会与下载所存储的不一致。图表会如实说明，而不是隐藏该控件。
- 实时数据常常与历史数据分开销售。免费的 Massive 密钥可以下载历史数据，但在实时登录时会被拒绝；Alpaca 的免费密钥可流式传输 IEX 和参考性的期权数据源，但不包括 SIP 或 OPRA；Interactive Brokers 提供你的账户所订阅的内容。当数据源无法运行时，图表会指明是哪种情况并停止，而不是在一个永远不会变绿的圆点背后不停重连。
- 这些供应商大多允许**每个账户一个实时连接**，所以同一密钥上的第二个程序会占掉这个席位。这种情况会如实报告并持续重试，因为关闭另一个程序后就会恢复。

**Alpaca** 为此有一项设置：*Market data feed*，`iex`（免费计划，默认）或 `sip`（付费）。它只选择实时 socket；下载不受影响。

### 加密货币衍生品服务商

`BTCUSDT` 既是现货交易对，**也是**永续合约，两者是不同的序列：永续合约相对现货存在基差，而有到期日的合约会向现货收敛。因此数据集持有的是哪个市场，绝不会从代码推断。

- **Binance USDⓈ-M Futures** 是与 Binance 并列的独立服务商，而不是它上面的一项设置。合约的写法与期货市场一致：永续合约为 `BTCUSDT`，有到期日的为 `ETHUSDT_250926`。Coin-M（反向）合约不提供。
- **Bitget** 带有一项 *Market* 设置，`spot`（默认）或 `usdt-futures`，因为它在两个盘口上用完全相同的方式书写同一个代码。
- **OKX** 无需设置：它自己的品种 ID 已经说明代码属于哪个市场，现货为 `BTC-USDT`，永续为 `BTC-USDT-SWAP`，有到期日的合约为 `BTC-USD-241227`。

### OANDA

这里唯一需要密钥的外汇服务商，而它的密钥就是账户的密钥：OANDA 不发放仅用于行情数据的 token，所以 connector 要求输入与 [broker 账户](/zh/config/brokers)相同的个人访问 token，外加用于读取价格的账户号码。

- **设置**：*Account ID*（`001-004-1234567-001`）和 *Environment*（`live` 或 `practice`，它们是不同的主机，使用不同的 token）。
- **品种**写作 `base_quote`，指数和商品也一样：`EUR_USD`、`XAU_USD`、`SPX500_USD`。*测试连接*会报告该账户被允许报价的品种数量。
- **日线 K 线固定在 UTC 午夜。**OANDA 自己的默认值在纽约时间 17:00 换日，这是外汇交易时段，但不是这里其他所有数据集所用的日界线，所以 connector 请求 UTC 版本。
- 成交量是 **tick 数**，而不是成交规模：做市商公布的是它报了多少次价，而不是多少数量易手。

当一个服务商拥有多个 connector 时，图表的实时控件会多出一个账户选择器：两个密钥意味着两份权限和两个连接席位，所以消耗哪一个由你决定，而不是备用回退。

### TradeStation

它的行情数据范围依附于账户自己的 OAuth 密钥，所以 connector 要求输入与 [broker 账户](/zh/config/brokers)相同的 API 密钥对和 refresh token。没有单独的行情数据凭据。

- **设置**：*Environment*（`live` 或 `sim`）。
- **代码**是 TradeStation 自己的：股票为 `AAPL`，连续期货为 `@ES`，合约为 `ESH26`，现金指数为 `$SPX.X`，期权为 `MSFT 260116C400`。输入一个会查询并显示它是什么，这是发现拼写错误最快的方式。
- **K 线以收盘时间标记**，所以 connector 会减去周期长度并存储开盘时间，与这里其他所有序列一样。
- **只提供 1m、5m、15m 和 1d。**TradeStation 从*交易时段*开盘起构建日内 K 线，所以它的小时 K 线从 9:30 开始，无法与你库中其他任何地方的小时 K 线对齐。请下载 15m，并以任意日内时间框架读取；拒绝信息中会这样说明。

### FOREX.com (StoneX)

情况相同：没有自己的行情数据凭据，所以它使用与 [broker 账户](/zh/config/brokers)相同的用户名、密码和 AppKey 登录，两者共用一个会话。

- **市场是一个数字。**API 接受数字形式的市场 ID；你输入 `EUR/USD`，connector 会解析它。当名称匹配多个市场时，错误信息会列出这些市场及其 ID，你按 ID 下载。
- **没有成交量。**做市商公布的是价格而不是规模，所以成交量列为零，而不是一个看似合理的数字。
- **日线 K 线是该交易场所的交易时段**，在纽约收盘时换日，而不是在 UTC 午夜。这是 StoneX 实际交易的时段，并按此存储，因此这里的日线序列不会与按 UTC 日划分的服务商的序列重叠。
- `4h` 会被拒绝：StoneX 没有说明从哪里开始计算它。请下载 `1h` 并以 4h 读取。

### Capital.com

第三个密钥即账户密钥的服务商：Capital.com 不发放行情数据凭据，所以 connector 使用与 [broker 账户](/zh/config/brokers)相同的 API 密钥、登录名和自定义密码登录，两者共用一个会话。

- **设置**：*Environment*（`live` 或 `demo`）。
- **品种是一个 epic**，即 Capital.com 自己的市场名称：`EURUSD`、`US500`、`AAPL`、`BTCUSD`。品种搜索会返回它们。
- **K 线是做市商交易的买卖双方的中间价**，下载和实时图表都是如此。
- **日线 K 线是该交易场所的交易时段**，而不是 UTC 日，因此这里的日线序列不会与按 UTC 日划分的服务商的序列重叠。
- **实时**使用同一个会话，一次最多允许 40 个品种。Capital.com 把买价和卖价作为两根独立的 K 线流式传输，所以只有双方都跳动过，窗格才会填满。

### Interactive Brokers

特殊的一个：没有供应商 URL，也没有 API 密钥。你在自己的机器上运行 **IB Gateway** 或 **TWS**，connector 使用它的 socket 协议，所以它携带的是一个**地址**而不是凭据：主机和端口，以明文存储，以便在连接失败时诊断。数据取决于你的 IB 账户所订阅的内容。

- **设置**：*Gateway host*（同一台机器上的 gateway 用 `host.docker.internal`，因为 OpenTraderWorld 在容器中运行）和 *API port*（Gateway 为 4001 实盘 / 4002 模拟，TWS 为 7496 / 7497）。
- **在 gateway 中**：Global Configuration → API → Settings，勾选 *Enable ActiveX and Socket Clients*，并检查端口一致。在 Docker Desktop 上，调用来自主机回环，所以 *Allow connections from localhost only* 已经涵盖了它；在 Docker Engine 上，请取消勾选，并将 `172.28.53.10` 加入 *Trusted IPs*，它只接受单个地址，而不接受范围。
- **测试连接**会报告是什么应答了，并在没有任何应答时指出要更改的设置。
- **代码**：股票用 `AAPL`、`SAN:EUR` 或 `7203@TSEJ:JPY`，现金货币对用 `EURUSD`，期权用 OCC 代码。期货写成带月份的形式 `ES.202512`，或使用 TWS 显示的本地代码 `MNQU6`。期货在下载任何内容之前会先向 gateway 查询，所以交易所是可选的：当代码对应多个上市地时，错误信息会列出它们，由你选择。
- client id 由应用在一个很高的私有区间内选择，从不询问你，所以你连接到 gateway 的其他任何程序都不会被踢下线。
- Interactive Brokers 允许**每个账户**每滚动 10 分钟 60 次历史请求：connector 会自行控制节奏，所以长时间的回填速度较慢是设计使然。

已在 **IB Gateway build 10.50.1e（2026 年 8 月 25 日）**上测试。较旧的版本预计也能工作，因为 socket 协议会向下协商，但这个版本是该 connector 的验证版本。

## 创建一个

1. **添加 connector**，选择服务商并命名：这个名称就是模块选择器中显示的内容，所以 *Binance charts* 比 *Binance 2* 更好。
2. 填写服务商要求的凭据，或从保险库中选择。无需密钥的服务商跳过此步。
3. 选择它服务的**模块**。从某个模块中打开时，新的 connector 只授权给该模块；从设置或 `/connectors` 打开时，则授权给所有模块。

缺少必需凭据的 connector 会显示为*需要凭据*，在你设置之前，每个模块都会跳过它。

## 模块授权

授权列表在**服务端**：请求一个从未授予它的 connector 的模块会被拒绝，因此只存在于浏览器中的复选框只会是摆设。勾选所有数据模块会折叠回*所有模块*通配符，这样未来的数据模块也能被涵盖。

目前可授权的模块：

| 模块 | 它读取什么 |
|---|---|
| **Historical Data** | 下载表单的服务商列表，以及品种查询 |
| **Visualization** | 图表的品种搜索、按需窗口和实时流 |
| **Watchlists** | 清单或单个品种的报价源 |
| **Journal** | 行情数据和未平仓风险标签页背后的 K 线 |
| **Quant Tools** | 衍生品标签页：期货合约、期权链和隐含波动率，来自 Interactive Brokers 或 Massive |

## 请求限额 {#request-limits}

限额是按 **天**、**小时**或**分钟**统计的出站调用次数，按 connector 跟踪。

- 在应用的其他所有地方，它都**仅作观察**：它为 [设置 → API 速率](/zh/config/settings#api-rate)中的计数器提供数据并向你发出警告，但不会限流。
- 在图表的按需获取（`/api/histviz/series`）上，它会**阻止**：一旦 connector 达到限额，窗口就会带着已存储的 K 线和一条*已达到请求限额*的提示返回，而不是悄悄耗尽按量计费的套餐。

如果你更愿意让服务商来说“不”，就不要设置限额。

## connector 的使用位置

- **Historical Data**：下载表单的服务商列表，以及品种查询。
- **Visualization**：数据标签页会同时搜索授权给该图表的每个 connector；实时流运行在你于实时控件中选择的 connector 上，或该图表就该服务商获得授权的最早一个 connector 上。
- **Watchlists**：清单或单个品种的报价源。CoinGecko 和 Yahoo 在完全没有 connector 的情况下仍然可用。
- **Trading Journal**：行情数据和未平仓风险标签页背后的 K 线，每种资产类型可选择来源。
- **Quant Tools**：衍生品标签页列出某个产品的期货合约或某个标的的期权链，并为每个定价。只有 Interactive Brokers 和 Massive 会列出它们；隐含波动率历史仅来自 Interactive Brokers。

::: tip 从旧的按模块设置升级
Historical Data 的 *Settings* 标签页和 Watchlists 的 *Sources* 标签页已不再存在：它们曾是这个界面在两个互不相连的列表上的两份互不相连的副本。在其中任何一个里创建的账户，现在都是这里的 connector，各自仍授权给它原来所属的模块，因此升级时覆盖范围不变。名称再次全局唯一：同时存在于两个列表中的名称只保留一份，另一个重命名为 `<name> #2`。
:::
