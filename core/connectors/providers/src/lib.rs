//! One module per venue, each behind its own Cargo feature. A provider depends on the model
//! and the SDK only, never on another provider (checked by `tests::providers_are_isolated`).

#[cfg(feature = "alpaca")]
pub mod alpaca;
#[cfg(feature = "alphavantage")]
pub mod alphavantage;
#[cfg(feature = "binance")]
pub mod binance;
#[cfg(feature = "bitfinex")]
pub mod bitfinex;
#[cfg(feature = "bitget")]
pub mod bitget;
#[cfg(feature = "bybit")]
pub mod bybit;
#[cfg(feature = "capitalcom")]
pub mod capitalcom;
#[cfg(feature = "coinbase")]
pub mod coinbase;
#[cfg(feature = "deribit")]
pub mod deribit;
#[cfg(feature = "eodhd")]
pub mod eodhd;
#[cfg(feature = "forexcom")]
pub mod forexcom;
#[cfg(feature = "gateio")]
pub mod gateio;
#[cfg(feature = "ibkr")]
pub mod ibkr;
#[cfg(feature = "kraken")]
pub mod kraken;
#[cfg(feature = "kucoin")]
pub mod kucoin;
#[cfg(feature = "massive")]
pub mod massive;
#[cfg(feature = "mexc")]
pub mod mexc;
#[cfg(feature = "ninjatrader")]
pub mod ninjatrader;
#[cfg(feature = "oanda")]
pub mod oanda;
#[cfg(feature = "okx")]
pub mod okx;
#[cfg(feature = "tradestation")]
pub mod tradestation;
#[cfg(feature = "yahoo")]
pub mod yahoo;

#[cfg(test)]
mod isolation {
    /// No provider module imports another one.
    #[test]
    fn providers_are_isolated() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let Ok(entries) = std::fs::read_dir(&root) else { return };
        let providers: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        for p in &providers {
            let mut stack = vec![root.join(p)];
            while let Some(dir) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&dir) else { continue };
                for f in rd.filter_map(|e| e.ok()) {
                    let path = f.path();
                    if path.is_dir() {
                        stack.push(path);
                        continue;
                    }
                    if path.extension().and_then(|x| x.to_str()) != Some("rs") {
                        continue;
                    }
                    let src = std::fs::read_to_string(&path).unwrap_or_default();
                    for other in providers.iter().filter(|o| *o != p) {
                        assert!(
                            !src.contains(&format!("crate::{other}")),
                            "{} imports provider `{other}`",
                            path.display()
                        );
                    }
                }
            }
        }
    }
}
