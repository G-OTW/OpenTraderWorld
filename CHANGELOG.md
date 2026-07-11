# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.4] - 2026-07-11

### Added
- Backtest: **portfolio backtesting** — run one strategy across several datasets on a merged clock, with an alignment preview (overlap window, warm-up bars, per-asset missing bars) and a per-asset breakdown of trades, PnL, fees and exposure.
- Backtest: **expert mode** — a full-screen builder for named strategies and custom node-graph indicators (multi-step, each step referencing earlier ones), with duplicate and search.
- Backtest: **grid strategy** — a ladder of levels that buys low and sells one level up (long / short / neutral), sized per level or by splitting a total budget; reports fills, round trips and end inventory.
- Backtest: **out-of-sample split** — runs the strategy on an in-sample head and an out-of-sample tail and shows the two stat blocks side by side to expose overfitting.
- Backtest: advanced sizing — **risk-per-trade** (stop-based), **fractional Kelly** (windowed, capped), and **equity tiers**; plus **portfolio limits** (max open positions, max total and per-asset exposure).
- Backtest: cost & execution realism — **slippage** (ticks or % of price), **funding** estimate for perps, **circuit breakers** (max daily loss / max drawdown halt), and instrument profile (tick size, lot step, min quantity, contract multiplier).
- Backtest: per-trade **MAE/MFE** (max adverse / favorable excursion) and a filterable trades table.
- Backtest: downloadable **Markdown report** per saved run.
- Prompt Store module: a searchable library of reusable prompts shown as a grid of vignettes (name, tags, last-saved), with tag filtering, thumbs up/down rating and quick filters, duplicate, and per-prompt version history with rollback.

### Changed
- Backtest: the custom-indicator builder now chains **indicators onto any source** — each step applies an indicator to a price field or to an earlier step's output (e.g. `HullMA` of an `RSI`, `MACD` of an `RSI`), with named steps and free-form math **formula steps** (`@volume / SMA(@volume)`) referencing steps by name.
- Backtest: **stop-loss and take-profit can be toggled on/off** independently with a checkbox, rather than disabled by entering 0.
- Design enhancement.

### Fixed
- Doc submissions from the editor now work out of the box: the submission relay no longer requires `DOC_SUBMISSION_TOKEN`.
- Doc submissions: uploaded images (`/api/files/…`) are now inlined as `data:` URIs before relaying.

## [0.0.3] - 2026-07-07

### Changed
- Updates no longer reset the operator's network mode: `network.env` is no longer tracked by git, so `git reset --hard` during an update leaves it untouched.

### Fixed
- Chart: drag-panning no longer gets stuck after a few pixels.
- Settings → Update app now shows the correct update commands (`git reset --hard` + image pull); the previous `git pull` fails on force-pushed releases.

## [0.0.2] - 2026-07-07

### Added
- Prebuilt Docker images and image-based Compose install (multi-arch).
- Documentation site with in-app update check.

### Changed
- Default network bind to localhost.

### Fixed
- findb: treat empty `FINDB_ARCHIVE_URL` as unset.

## [0.0.1] - 2026-07-06

[Unreleased]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.4...HEAD
[0.0.4]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/G-OTW/OpenTraderWorld/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/G-OTW/OpenTraderWorld/releases/tag/v0.0.1
