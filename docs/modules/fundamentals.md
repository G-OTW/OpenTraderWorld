# Fundamentals

One place to read the economy and a company from the sources that publish the numbers: macro series with charts, company statements, SEC filings, earnings-call transcripts, ETF holdings, a market calendar and alternative data. Everything is stored in your own database, so charts, the [Agent](/modules/agent) and other modules read it without asking the provider again.

Fundamentals has no provider settings of its own. Every source is a **[data connector](/config/connectors)** granted to the module: the plug icon in the page header opens the shared connector screen. Many sources are **keyless** public bodies (SEC EDGAR, the US Treasury, the ECB, Eurostat, the BIS, the OECD, the IMF, the World Bank, the CFTC, FINRA, USAspending); the rest take a free or paid key you bring. Nothing is fetched until you add a connector and grant it to Fundamentals.

## Pages

| Page | What it shows |
|---|---|
| **Macro** | Your series by category (growth, inflation, labour, rates, money, surveys, housing, energy, fiscal, positioning), up to four on one chart, the Treasury yield curve and central-bank policy rates. |
| **Company** | Profile and key metrics, financial statements, estimates, earnings, segments, dividends and buybacks, ownership, peers, ESG and pay, filings and transcripts. |
| **ETF** | Profile, top holdings, sector and country exposure. |
| **Events** | Upcoming earnings, IPOs, corporate actions and central-bank decisions. |
| **Documents** | Every stored filing and transcript, with full-text search and the transcript reader. |
| **Alternative data** | Congress trades, lobbying spend, federal contracts and patents granted. |
| **Library** | Tabs for the series and companies you keep (filterable), the source priority, and which provider serves which data family. |

**Customize** (top right) sets the density, which sections show and in what order, page by page.

## Macro series {#macro}

**Add series** searches a provider's catalog or takes the provider's own code (`CPIAUCSL` on FRED, `HICP/M.U2.N.000000.4D0.ANR` on the ECB). A code is checked against the provider before anything is stored: an unknown code is an error naming it, never an empty series. **Add a starter set** adds a first selection of US and euro-area series in one click.

| Provider | Key | What it covers |
|---|---|---|
| FRED | free | Most US series (also mirrors BLS, BEA and Census) |
| US Treasury | none | Daily par yield curve, total public debt |
| ECB, Eurostat | none | Euro-area inflation, rates, money, GDP, unemployment |
| BIS | none | Central-bank policy rates, effective exchange rates |
| OECD, IMF, World Bank | none | Leading indicators, World Economic Outlook, annual country data |
| BLS, BEA, EIA, US Census | free | US detail when FRED lags or lacks a series |
| CFTC | none | Commitments of Traders, non-commercial net positioning |

A row is a **period**: an observation is stored against the start of the period it covers. The chart computes the transforms on read (level, year on year, period change, difference, index 100), so nothing derived is ever stored. Year on year compares each value with the one dated a year earlier; a missing period shows a gap rather than a comparison against the wrong month. Recession shading follows the NBER dates.

## Companies {#company}

Company, ETF and Alternative data share one **symbol picker**: your favourites first, then the 15 most recently opened. Its search covers every stored symbol, plus EDGAR matches for companies.

Type a ticker. It is resolved on SEC EDGAR's ticker list; a ticker EDGAR does not know is an error, never a best guess. Opening a company stores it and fetches, in the background:

- **Statements** from the XBRL company facts, annual and quarterly. Fourth quarters and year-to-date cash-flow lines are derived by difference; each line keeps the tag it was reported under.
- **Filings** (10-K, 10-Q, 8-K, proxies...) with a link to the source.
- **Insider trades** parsed from Form 4.

The other tabs read the aggregators you connect, best source first. A provider whose plan leaves a dataset out (a free FMP key and earnings history, say) hands over to the next one, and a connector granted to the module without its key is skipped. For the price, the longest history wins (free plans often stop at one or two years):

| Data | Providers |
|---|---|
| Estimates, price targets, rating actions | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Earnings (EPS estimate and actual, next date) | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Segments | Financial Modeling Prep |
| Dividends and splits | EODHD, Massive, Financial Modeling Prep, Alpha Vantage |
| 13F holders | Financial Modeling Prep |
| Short interest | FINRA, Massive |
| Peers | Financial Modeling Prep, Finnhub |
| ESG and executive pay | Financial Modeling Prep, Finnhub |
| Transcripts | Financial Modeling Prep, Alpha Vantage, Finnhub |
| Price and market ratios | any market-data connector with daily equity bars |

**Library → Source priority** lists every dataset with more than one source (transcripts included) in the order its providers are tried. Pick a rank next to a provider to move it there; **Default order** puts the app's order back. The price has no order: the longest history wins.

Each tab says which provider answered and when, or the error that names the fix (usually a connector to add or a plan that does not include the dataset). An answer is kept and reused until it goes stale (a few hours for the calendar, a day for estimates, a week for holders); **Refresh** asks again now.

Opening a page does not spend your quota on a provider that just refused: one that turned the dataset down (plan, symbol, rate limit) is left alone for a while, from a few minutes after a network error to a week after a plan refusal, and so is one whose quota, declared on its connector, is used up. The tab says so and when the next automatic try is; **Refresh** asks every provider at once.

**Follow** a company to have it refreshed daily and to be told about its new filings.

### Transcripts {#transcripts}

The Transcripts tab lists the calls a provider holds for the company; a transcript's text is fetched the first time you open it, split into speaker turns, prepared remarks apart from the Q&A, and searchable with the other documents.

## Alternative data {#alt}

The company is the one open in Company; a ticker typed here is opened first.

| Tab | Providers |
|---|---|
| Congress trades | Quiver Quant (the latest across all members, or one company's), Finnhub premium (per company) |
| Lobbying | LDA.gov, Quiver Quant |
| Government contracts | USAspending, Quiver Quant |
| Patents | USPTO Open Data Portal (free key), Quiver Quant |

The public sources know a company by its **registered name**, not its ticker. LDA.gov, USAspending and the USPTO are queried with the name EDGAR stores, and a record only counts when its name is the same once punctuation and the legal suffix are dropped (`Lockheed Martin Corp` matches `LOCKHEED MARTIN CORPORATION`, never `Lockheed Martin Aculight`). Each tab shows the name it matched. For contracts, the parent recipient is used, so subsidiaries filed under it count and a separately registered one (Amazon Web Services under Amazon) does not.

LDA.gov takes a free key (register on lda.gov). Its firewall turns networks outside the US away (an HTTP 403 naming it): reach it from a US connection, or use Quiver Quant. A report counts once: an amendment replaces the original, and lobbyist registrations, which carry no spend, are left out. A company that lobbies through a subsidiary under another name (JPMorgan Chase Holdings for JPMorgan Chase) only shows the reports filed under its own name.

## Provider coverage {#coverage}

Which provider can serve which data, as in **Library → Provider coverage**. A family served by several providers is tried in the order of **Library → Source priority**. The price and the market ratios come from any market-data connector with daily equity bars (Alpha Vantage, EODHD, Massive, Yahoo, IBKR...), not listed here.

| Provider | Key | Macro | COT | Statements | Filings | Insiders | Estimates | Earnings | Segments | Dividends | Holders | Short interest | Peers | ESG | ETF | Calendar | Transcripts | Alt data |
|---|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| SEC EDGAR | none |  |  | ✓ | ✓ | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |
| FRED (St. Louis Fed) | free | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Treasury | none | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| ECB Data Portal | none | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| Eurostat | none | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BIS | none | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| OECD | none | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| IMF | none | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| World Bank | none | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BLS | free | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| BEA | free | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| EIA | free | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| US Census | free | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| CFTC | none |  | ✓ |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |
| FINRA | none |  |  |  |  |  |  |  |  |  |  | ✓ |  |  |  |  |  |  |
| Financial Modeling Prep | free tier |  |  |  |  |  | ✓ | ✓ | ✓ | ✓ | ✓ |  | ✓ | ✓ | ✓ | ✓ | ✓ |  |
| Finnhub | free tier |  |  |  |  |  | ✓ | ✓ |  |  |  |  | ✓ | ✓ |  | ✓ | ✓ | ✓¹ |
| Alpha Vantage | free tier |  |  |  |  |  | ✓ | ✓ |  | ✓ |  |  |  |  | ✓ | ✓ | ✓ |  |
| EODHD | free tier |  |  |  |  |  |  |  |  | ✓ |  |  |  |  | ✓ | ✓ |  |  |
| Massive (Polygon.io) | free tier |  |  |  |  |  |  |  |  | ✓ |  | ✓ |  |  |  |  |  |  |
| USAspending | none |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| LDA.gov (lobbying) | free |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| USPTO Open Data Portal | free |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |
| Quiver Quant | paid |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  |  | ✓ |

¹ Finnhub serves congress trades on a premium plan only.

Indicative: plans change, and a free tier can leave a family out (Financial Modeling Prep free: no 13F holders, transcripts or ETF holdings; Finnhub free: no ESG or congress trades; Alpha Vantage free: 25 requests a day). Check the provider's own page before you pay.

## Automatic refresh and notifications {#refresh}

Stored series are refreshed once their frequency says a new value may be out: a daily series twice a day, a weekly or monthly one daily, a quarterly one every three days, an annual one weekly. Followed companies are refreshed from EDGAR once a day. A source with no connector granted is skipped.

A series with a new period and a followed company with a new filing (insider forms aside) raise a notification, pushed to the [channels](/config/settings#notifications) Fundamentals has been granted (the bell in the page header).

## Dashboard widgets {#dashboard}

The [Dashboard](/modules/dashboard) offers macro series and boards, company snapshots, statement history, companies, filings, upcoming earnings and valuation comparisons. Cards show currency, reporting basis and dates; their refresh reads stored data only. Upcoming earnings uses a company's stored earnings snapshot or, when it has no future date, the stored market calendar. Valuation uses manually selected stored companies or the stored peers from its Peers tab. Missing estimates and ratios remain unavailable rather than being inferred.

## Search and agents {#search}

The top-bar search, with content titles on, finds stored companies, series and document titles.

Agents reach what is stored, read only, through the [MCP gateway](/config/ai-agents) once a token is granted **Fundamentals**: series and observations, companies, statements, filings, transcripts, insider trades and every stored dataset. Looking a provider up, refreshing and adding series stay out: they spend your provider quota.
