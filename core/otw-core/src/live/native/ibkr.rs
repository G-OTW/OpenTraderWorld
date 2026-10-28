//! Interactive Brokers live, through the framework's gateway adapter.
//!
//! Not a WebSocket, so not [`super::super::run_feed`]: the adapter opens one subscription per
//! instrument on the shared gateway session and hands back canonical updates; this bridge
//! turns them into the hub's [`FeedUpdate`]s. The hub folds every timeframe from the prints
//! (or the five-second bars where IB serves no ticks), so [`StreamConnector::grain`] follows
//! [`contract::live_source`].
//!
//! **One market-data line per symbol, not per pane**: IB counts lines per account, so two
//! timeframes of one instrument ride one subscription. A refusal belongs to its instrument
//! alone and travels as a tagged fault; a socket that died ends the session, because every
//! instrument rides it.
//!
//! **The link is watched, not assumed.** A socket the Gateway stopped serving, or IB losing
//! its own servers (notice 1100), leaves every subscription open and silent, so the pane would
//! stay green on a dead feed. The connection is probed every [`PROBE_EVERY`], IB's
//! connectivity notices end the session while the link is down (and [`StreamConnector::open`]
//! refuses until IB restores it), and a line that ended on a passing fault is placed again
//! after [`RESUBSCRIBE_AFTER`] instead of leaving its pane on "retrying" forever.
//!
//! The best bid and ask is a line of its own per instrument (`reqMktData`, see
//! [`IbkrGateway::stream_quotes`]), keyed `<symbol>|quote` so it starts and stops apart from
//! the instrument's prints. Indices have no bid or ask.

use std::collections::HashMap;

use std::time::Duration;

use anyhow::Result;
use otw_connector_model::{ConnectorError, ErrorKind, Event};
use otw_connector_providers::ibkr::gateway::{contract, contract::LiveSource, mapper, session::Link, IbkrGateway};
use time::OffsetDateTime;
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio_stream::wrappers::ReceiverStream;

use super::super::{FeedCmd, FeedHandle, FeedUpdate, Instrument, LiveEvent, LiveQuote, LiveSession, SessionRequest, SourceGrain, StreamConnector};
use super::{f, live_err};

/// Market-data lines a plain IB account holds at once. IB never states the number over the
/// API, it only starts refusing requests once it is reached (error 322), so this is the
/// documented default: the account may have more (commissions and quote boosters raise it),
/// which is why crossing it is a warning and never a refusal on our side.
pub const LINE_CAP: usize = 100;

/// IB's only streaming bar size, for the instruments that have no tick-by-tick stream.
const SOURCE_SECS: i64 = 5;
/// Five-second bars arrive on the beat, so the close timer only covers a quiet market.
const GRACE: i64 = 3;
/// How often the gateway connection is asked whether it still answers.
const PROBE_EVERY: Duration = Duration::from_secs(30);
/// How long a line that ended on a passing fault waits before it is placed again: above IB's
/// 15 s floor between two tick requests for one instrument.
const RESUBSCRIBE_AFTER: Duration = Duration::from_secs(20);

pub struct NativeIbkrLive {
    gateway: IbkrGateway,
}

impl NativeIbkrLive {
    pub fn new(gateway: IbkrGateway) -> Self {
        NativeIbkrLive { gateway }
    }
}

#[async_trait::async_trait]
impl StreamConnector for NativeIbkrLive {
    /// One gateway per connector: every instrument shares the process-wide IB connection.
    fn socket_class(&self, _inst: &Instrument) -> Result<String> {
        Ok(String::new())
    }

    fn grain(&self, inst: &Instrument) -> Result<SourceGrain> {
        Ok(match contract::live_source(&inst.asset_type) {
            LiveSource::Bars5s => SourceGrain::Bars { secs: SOURCE_SECS, grace: GRACE },
            LiveSource::LastTick | LiveSource::MidPointTick => SourceGrain::Trades,
        })
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        Ok(line_key(inst))
    }

    fn quote_source(&self, inst: &Instrument) -> Option<&'static str> {
        (inst.asset_type != "index").then_some("native")
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        // The gateway address rides in the same map a credential would; a missing one fails
        // the session rather than each instrument.
        self.gateway.check_settings(req.secrets).map_err(live_err)?;
        // Subscriptions placed while IB has lost its servers are accepted and silent: the
        // session stays down, and the hub keeps retrying, until IB says it is back.
        let link = self.gateway.link(req.secrets).await.map_err(live_err)?;
        if let Some(why) = link.borrow().lost.clone() {
            return Err(live_err(ConnectorError::new(ErrorKind::Transient, why)));
        }
        let (out_tx, out_rx) = mpsc::channel::<Result<FeedUpdate>>(512);
        let (cmd_tx, cmd_rx) = mpsc::channel::<FeedCmd>(32);
        tokio::spawn(manage(self.gateway.clone(), req.secrets.clone(), req.connector.to_string(), req.instruments.to_vec(), link, cmd_rx, out_tx));
        Ok(LiveSession { handle: FeedHandle::new(cmd_tx), stream: Box::pin(ReceiverStream::new(out_rx)) })
    }
}

/// One line per instrument, and one more for its bid and ask.
fn line_key(inst: &Instrument) -> String {
    if inst.is_quote() {
        format!("{}|quote", inst.symbol)
    } else {
        inst.symbol.clone()
    }
}

/// Own the account's live subscriptions: one task per instrument, started and stopped as
/// panes come and go, all reporting onto one stream. Ends the session (an `Err` on the stream)
/// when the connection stops answering or IB loses its link; the hub reconnects.
async fn manage(
    gateway: IbkrGateway,
    secrets: HashMap<String, String>,
    connector: String,
    initial: Vec<Instrument>,
    mut link: watch::Receiver<Link>,
    mut cmds: mpsc::Receiver<FeedCmd>,
    out: mpsc::Sender<Result<FeedUpdate>>,
) {
    let mut wanted: HashMap<String, Instrument> = HashMap::new();
    let mut lines: HashMap<String, JoinHandle<()>> = HashMap::new();
    // Lines that ended on a passing fault report here, and wait out RESUBSCRIBE_AFTER in
    // `retries` before they are placed again.
    let (ended_tx, mut ended_rx) = mpsc::unbounded_channel::<String>();
    let mut retries: JoinSet<String> = JoinSet::new();
    let placed = link.borrow_and_update().resubscribe;
    let mut link_open = true;
    let mut probe = tokio::time::interval(PROBE_EVERY);
    probe.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    probe.tick().await;

    for inst in initial {
        wanted.insert(line_key(&inst), inst.clone());
        start(&gateway, &secrets, &connector, &inst, &out, &ended_tx, &mut lines).await;
    }
    let ended: Option<anyhow::Error> = loop {
        tokio::select! {
            cmd = cmds.recv() => match cmd {
                Some(FeedCmd::Add(insts)) => {
                    for inst in insts {
                        let key = line_key(&inst);
                        wanted.insert(key.clone(), inst.clone());
                        if !lines.contains_key(&key) {
                            start(&gateway, &secrets, &connector, &inst, &out, &ended_tx, &mut lines).await;
                        }
                    }
                }
                Some(FeedCmd::Remove(insts)) => {
                    for inst in insts {
                        let key = line_key(&inst);
                        wanted.remove(&key);
                        if let Some(task) = lines.remove(&key) {
                            task.abort();
                        }
                    }
                }
                None => break None,
            },
            Some(key) = ended_rx.recv() => {
                lines.remove(&key);
                if wanted.contains_key(&key) {
                    retries.spawn(async move {
                        tokio::time::sleep(RESUBSCRIBE_AFTER).await;
                        key
                    });
                }
            }
            Some(Ok(key)) = retries.join_next() => {
                if let (Some(inst), false) = (wanted.get(&key), lines.contains_key(&key)) {
                    let inst = inst.clone();
                    start(&gateway, &secrets, &connector, &inst, &out, &ended_tx, &mut lines).await;
                }
            }
            changed = link.changed(), if link_open => {
                if changed.is_err() {
                    link_open = false;
                    continue;
                }
                let now = link.borrow_and_update().clone();
                if let Some(why) = now.lost {
                    break Some(live_err(ConnectorError::new(ErrorKind::Transient, why)));
                }
                if now.resubscribe != placed {
                    break Some(live_err(ConnectorError::new(
                        ErrorKind::Transient,
                        "Interactive Brokers restored its connection but dropped the live subscriptions; placing them again",
                    )));
                }
            }
            _ = probe.tick(), if !wanted.is_empty() => {
                if let Err(e) = gateway.probe(&secrets).await {
                    if dropped(&e) {
                        break Some(live_err(e));
                    }
                }
            }
        }
    };
    for (_, task) in lines {
        task.abort();
    }
    if let Some(e) = ended {
        let _ = out.send(Err(e)).await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn start(
    gateway: &IbkrGateway,
    secrets: &HashMap<String, String>,
    connector: &str,
    inst: &Instrument,
    out: &mpsc::Sender<Result<FeedUpdate>>,
    ended: &mpsc::UnboundedSender<String>,
    lines: &mut HashMap<String, JoinHandle<()>>,
) {
    let channel = line_key(inst);
    let opened = async {
        let id = gateway.instrument_for(&inst.symbol, crate::histdata::native::market(&inst.asset_type))?;
        if inst.is_quote() {
            gateway.stream_quotes(secrets, &id, connector).await
        } else {
            gateway.stream(secrets, &id, connector).await
        }
    };
    match opened.await {
        Ok(line) => {
            lines.insert(channel.clone(), tokio::spawn(forward(line, channel, out.clone(), ended.clone())));
        }
        Err(e) => {
            let e = live_err(e);
            let passing = !super::super::fault_of(&e).terminal();
            let _ = out.send(Ok(FeedUpdate::fault(channel.clone(), &e))).await;
            if passing {
                let _ = ended.send(channel);
            }
        }
    }
}

fn at(t: otw_connector_model::Timestamp) -> Option<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(t.nanos())).ok()
}

/// One canonical update as the hub's.
fn update(channel: &str, env: otw_connector_model::Envelope) -> Result<Option<FeedUpdate>> {
    Ok(match env.event {
        Event::Trade(t) => {
            let ts = at(env.exchange_ts.unwrap_or(env.received_ts)).unwrap_or_else(OffsetDateTime::now_utc);
            Some(FeedUpdate::trade(channel, f(t.price.0)?, f(t.qty.0)?, ts))
        }
        Event::Candle(c) => Some(FeedUpdate::bar(
            channel,
            LiveEvent {
                bar_ts: at(c.open_ts).unwrap_or_else(OffsetDateTime::now_utc),
                open: f(c.open.0)?,
                high: f(c.high.0)?,
                low: f(c.low.0)?,
                close: f(c.close.0)?,
                volume: f(c.volume.0)?,
                closed: c.is_final,
                provider_ts: OffsetDateTime::now_utc(),
            },
        )),
        Event::Quote(q) => {
            let ts = at(env.exchange_ts.unwrap_or(env.received_ts)).unwrap_or_else(OffsetDateTime::now_utc);
            LiveQuote::new(f(q.bid.0)?, f(q.ask.0)?, ts).map(|q| FeedUpdate::quote(channel, q))
        }
        _ => None,
    })
}

/// Drain one line onto the feed until it ends or the line is removed (dropping the line stops
/// the subscription). A line that ended on anything but a dead socket or a terminal refusal
/// reports on `ended`, so it is placed again.
async fn forward(mut line: otw_connector_providers::ibkr::gateway::LiveLine, channel: String, out: mpsc::Sender<Result<FeedUpdate>>, ended: mpsc::UnboundedSender<String>) {
    while let Some(item) = line.updates.recv().await {
        let sent = match item {
            Ok(env) => match update(&channel, env) {
                Ok(Some(u)) => out.send(Ok(u)).await,
                Ok(None) => continue,
                Err(e) => out.send(Ok(FeedUpdate::fault(channel.clone(), &e))).await,
            },
            Err(e) if dropped(&e) => {
                let _ = out.send(Err(live_err(e))).await;
                return;
            }
            Err(e) => {
                let e = live_err(e);
                let passing = !super::super::fault_of(&e).terminal();
                let _ = out.send(Ok(FeedUpdate::fault(channel.clone(), &e))).await;
                if passing {
                    let _ = ended.send(channel);
                }
                return;
            }
        };
        if sent.is_err() {
            return;
        }
    }
}

fn dropped(e: &ConnectorError) -> bool {
    e.provider_code.as_deref() == Some(mapper::DROPPED)
}
