//! An IBKR Flex statement, through the mapper as the driver feeds it: the reference answer,
//! then the statement, for fills, positions and the summary. Never panics.

#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use otw_connector_model::{AccountKind, Command, CorrId, InstrumentId, TimeRange, Timestamp};
use otw_connector_providers::ibkr::flex::{IbkrFlex, Session, VENUE};
use otw_connector_sdk::{InstrumentCache, RawFrame};
use otw_connector_testkit::{decode_once, encode_once};

fn frame(endpoint: &'static str, body: &[u8]) -> RawFrame {
    RawFrame::Http { corr_id: CorrId(1), endpoint_id: endpoint, status: 200, headers: vec![], body: Bytes::copy_from_slice(body) }
}

fuzz_target!(|data: &[u8]| {
    let Ok(m) = IbkrFlex::new() else { return };
    let cache = InstrumentCache::new();
    let now = Timestamp::from_secs(1_775_001_600).unwrap_or_default();
    let range = TimeRange::new(Timestamp::from_secs(1_772_323_200).unwrap_or_default(), now);
    for kind in [AccountKind::Fills, AccountKind::Positions, AccountKind::Summary] {
        for tz in ["0", "-300", "x"] {
            let cmd = Command::QueryAccount {
                req_id: CorrId(1),
                kind,
                symbols: vec![InstrumentId::new(VENUE, "AAPL")],
                range: Some(range),
                cursor: None,
                account: Some(format!("123|{tz}")),
            };
            let mut s = Session::default();
            if encode_once(&m, &mut s, &cache, &cmd, now).is_err() {
                continue;
            }
            // The fuzz input as the reference answer, then as the statement itself.
            let _ = decode_once(&m, &mut s, &cache, &frame("send", data), now);
            let mut s = Session::default();
            let _ = encode_once(&m, &mut s, &cache, &cmd, now);
            let ok = b"<FlexStatementResponse><Status>Success</Status><ReferenceCode>R</ReferenceCode></FlexStatementResponse>";
            let _ = decode_once(&m, &mut s, &cache, &frame("send", ok), now);
            let _ = decode_once(&m, &mut s, &cache, &frame("statement", data), now);
        }
    }
});
