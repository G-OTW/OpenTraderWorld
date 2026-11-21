//! A fake venue for the shared trading layer's own tests: a [`TradeMapper`] whose every
//! optional hook can be switched on ([`Knobs`]), speaking a small JSON dialect of its own
//! (`synthetic`, no venue's). It proves the hooks the trader and the broker offer without any
//! venue's code: margin per instrument, account-wide position mode, conditional ids, contract
//! sizes, private socket variants, cancel and replace, batch cancel, ack without an id, per-row
//! errors, several fee lines and fills without a fee, the quote-sized market buy; and the batch 2
//! hooks: leverage read from the venue, a book sized in the quote currency, minted client ids
//! with a time-anchored lookup and a cancel that names them, a lookup that walks pages, fills
//! from two stores by age, a name table, a trigger's successor order, a socket with nothing to
//! subscribe, no heartbeat and a keepalive call, binary and partial frames, a pushed mode;
//! and the doc review's: a frame naming its order by client id only, announced maintenance,
//! numbered socket frames, a list paged by a header.
//!
//! The dialect: every answer is `{"code": 0, ...}`; a non-zero `code` is an error from
//! [`ERRORS`]. Orders `{"rows": [{"id", "cid", "symbol", "side", "status", "qty", "filled"}]}`;
//! fills `{"rows": [{"exec", "id", "cid", "symbol", "qty", "price", "fee"?, "extra"?}]}`;
//! rules `{"rows": [{"symbol", "tick", "step", "contract"?, "quote_step"?, "unit"?, "min"?,
//! "aliases"?}]}`; an order row may name its successor (`"next"`) and its limit (`"price"`);
//! an order list's next page is `"next"` in the body, else the `x-next` header; positions
//! `{"rows": [{"symbol", "side", "dir", "qty", "margin"?}]}`. Socket frames may carry `"seq"`,
//! `"changed_cid"` (`{"cid", "symbol"}`) and `"maintenance"` (`"on"` or `"off"`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use bytes::Bytes;
use otw_connector_model::{
    Capabilities, Command, ConnectorError, CorrId, Decimal, Envelope, InstrumentId, Mapped, OrderAmend, OrderRequest, OrderStatus, OrderType, Side, Timestamp,
};
use otw_connector_model::SizeUnit;
use otw_connector_sdk::trade::{
    Ack, CancelAllWay, CancelRow, Canon, ClientIdRule, ClientRef, DecodeCtx, ErrorRow, FeeLine, LookupAt, MarginMode, OrderSpace, Page, Placing, PositionMode, PositionSide,
    PrivateEvent, PrivateSocket, Refusal, Settings, TradeMapper, VenueError, VenueFill, VenueOrder, VenuePosition, VenueRules,
};
use otw_connector_sdk::{HttpRequest, MapCtx, Mapper, Method, NativeRequest, RawFrame, SignableRequest, Signer};
use serde_json::{json, Value};

const CAPS: &str = r#"
provider = "fake"
label = "Fake"
venue = "FAKE"
api_version = "1"
maturity = "M2"
asset_classes = ["perp"]
instrument_discovery = "full"
auth = "none"
native_timeframes = ["1m"]

[orders]
types = ["market", "limit", "stop"]
tif = ["gtc", "ioc"]
reduce_only = true
amend = true
client_id = true
cancel_all_native = true
position_mode = "both"
level = "E2D"
"#;

/// The fake's error table: `(code, kind, words)`.
pub const ERRORS: &[ErrorRow] = &[
    ("1", Canon::Rejected(Refusal::Funds), "no funds"),
    ("2", Canon::Rejected(Refusal::NotFound), "no such order"),
    ("3", Canon::Rejected(Refusal::Final), "order already final"),
    ("4", Canon::InvalidRequest, "position mode mismatch"),
    ("5", Canon::InvalidRequest, "margin mode mismatch"),
];

/// Which optional hooks the fake answers.
#[derive(Debug, Clone, Default)]
pub struct Knobs {
    /// Sizes in contracts ([`TradeMapper::counts_contracts`]); the rules carry `contract`.
    pub contracts: bool,
    /// A per-symbol margin read (`GET /margin`).
    pub symbol_margin: bool,
    /// An account-wide position mode read (`GET /mode`).
    pub account_mode: bool,
    /// Changes go as cancel and replace (`POST /replace`).
    pub replaces: bool,
    /// Cancel-all as a batch by ids, at most this many per call (`POST /cancel-batch`).
    pub batch: Option<usize>,
    /// Cancel-all over the whole book in one call (`POST /cancel-all`, no symbol).
    pub book_cancel: bool,
    /// A market buy sized in the quote coin, valued at `GET /ticker`'s `ask`.
    pub quote_buy: bool,
    /// No price read of the venue's own: the ticket's reference values a quote-sized buy.
    pub no_ticker: bool,
    /// Stop orders live apart, in the conditional id space (`/cond/...`).
    pub conditional: bool,
    /// Private sockets: one per path, each at the `ws` base; the first one bootstrapped by
    /// `GET /bullet` when `bootstrap` is set.
    pub sockets: Vec<&'static str>,
    pub bootstrap: bool,
    /// The venue's own leverage read (`GET /leverage`, `{"leverage": "5" | null}`).
    pub leverage: bool,
    /// Client ids minted by the trader (the stamp, as digits); the lookup carries `since` (the
    /// order's creation time), the cancel its client id.
    pub minted: bool,
    /// Lookup pages read, following the cursor (`"next"` in the answer); 0: the default.
    pub lookup_pages: usize,
    /// Fills older than a day read from `/fills-old` beside `/fills`.
    pub two_stores: bool,
    /// Decoders read the name table: an order row's `symbol` may be an alias.
    pub names: bool,
    /// Sockets with nothing to subscribe, no heartbeat frame (`ping_ms` 1000), and a keepalive
    /// call (`POST /keep`) every second.
    pub bare_socket: bool,
}

/// The fake venue.
#[derive(Clone)]
pub struct FakeVenue {
    caps: Capabilities,
    pub knobs: Knobs,
}

impl FakeVenue {
    pub fn new(knobs: Knobs) -> Self {
        FakeVenue { caps: Capabilities::from_toml(CAPS).expect("fake caps"), knobs }
    }

    fn post(&self, endpoint: &'static str, path: &str, body: Value, corr: CorrId) -> HttpRequest {
        let mut h = HttpRequest::get(endpoint, "rest", path, corr).signed();
        h.method = Method::Post;
        h.body = Some(Bytes::from(body.to_string()));
        h
    }

    fn get(&self, endpoint: &'static str, path: &str, corr: CorrId) -> HttpRequest {
        HttpRequest::get(endpoint, "rest", path, corr).signed()
    }

    fn space_of(o: &OrderRequest, knobs: &Knobs) -> OrderSpace {
        if knobs.conditional && o.order_type == OrderType::Stop {
            OrderSpace::Conditional
        } else {
            OrderSpace::Plain
        }
    }

    fn qty_in(&self, ctx: &DecodeCtx<'_>, symbol: &str, v: &Value) -> Result<Decimal, ConnectorError> {
        let q = dec(v)?;
        if self.knobs.contracts {
            ctx.sizes.to_base("Fake", symbol, q)
        } else {
            Ok(q)
        }
    }

    fn order_row(&self, ctx: &DecodeCtx<'_>, r: &Value) -> Result<VenueOrder, ConnectorError> {
        let symbol = if self.knobs.names { ctx.native("Fake", &text(&r["symbol"]))?.to_string() } else { text(&r["symbol"]) };
        let space = if ctx.endpoint.starts_with("cond_") { OrderSpace::Conditional } else { OrderSpace::Plain };
        Ok(VenueOrder {
            venue_id: space.tag(&text(&r["id"])),
            client_id: Some(text(&r["cid"])).filter(|c| !c.is_empty()),
            side: if text(&r["side"]) == "sell" { Mapped::Known(Side::Sell) } else { Mapped::Known(Side::Buy) },
            order_type: "limit".into(),
            status: self.status_of(&text(&r["status"])),
            qty: self.qty_in(ctx, &symbol, &r["qty"])?,
            filled_qty: self.qty_in(ctx, &symbol, &r["filled"])?,
            limit: opt_dec(&r["price"]),
            stop: None,
            avg_price: None,
            reduce_only: false,
            position: PositionSide::Net,
            created_ms: None,
            trigger_pending: false,
            parent_client_id: None,
            exit: None,
            symbol,
            successor_id: r["next"].as_str().map(str::to_string),
        })
    }
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn dec(v: &Value) -> Result<Decimal, ConnectorError> {
    v.as_str().unwrap_or("0").parse().map_err(|_| ConnectorError::decode(format!("fake: {v} is not a decimal")))
}

fn opt_dec(v: &Value) -> Option<Decimal> {
    v.as_str().and_then(|s| s.parse().ok())
}

fn parse(body: &[u8]) -> Result<Value, ConnectorError> {
    serde_json::from_slice(body).map_err(|e| ConnectorError::decode(format!("fake: {e}")))
}

fn rows(body: &[u8]) -> Result<Vec<Value>, ConnectorError> {
    Ok(parse(body)?["rows"].as_array().cloned().unwrap_or_default())
}

fn margin_of(v: &Value) -> Option<MarginMode> {
    match v.as_str() {
        Some("cross") => Some(MarginMode::cross()),
        Some("isolated") => Some(MarginMode::isolated()),
        _ => None,
    }
}

impl Mapper for FakeVenue {
    type Session = ();

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn base_urls(&self) -> &'static [(&'static str, &'static str)] {
        &[("rest", "http://fake.invalid"), ("ws", "ws://fake.invalid")]
    }

    fn encode(&self, _cmd: &Command, _s: &mut (), _ctx: &MapCtx<'_>) -> Result<Vec<NativeRequest>, ConnectorError> {
        Err(ConnectorError::unsupported("fake: no market data"))
    }

    fn decode(&self, _frame: &RawFrame, _s: &mut (), _ctx: &MapCtx<'_>) -> Result<Vec<Envelope>, ConnectorError> {
        Ok(Vec::new())
    }

    fn instrument_id(&self, symbol: &str) -> Result<InstrumentId, ConnectorError> {
        Ok(InstrumentId::new("FAKE", &symbol.to_ascii_uppercase()))
    }
}

impl TradeMapper for FakeVenue {
    fn client_id_rule(&self) -> ClientIdRule {
        ClientIdRule::alnum_dash_underscore(36)
    }

    fn place(&self, _s: &Settings, o: &OrderRequest, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        let sym = o.instrument.native.as_str();
        let mut b = json!({ "symbol": sym, "side": if o.side == Side::Buy { "buy" } else { "sell" }, "cid": p.client_id, "reduce": o.opts.reduce_only });
        match (p.quote_amount, self.knobs.contracts) {
            (Some(q), _) => b["quote"] = json!(q.to_string()),
            (None, true) => b["contracts"] = json!(p.contracts("Fake", sym, o.qty.value())?.to_string()),
            (None, false) => b["qty"] = json!(o.qty.value().normalize().to_string()),
        }
        if let Some(m) = &p.margin {
            b["margin"] = json!(m.word());
        }
        if let Some(l) = p.leverage {
            b["lev"] = json!(l.normalize().to_string());
        }
        b["pos"] = json!(match p.position {
            PositionSide::Net => "net",
            PositionSide::Long => "long",
            PositionSide::Short => "short",
        });
        Ok(match Self::space_of(o, &self.knobs) {
            OrderSpace::Conditional => self.post("cond_place", "/cond/order", b, p.corr),
            OrderSpace::Plain => self.post("place", "/order", b, p.corr),
        })
    }

    fn cancel(&self, _s: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(match OrderSpace::of(venue_id) {
            (OrderSpace::Conditional, id) => self.post("cond_cancel", "/cond/cancel", json!({ "symbol": symbol, "id": id }), corr),
            (OrderSpace::Plain, id) => self.post("cancel", "/cancel", json!({ "symbol": symbol, "id": id }), corr),
        })
    }

    fn cancel_all(&self, _s: &Settings, symbols: &[String], corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        if self.knobs.book_cancel {
            assert!(symbols.is_empty(), "a book-wide cancel names no symbol");
            return Ok(vec![self.post("cancel_all", "/cancel-all", json!({}), corr)]);
        }
        Ok(symbols.iter().map(|s| self.post("cancel_all", "/cancel-all", json!({ "symbol": s }), corr)).collect())
    }

    fn cancel_all_way(&self) -> CancelAllWay {
        match (self.knobs.batch, self.knobs.book_cancel) {
            (Some(max), _) => CancelAllWay::ByIds { max },
            (None, true) => CancelAllWay::Book,
            (None, false) => CancelAllWay::BySymbol,
        }
    }

    fn cancel_ids(&self, _s: &Settings, orders: &[VenueOrder], corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let ids: Vec<&str> = orders.iter().map(|o| o.venue_id.as_str()).collect();
        Ok(self.post("cancel_batch", "/cancel-batch", json!({ "ids": ids }), corr))
    }

    fn decode_cancel_rows(&self, _ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<CancelRow>, ConnectorError> {
        rows(body)?
            .iter()
            .map(|r| {
                let code = text(&r["code"]);
                let error = (code != "0" && !code.is_empty()).then(|| decode_code(&code, "row refused"));
                Ok(CancelRow { venue_id: text(&r["id"]), error })
            })
            .collect()
    }

    fn amend_replaces(&self) -> bool {
        self.knobs.replaces
    }

    fn amend(&self, _s: &Settings, cur: &VenueOrder, change: &OrderAmend, p: &Placing) -> Result<HttpRequest, ConnectorError> {
        let px = change.limit.map(|p| p.value().to_string());
        Ok(self.post("amend", "/amend", json!({ "id": cur.venue_id, "price": px }), p.corr))
    }

    fn amend_replace(&self, _s: &Settings, cur: &VenueOrder, change: &OrderAmend, new_client_id: &str, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let px = change.limit.map(|p| p.value().to_string());
        Ok(self.post("replace", "/replace", json!({ "id": cur.venue_id, "new_cid": new_client_id, "price": px }), corr))
    }

    fn query_by_client_id(&self, _s: &Settings, symbol: &str, client_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let mut out = vec![self.get("order", "/order", corr).query("symbol", symbol).query("cid", client_id)];
        if self.knobs.conditional {
            out.push(self.get("cond_order", "/cond/order", corr).query("symbol", symbol).query("cid", client_id));
        }
        Ok(out)
    }

    fn query_by_venue_id(&self, _s: &Settings, symbol: &str, venue_id: &str, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        Ok(vec![match OrderSpace::of(venue_id) {
            (OrderSpace::Conditional, id) => self.get("cond_order", "/cond/order", corr).query("symbol", symbol).query("id", id),
            (OrderSpace::Plain, id) => self.get("order", "/order", corr).query("symbol", symbol).query("id", id),
        }])
    }

    fn open_orders(&self, _s: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let with = |h: HttpRequest| match symbol {
            Some(s) => h.query("symbol", s),
            None => h,
        };
        let mut out = vec![with(self.get("open", "/open", corr))];
        if self.knobs.conditional {
            out.push(with(self.get("cond_open", "/cond/open", corr)));
        }
        Ok(out)
    }

    fn fills_since(&self, _s: &Settings, symbol: &str, from_ms: i64, to_ms: i64, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        Ok(self.get("fills", "/fills", corr).query("symbol", symbol).query("from", from_ms).query("to", to_ms))
    }

    fn instrument_rules(&self, _s: &Settings, symbol: Option<&str>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        let h = HttpRequest::get("rules", "rest", "/rules", corr);
        Ok(match symbol {
            Some(s) => h.query("symbol", s),
            None => h,
        })
    }

    fn positions(&self, _s: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(Some(self.get("positions", "/positions", corr).query("symbol", symbol)))
    }

    fn invalidates_position_mode(&self, e: &VenueError) -> bool {
        e.code.as_deref() == Some("4")
    }

    fn invalidates_margin_mode(&self, e: &VenueError) -> bool {
        e.code.as_deref() == Some("5")
    }

    fn symbol_margin(&self, _s: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(self.knobs.symbol_margin.then(|| self.get("margin", "/margin", corr).query("symbol", symbol)))
    }

    fn decode_symbol_margin(&self, _symbol: &str, body: &[u8]) -> Result<Option<MarginMode>, ConnectorError> {
        let v = parse(body)?;
        let mut m = margin_of(&v["margin"]);
        if let (Some(mm), Some(coin)) = (m.as_mut(), v["coin"].as_str()) {
            mm.coin = Some(coin.to_string());
        }
        Ok(m)
    }

    fn account_position_mode(&self, _s: &Settings, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(self.knobs.account_mode.then(|| self.get("mode", "/mode", corr)))
    }

    fn decode_account_position_mode(&self, body: &[u8]) -> Result<Option<PositionMode>, ConnectorError> {
        Ok(match parse(body)?["mode"].as_str() {
            Some("one_way") => Some(PositionMode::OneWay),
            Some("hedge") => Some(PositionMode::Hedge),
            _ => None,
        })
    }

    fn counts_contracts(&self) -> bool {
        self.knobs.contracts
    }

    fn market_buy_in_quote(&self, o: &OrderRequest) -> bool {
        self.knobs.quote_buy && o.order_type == OrderType::Market && o.side == Side::Buy
    }

    fn reference_price(&self, _s: &Settings, symbol: &str, _side: Side, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok((!self.knobs.no_ticker).then(|| HttpRequest::get("ticker", "rest", "/ticker", corr).query("symbol", symbol)))
    }

    fn decode_reference_price(&self, body: &[u8], _side: Side) -> Result<Decimal, ConnectorError> {
        dec(&parse(body)?["ask"])
    }

    fn private_sockets(&self, _s: &Settings) -> Vec<PrivateSocket> {
        if self.knobs.bare_socket {
            return self.knobs.sockets.iter().map(|path| PrivateSocket::new("ws", *path, Vec::new(), Vec::new(), 1_000)).collect();
        }
        self.knobs
            .sockets
            .iter()
            .map(|path| PrivateSocket::new("ws", *path, json!({ "op": "subscribe", "args": ["orders"] }).to_string().into_bytes(), b"ping".to_vec(), 20_000))
            .map(|mut s| {
                s.more.push(json!({ "op": "subscribe", "args": ["fills"] }).to_string().into_bytes());
                s
            })
            .collect()
    }

    fn private_socket_bootstrap(&self, _s: &Settings, socket: &PrivateSocket, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        let first = self.knobs.sockets.first().is_some_and(|p| *p == socket.path);
        Ok((self.knobs.bootstrap && first).then(|| self.post("bullet", "/bullet", json!({}), corr)))
    }

    fn decode_private_socket_bootstrap(&self, mut socket: PrivateSocket, body: &[u8]) -> Result<PrivateSocket, ConnectorError> {
        let v = parse(body)?;
        socket.url = Some(format!("{}?token={}", text(&v["endpoint"]), text(&v["token"])));
        socket.ping_every_ms = v["ping_ms"].as_u64().unwrap_or(socket.ping_every_ms);
        Ok(socket)
    }

    fn decode_private_in(&self, ctx: &DecodeCtx<'_>, payload: &[u8]) -> Result<Vec<PrivateEvent>, ConnectorError> {
        let v = parse(payload)?;
        if let Some(op) = v["op"].as_str() {
            return Ok(vec![PrivateEvent::Answer { op: op.to_string(), ok: v["ok"].as_bool().unwrap_or(false), message: text(&v["msg"]) }]);
        }
        let mut out = Vec::new();
        if let Some(c) = v.get("changed") {
            out.push(PrivateEvent::OrderChanged { symbol: c["symbol"].as_str().map(str::to_string), venue_id: text(&c["id"]) });
        }
        if let Some(c) = v.get("changed_cid") {
            out.push(PrivateEvent::OrderChangedByClient { symbol: c["symbol"].as_str().map(str::to_string), client_id: text(&c["cid"]) });
        }
        if let Some(m) = v["maintenance"].as_str() {
            out.push(PrivateEvent::Maintenance { on: m == "on", message: format!("fake maintenance {m}") });
        }
        if let Some(m) = v.get("mode_push") {
            let mode = match m.as_str() {
                Some("one_way") => Some(PositionMode::OneWay),
                Some("hedge") => Some(PositionMode::Hedge),
                _ => None,
            };
            out.push(PrivateEvent::PositionMode { symbol: v["symbol"].as_str().map(str::to_string), mode });
        }
        if v.get("order").is_some() {
            out.push(PrivateEvent::Order(self.order_row(ctx, &v["order"])?));
        }
        if v.get("fill").is_some() {
            out.extend(self.fill_rows(ctx, std::slice::from_ref(&v["fill"]))?.into_iter().map(PrivateEvent::Fill));
        }
        Ok(out)
    }

    fn decode_error(&self, frame: &RawFrame) -> Option<VenueError> {
        let RawFrame::Http { status, body, .. } = frame else { return Some(VenueError::new(Canon::Unknown, None, "fake: no answer")) };
        if *status >= 500 {
            return Some(VenueError::new(Canon::Unknown, Some(status.to_string()), "fake: gateway"));
        }
        let v: Value = serde_json::from_slice(body).ok()?;
        let code = v["code"].as_i64().unwrap_or(0);
        (code != 0).then(|| decode_code(&code.to_string(), &text(&v["msg"])))
    }

    fn decode_ack(&self, _body: &[u8]) -> Result<Ack, ConnectorError> {
        Err(ConnectorError::decode("fake: acks are read with their call"))
    }

    fn decode_ack_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Ack, ConnectorError> {
        let v = parse(body)?;
        let space = if ctx.endpoint.starts_with("cond_") { OrderSpace::Conditional } else { OrderSpace::Plain };
        let cid = Some(text(&v["cid"])).filter(|c| !c.is_empty());
        Ok(match v["id"].as_str().filter(|i| !i.is_empty()) {
            Some(id) => Ack { venue_id: space.tag(id), client_id: cid },
            None => Ack::without_id(cid),
        })
    }

    fn decode_orders(&self, _body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        Err(ConnectorError::decode("fake: orders are read with their call"))
    }

    fn decode_orders_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueOrder>, ConnectorError> {
        let next = parse(body)?["next"].as_str().map(str::to_string).or_else(|| ctx.header("x-next").map(str::to_string));
        Ok((rows(body)?.iter().map(|r| self.order_row(ctx, r)).collect::<Result<_, _>>()?, next))
    }

    fn decode_fills(&self, _body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        Err(ConnectorError::decode("fake: fills are read with their call"))
    }

    fn decode_fills_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Page<VenueFill>, ConnectorError> {
        Ok((self.fill_rows(ctx, &rows(body)?)?, None))
    }

    fn decode_positions_in(&self, ctx: &DecodeCtx<'_>, body: &[u8]) -> Result<Vec<VenuePosition>, ConnectorError> {
        rows(body)?
            .iter()
            .map(|r| {
                let symbol = text(&r["symbol"]);
                Ok(VenuePosition {
                    side: match text(&r["side"]).as_str() {
                        "long" => PositionSide::Long,
                        "short" => PositionSide::Short,
                        _ => PositionSide::Net,
                    },
                    direction: match text(&r["dir"]).as_str() {
                        "buy" => Some(Side::Buy),
                        "sell" => Some(Side::Sell),
                        _ => None,
                    },
                    qty: self.qty_in(ctx, &symbol, &r["qty"])?,
                    leverage: opt_dec(&r["lev"]),
                    margin: margin_of(&r["margin"]),
                    symbol,
                })
            })
            .collect()
    }

    fn decode_rules(&self, body: &[u8]) -> Result<Page<VenueRules>, ConnectorError> {
        let out = rows(body)?
            .iter()
            .map(|r| {
                let mut v = VenueRules::new(InstrumentId::new("FAKE", &text(&r["symbol"])));
                v.tick = opt_dec(&r["tick"]);
                v.lot_step = opt_dec(&r["step"]);
                v.contract_size = opt_dec(&r["contract"]);
                v.quote_step = opt_dec(&r["quote_step"]);
                v.min_qty = opt_dec(&r["min"]);
                if text(&r["unit"]) == "quote" {
                    v.size_unit = SizeUnit::Quote;
                }
                v.aliases = r["aliases"].as_array().map(|a| a.iter().map(text).collect()).unwrap_or_default();
                v
            })
            .collect();
        Ok((out, None))
    }

    fn status_of(&self, word: &str) -> Mapped<OrderStatus> {
        Mapped::Known(match word {
            "new" => OrderStatus::New,
            "partial" => OrderStatus::PartiallyFilled,
            "filled" => OrderStatus::Filled,
            "cancelled" => OrderStatus::Cancelled,
            other => return Mapped::Unknown(other.to_string()),
        })
    }

    fn symbol_leverage(&self, _s: &Settings, symbol: &str, corr: CorrId) -> Result<Option<HttpRequest>, ConnectorError> {
        Ok(self.knobs.leverage.then(|| self.get("leverage", "/leverage", corr).query("symbol", symbol)))
    }

    fn decode_symbol_leverage(&self, _symbol: &str, _side: PositionSide, body: &[u8]) -> Result<Option<Decimal>, ConnectorError> {
        Ok(opt_dec(&parse(body)?["leverage"]))
    }

    fn mints_client_ids(&self) -> bool {
        self.knobs.minted
    }

    fn mint_client_id(&self, stamp_ms: i64) -> Result<String, ConnectorError> {
        Ok(stamp_ms.to_string())
    }

    fn query_by_client_id_at(&self, s: &Settings, symbol: &str, client_id: &str, at: &LookupAt, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let mut out = self.query_by_client_id(s, symbol, client_id, corr)?;
        if let Some(since) = at.created_ms.filter(|_| self.knobs.minted) {
            out = out.into_iter().map(|h| h.query("since", since)).collect();
        }
        Ok(out)
    }

    fn lookup_pages(&self) -> usize {
        self.knobs.lookup_pages.max(1)
    }

    fn cancel_order(&self, s: &Settings, symbol: &str, venue_id: &str, client: Option<&ClientRef>, corr: CorrId) -> Result<HttpRequest, ConnectorError> {
        match client.filter(|_| self.knobs.minted) {
            Some(c) => Ok(self.post("cancel", "/cancel", json!({ "symbol": symbol, "id": OrderSpace::of(venue_id).1, "cid": c.id, "cid_at": c.at_ms }), corr)),
            None => self.cancel(s, symbol, venue_id, corr),
        }
    }

    fn fills_requests(&self, s: &Settings, symbol: &str, from_ms: i64, to_ms: i64, now_ms: i64, corr: CorrId) -> Result<Vec<HttpRequest>, ConnectorError> {
        let mut out = vec![self.fills_since(s, symbol, from_ms, to_ms, corr)?];
        if self.knobs.two_stores && from_ms < now_ms - 86_400_000 {
            out.push(self.get("fills_old", "/fills-old", corr).query("symbol", symbol).query("from", from_ms).query("to", to_ms));
        }
        Ok(out)
    }

    fn uses_symbol_names(&self) -> bool {
        self.knobs.names
    }

    fn private_sequence(&self, payload: &[u8]) -> Option<u64> {
        parse(payload).ok()?["seq"].as_u64()
    }

    fn private_socket_keepalive(&self, _s: &Settings, _socket: &PrivateSocket, corr: CorrId) -> Result<Option<(HttpRequest, u64)>, ConnectorError> {
        Ok(self.knobs.bare_socket.then(|| (self.post("keep", "/keep", json!({}), corr), 1_000)))
    }

    fn error_table(&self) -> &'static [ErrorRow] {
        ERRORS
    }
}

impl FakeVenue {
    fn fill_rows(&self, ctx: &DecodeCtx<'_>, rows: &[Value]) -> Result<Vec<VenueFill>, ConnectorError> {
        rows.iter()
            .map(|r| {
                let symbol = text(&r["symbol"]);
                let fee = opt_dec(&r["fee"]);
                let extra_fees = r["extra"]
                    .as_array()
                    .map(|a| a.iter().map(|x| Ok(FeeLine { fee: dec(&x["fee"])?, currency: text(&x["coin"]), kind: text(&x["kind"]) })).collect::<Result<Vec<_>, ConnectorError>>())
                    .transpose()?
                    .unwrap_or_default();
                Ok(VenueFill {
                    exec_id: text(&r["exec"]),
                    venue_order_id: text(&r["id"]),
                    client_id: Some(text(&r["cid"])).filter(|c| !c.is_empty()),
                    side: Mapped::Known(Side::Buy),
                    qty: self.qty_in(ctx, &symbol, &r["qty"])?,
                    price: dec(&r["price"])?,
                    fee: fee.unwrap_or_default(),
                    fee_currency: text(&r["fee_coin"]),
                    at_ms: r["at"].as_i64().unwrap_or(0),
                    position_only: false,
                    extra_fees,
                    fee_pending: fee.is_none(),
                    symbol,
                })
            })
            .collect()
    }
}

fn decode_code(code: &str, msg: &str) -> VenueError {
    match ERRORS.iter().find(|(c, _, _)| *c == code) {
        Some((_, canon, words)) => VenueError::new(*canon, Some(code.to_string()), format!("fake: {words} ({msg})")),
        None => VenueError::unmapped(Some(code.to_string()), format!("fake: code {code} ({msg})")),
    }
}

/// A signer whose key's signed calls go out one at a time ([`Signer::serial_key`]), named.
pub struct SerialSigner(pub &'static str);

impl Signer for SerialSigner {
    fn sign(&self, _req: &mut SignableRequest, _now: Timestamp) -> Result<(), ConnectorError> {
        Ok(())
    }

    fn serial_key(&self) -> Option<String> {
        Some(self.0.to_string())
    }
}

/// A signer that signs nothing over REST, logs no socket in, and signs each subscribe frame by
/// adding `"sig": "fake"` (the per-frame auth some venues ask).
pub struct FakeSigner;

impl Signer for FakeSigner {
    fn sign(&self, _req: &mut SignableRequest, _now: Timestamp) -> Result<(), ConnectorError> {
        Ok(())
    }

    fn sign_frame(&self, frame: Bytes, _now: Timestamp) -> Result<Bytes, ConnectorError> {
        let mut v: Value = serde_json::from_slice(&frame).map_err(|e| ConnectorError::invalid(format!("fake: {e}")))?;
        v["sig"] = json!("fake");
        Ok(Bytes::from(v.to_string()))
    }
}
