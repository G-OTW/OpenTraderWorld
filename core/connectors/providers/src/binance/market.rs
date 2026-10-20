//! The two Binance markets this module serves. Same wire shapes, different hosts, paths,
//! page sizes and account endpoints: data, not two adapters.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Spot,
    /// USDⓈ-M futures: linear contracts sized in the base coin.
    UsdM,
}

#[derive(Debug)]
pub struct Market {
    pub kind: Kind,
    pub venue: &'static str,
    pub name: &'static str,
    pub bases: &'static [(&'static str, &'static str)],
    pub caps_toml: &'static str,
    /// Klines per page and what a full page weighs.
    pub page: u32,
    pub page_weight: u32,
    pub klines: &'static str,
    pub exchange_info: &'static str,
    pub book_ticker: &'static str,
    pub ticker_price: &'static str,
    pub time: &'static str,
    pub open_orders: &'static str,
}

pub const SPOT: Market = Market {
    kind: Kind::Spot,
    venue: "BINANCE",
    name: "binance",
    bases: &[("rest", "https://api.binance.com"), ("ws", "wss://stream.binance.com:9443")],
    caps_toml: include_str!("capabilities.toml"),
    page: 1000,
    page_weight: 2,
    klines: "/api/v3/klines",
    exchange_info: "/api/v3/exchangeInfo",
    book_ticker: "/api/v3/ticker/bookTicker",
    ticker_price: "/api/v3/ticker/price",
    time: "/api/v3/time",
    open_orders: "/api/v3/openOrders",
};

pub const USDM: Market = Market {
    kind: Kind::UsdM,
    venue: "BINANCE_USDM",
    name: "binance futures",
    bases: &[("rest", "https://fapi.binance.com"), ("ws", "wss://fstream.binance.com/market"), ("ws_public", "wss://fstream.binance.com/public")],
    caps_toml: include_str!("capabilities_usdm.toml"),
    page: 1500,
    page_weight: 10,
    klines: "/fapi/v1/klines",
    exchange_info: "/fapi/v1/exchangeInfo",
    book_ticker: "/fapi/v1/ticker/bookTicker",
    ticker_price: "/fapi/v1/ticker/price",
    time: "/fapi/v1/time",
    open_orders: "/fapi/v1/openOrders",
};
