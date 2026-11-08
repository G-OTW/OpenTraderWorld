//! The protobuf messages the spot socket's candle and book-ticker channels send, read by hand:
//! a varint and length-delimited field reader is all they need. Field numbers from MEXC's own
//! definitions (mexcdevelop/websocket-proto at commit
//! `0c9c4f35dd0fadc3a46a350e909a93379d81e811`: `PushDataV3ApiWrapper.proto`,
//! `PublicSpotKlineV3Api.proto`, `PublicAggreBookTickerV3Api.proto`).

/// `PushDataV3ApiWrapper`, the fields a candle needs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Wrapper<'a> {
    /// 1: the channel subscribed (`spot@public.kline.v3.api.pb@BTCUSDT@Min1`).
    pub channel: &'a str,
    /// 5: when the message was made, ms.
    pub create_time: Option<i64>,
    /// 6: when it was sent, ms.
    pub send_time: Option<i64>,
    /// 308: `publicSpotKline`.
    pub kline: Option<&'a [u8]>,
    /// 315: `publicAggreBookTicker`.
    pub book_ticker: Option<&'a [u8]>,
}

/// `PublicAggreBookTickerV3Api`: the best bid and ask, as decimal strings.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct BookTicker<'a> {
    pub bid_price: &'a str,
    pub bid_qty: &'a str,
    pub ask_price: &'a str,
    pub ask_qty: &'a str,
}

/// `PublicSpotKlineV3Api`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Kline<'a> {
    pub interval: &'a str,
    /// Seconds.
    pub window_start: i64,
    pub open: &'a str,
    pub close: &'a str,
    pub high: &'a str,
    pub low: &'a str,
    /// Base amount.
    pub volume: &'a str,
    /// Quote amount.
    pub amount: &'a str,
}

enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Skipped,
}

struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    fn varint(&mut self) -> Option<u64> {
        let mut v: u64 = 0;
        for i in 0..10 {
            let (&b, rest) = self.buf.split_first()?;
            self.buf = rest;
            v |= u64::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if n > self.buf.len() {
            return None;
        }
        let (head, rest) = self.buf.split_at(n);
        self.buf = rest;
        Some(head)
    }

    /// The next field, or `None` at the end. `Err` on a malformed message.
    fn field(&mut self) -> Result<Option<(u64, Value<'a>)>, ()> {
        if self.buf.is_empty() {
            return Ok(None);
        }
        let key = self.varint().ok_or(())?;
        let value = match key & 7 {
            0 => Value::Varint(self.varint().ok_or(())?),
            1 => {
                self.take(8).ok_or(())?;
                Value::Skipped
            }
            2 => {
                let n = usize::try_from(self.varint().ok_or(())?).map_err(|_| ())?;
                Value::Bytes(self.take(n).ok_or(())?)
            }
            5 => {
                self.take(4).ok_or(())?;
                Value::Skipped
            }
            _ => return Err(()),
        };
        Ok(Some((key >> 3, value)))
    }
}

fn text(b: &[u8]) -> Result<&str, ()> {
    std::str::from_utf8(b).map_err(|_| ())
}

fn signed(v: u64) -> i64 {
    // proto3 `int64` is two's complement in a varint.
    v as i64
}

pub fn wrapper(buf: &[u8]) -> Result<Wrapper<'_>, ()> {
    let mut r = Reader { buf };
    let mut w = Wrapper::default();
    while let Some((n, v)) = r.field()? {
        match (n, v) {
            (1, Value::Bytes(b)) => w.channel = text(b)?,
            (5, Value::Varint(v)) => w.create_time = Some(signed(v)),
            (6, Value::Varint(v)) => w.send_time = Some(signed(v)),
            (308, Value::Bytes(b)) => w.kline = Some(b),
            (315, Value::Bytes(b)) => w.book_ticker = Some(b),
            _ => {}
        }
    }
    Ok(w)
}

pub fn book_ticker(buf: &[u8]) -> Result<BookTicker<'_>, ()> {
    let mut r = Reader { buf };
    let mut t = BookTicker::default();
    while let Some((n, v)) = r.field()? {
        match (n, v) {
            (1, Value::Bytes(b)) => t.bid_price = text(b)?,
            (2, Value::Bytes(b)) => t.bid_qty = text(b)?,
            (3, Value::Bytes(b)) => t.ask_price = text(b)?,
            (4, Value::Bytes(b)) => t.ask_qty = text(b)?,
            _ => {}
        }
    }
    Ok(t)
}

pub fn kline(buf: &[u8]) -> Result<Kline<'_>, ()> {
    let mut r = Reader { buf };
    let mut k = Kline::default();
    while let Some((n, v)) = r.field()? {
        match (n, v) {
            (1, Value::Bytes(b)) => k.interval = text(b)?,
            (2, Value::Varint(v)) => k.window_start = signed(v),
            (3, Value::Bytes(b)) => k.open = text(b)?,
            (4, Value::Bytes(b)) => k.close = text(b)?,
            (5, Value::Bytes(b)) => k.high = text(b)?,
            (6, Value::Bytes(b)) => k.low = text(b)?,
            (7, Value::Bytes(b)) => k.volume = text(b)?,
            (8, Value::Bytes(b)) => k.amount = text(b)?,
            _ => {}
        }
    }
    Ok(k)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_recorded_frame() {
        // Recorded from wss://wbs-api.mexc.com/ws on 2026-10-19.
        let raw = hex::decode("0a2873706f74407075626c69632e6b6c696e652e76332e6170692e70624042544355534454404d696e311a0742544355534454222032666239343231353465663434613461623265663938633861666236613461372890b5b89a9534a2134f0a044d696e3110f0acd7d6061a0738323734302e34220838323736322e30382a0838323736322e3039320838323734302e33313a0a302e3432313731383334420833343839382e373248acadd7d606").unwrap();
        let w = wrapper(&raw).unwrap();
        assert_eq!(w.channel, "spot@public.kline.v3.api.pb@BTCUSDT@Min1");
        assert_eq!(w.create_time, Some(1_792_398_989_968));
        let k = kline(w.kline.unwrap()).unwrap();
        assert_eq!(
            k,
            Kline { interval: "Min1", window_start: 1_792_398_960, open: "82740.4", close: "82762.08", high: "82762.09", low: "82740.31", volume: "0.42171834", amount: "34898.72" }
        );
        assert!(wrapper(&raw[..raw.len() - 3]).is_err(), "a cut frame is refused");
    }

    #[test]
    fn reads_a_recorded_book_ticker() {
        // Recorded from wss://wbs-api.mexc.com/ws on 2026-10-19.
        let raw = hex::decode("0a3473706f74407075626c69632e61676772652e626f6f6b5469636b65722e76332e6170692e7062403130306d7340425443555344541a074254435553445430f8e2d49f9534da13400a0838323236312e3433120a382e33373338353037361a0838323236312e3434220a302e30303032303135312a0b383430343234373133323630efe2d49f9534").unwrap();
        let w = wrapper(&raw).unwrap();
        assert_eq!(w.channel, "spot@public.aggre.bookTicker.v3.api.pb@100ms@BTCUSDT");
        assert!(w.kline.is_none());
        let t = book_ticker(w.book_ticker.unwrap()).unwrap();
        assert_eq!(t, BookTicker { bid_price: "82261.43", bid_qty: "8.37385076", ask_price: "82261.44", ask_qty: "0.00020151" });
    }
}
