//! Interactive Brokers through the framework's gateway adapter, served through the same
//! [`Connector`] trait as every other provider.
//!
//! The adapter owns the socket (one session per gateway, its pacing, its client id) and
//! answers canonical values; this bridge turns OTW's `(ticker, asset_type, timeframe, from,
//! to)` into an instrument id and a range, and the canonical candles back into OTW's bars,
//! through their text as every ported venue does.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use otw_connector_model::{AssetClass, TimeRange, Timeframe};
use otw_connector_providers::ibkr::gateway::{contract, IbkrGateway, Series};
use time::OffsetDateTime;

use otw_store::histdata::{Bar, QuoteBar};

use super::super::{Capability, Chunk, Connector, QuoteHistory, SymbolHit};
use super::{bar, err, f, market, to_ts};

pub struct NativeIbkr {
    gateway: IbkrGateway,
    cap: &'static Capability,
}

impl NativeIbkr {
    pub fn new(gateway: IbkrGateway, cap: &'static Capability) -> Self {
        NativeIbkr { gateway, cap }
    }

    /// The instrument id of an OTW ticker. An asset type IB is not asked for is refused, not
    /// read as a share.
    fn id(&self, ticker: &str, asset_type: &str) -> Result<otw_connector_model::InstrumentId> {
        if !contract::ASSET_TYPES.contains(&asset_type) {
            return Err(anyhow!("Interactive Brokers connector does not serve asset type {asset_type:?}"));
        }
        self.gateway.instrument_for(ticker, market(asset_type)).map_err(err)
    }

    async fn series(
        &self,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
        series: Series,
    ) -> Result<Vec<Bar>> {
        let id = self.id(ticker, asset_type)?;
        let tf: Timeframe = timeframe.parse().map_err(|e| anyhow!("{e}"))?;
        let range = TimeRange::new(to_ts(from)?, to_ts(to)?);
        let candles = self.gateway.history(secrets, &id, tf, range, series).await.map_err(err)?;
        candles.iter().map(bar).collect()
    }
}

/// OTW's asset type for what IB classed a search hit as.
fn kind(a: AssetClass) -> &'static str {
    match a {
        AssetClass::Spot => "crypto",
        AssetClass::Forex => "fx",
        AssetClass::Index => "index",
        AssetClass::Future => "future",
        AssetClass::Option => "option",
        _ => "equity",
    }
}

#[async_trait::async_trait]
impl Connector for NativeIbkr {
    fn capability(&self) -> &'static Capability {
        self.cap
    }

    fn validate_config(&self, config: &HashMap<String, String>) -> Result<()> {
        self.gateway.check_settings(config).map_err(err)
    }

    async fn test(&self, _client: &reqwest::Client, secrets: &HashMap<String, String>) -> Result<String> {
        self.gateway.test(secrets).await.map_err(err)
    }

    async fn fetch_chunk(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Chunk> {
        // Every bar IB answered, the forming one included: the worker keeps the closed ones.
        Ok(Chunk::of(self.series(secrets, ticker, asset_type, timeframe, from, to, Series::Native).await?))
    }

    fn quote_history(&self, _asset_type: &str) -> QuoteHistory {
        QuoteHistory::Candles
    }

    /// IB serves each side as its own bar series: `BID`, then `ASK`, joined per bar.
    async fn fetch_quote_bars(
        &self,
        _client: &reqwest::Client,
        secrets: &HashMap<String, String>,
        ticker: &str,
        asset_type: &str,
        timeframe: &str,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> Result<Vec<QuoteBar>> {
        let bid = self.series(secrets, ticker, asset_type, timeframe, from, to, Series::Bid).await?;
        let ask = self.series(secrets, ticker, asset_type, timeframe, from, to, Series::Ask).await?;
        Ok(super::super::join_sides(&bid, &ask))
    }

    async fn best_quote(&self, _client: &reqwest::Client, secrets: &HashMap<String, String>, ticker: &str, asset_type: &str) -> Result<(f64, f64)> {
        let id = self.id(ticker, asset_type)?;
        let q = self.gateway.quote(secrets, &id).await.map_err(err)?;
        Ok((f(q.bid.0)?, f(q.ask.0)?))
    }

    async fn search_symbols(&self, _client: &reqwest::Client, secrets: &HashMap<String, String>, query: &str, asset_type: &str, limit: usize) -> Result<Vec<SymbolHit>> {
        let specs = self.gateway.search(secrets, query, market(asset_type), limit).await.map_err(err)?;
        Ok(specs
            .into_iter()
            .map(|s| SymbolHit { symbol: s.id.native.clone(), name: s.symbol.0.clone(), asset_type: kind(s.asset_class).into(), exchange: s.exchange.clone().unwrap_or_default() })
            .collect())
    }
}
