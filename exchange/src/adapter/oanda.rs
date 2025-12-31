use super::{
    super::{
        Exchange, Kline, MarketKind, PushFrequency, StreamKind, Ticker, TickerInfo, TickerStats,
        Timeframe,
    },
    AdapterError, Event,
};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use iced_futures::{
    futures::{Stream, StreamExt},
    stream,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const PRACTICE_REST: &str = "https://api-fxpractice.oanda.com";
const PRACTICE_STREAM: &str = "https://stream-fxpractice.oanda.com";
const LIVE_REST: &str = "https://api-fxtrade.oanda.com";
const LIVE_STREAM: &str = "https://stream-fxtrade.oanda.com";

#[derive(Clone)]
struct OandaConfig {
    token: String,
    account_id: String,
    rest_base: &'static str,
    stream_base: &'static str,
}

fn oanda_config() -> Result<OandaConfig, AdapterError> {
    let token = std::env::var("OANDA_API_TOKEN")
        .map_err(|_| AdapterError::InvalidRequest("Missing env OANDA_API_TOKEN".to_string()))?;
    let account_id = std::env::var("OANDA_ACCOUNT_ID").map_err(|_| {
        AdapterError::InvalidRequest("Missing env OANDA_ACCOUNT_ID".to_string())
    })?;

    let env = std::env::var("OANDA_ENV").unwrap_or_else(|_| "practice".to_string());
    let env = env.to_ascii_lowercase();
    let (rest_base, stream_base) = if matches!(env.as_str(), "live" | "fxtrade") {
        (LIVE_REST, LIVE_STREAM)
    } else {
        (PRACTICE_REST, PRACTICE_STREAM)
    };

    Ok(OandaConfig {
        token,
        account_id,
        rest_base,
        stream_base,
    })
}

fn maybe_oanda_config() -> Option<OandaConfig> {
    let token = std::env::var("OANDA_API_TOKEN").ok()?;
    let account_id = std::env::var("OANDA_ACCOUNT_ID").ok()?;

    let env = std::env::var("OANDA_ENV").unwrap_or_else(|_| "practice".to_string());
    let env = env.to_ascii_lowercase();
    let (rest_base, stream_base) = if matches!(env.as_str(), "live" | "fxtrade") {
        (LIVE_REST, LIVE_STREAM)
    } else {
        (PRACTICE_REST, PRACTICE_STREAM)
    };

    Some(OandaConfig {
        token,
        account_id,
        rest_base,
        stream_base,
    })
}

#[derive(Deserialize)]
struct InstrumentsResponse {
    instruments: Vec<OandaInstrument>,
}

#[derive(Deserialize)]
struct OandaInstrument {
    name: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "displayPrecision")]
    display_precision: i32,
    #[serde(rename = "minimumTradeSize")]
    minimum_trade_size: String,
}

#[derive(Deserialize)]
struct PricingResponse {
    prices: Vec<OandaPrice>,
}

#[derive(Deserialize)]
struct OandaPrice {
    instrument: String,
    bids: Vec<OandaPriceLevel>,
    asks: Vec<OandaPriceLevel>,
}

#[derive(Deserialize)]
struct OandaPriceLevel {
    price: String,
}

#[derive(Deserialize)]
struct CandlesResponse {
    candles: Vec<OandaCandle>,
}

#[derive(Deserialize)]
struct OandaCandle {
    time: String,
    volume: i64,
    mid: OandaCandlePrice,
}

#[derive(Deserialize)]
struct OandaCandlePrice {
    o: String,
    h: String,
    l: String,
    c: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum StreamMessage {
    #[serde(rename = "PRICE")]
    Price {
        instrument: String,
        time: String,
        bids: Vec<OandaPriceLevel>,
        asks: Vec<OandaPriceLevel>,
    },
    #[serde(rename = "HEARTBEAT")]
    Heartbeat { time: String },
}

pub async fn fetch_ticker_info(
    _market_type: MarketKind,
) -> Result<HashMap<Ticker, Option<TickerInfo>>, AdapterError> {
    let Some(cfg) = maybe_oanda_config() else {
        return Ok(HashMap::new());
    };

    let url = format!("{}/v3/accounts/{}/instruments", cfg.rest_base, cfg.account_id);
    let resp = reqwest::Client::new()
        .get(url)
        .bearer_auth(&cfg.token)
        .send()
        .await?;

    let data: InstrumentsResponse = resp.json().await?;

    let mut map = HashMap::new();
    for instrument in data.instruments {
        let ticker = Ticker::new_with_display(
            &instrument.name,
            Exchange::OandaFx,
            instrument.display_name.as_deref(),
        );
        let min_tick = 10f32.powi(-instrument.display_precision);
        let min_qty = instrument
            .minimum_trade_size
            .parse::<f32>()
            .unwrap_or(1.0);
        let info = TickerInfo::new(ticker, min_tick, min_qty, None);
        map.insert(ticker, Some(info));
    }

    Ok(map)
}

pub async fn fetch_ticker_prices(
    _market_type: MarketKind,
) -> Result<HashMap<Ticker, TickerStats>, AdapterError> {
    let Some(cfg) = maybe_oanda_config() else {
        return Ok(HashMap::new());
    };

    let url = format!("{}/v3/accounts/{}/instruments", cfg.rest_base, cfg.account_id);
    let resp = reqwest::Client::new()
        .get(url)
        .bearer_auth(&cfg.token)
        .send()
        .await?;
    let data: InstrumentsResponse = resp.json().await?;
    let instruments: Vec<String> = data.instruments.into_iter().map(|i| i.name).collect();
    if instruments.is_empty() {
        return Ok(HashMap::new());
    }

    let url = format!(
        "{}/v3/accounts/{}/pricing?instruments={}",
        cfg.rest_base,
        cfg.account_id,
        instruments.join(",")
    );
    let resp = reqwest::Client::new()
        .get(url)
        .bearer_auth(&cfg.token)
        .send()
        .await?;

    let data: PricingResponse = resp.json().await?;

    let mut map = HashMap::new();
    for price in data.prices {
        let bid = price
            .bids
            .first()
            .and_then(|b| b.price.parse::<f32>().ok());
        let ask = price
            .asks
            .first()
            .and_then(|a| a.price.parse::<f32>().ok());

        if let (Some(bid), Some(ask)) = (bid, ask) {
            let mid = (bid + ask) * 0.5;
            let ticker = Ticker::new(&price.instrument, Exchange::OandaFx);
            map.insert(
                ticker,
                TickerStats {
                    mark_price: mid,
                    daily_price_chg: 0.0,
                    daily_volume: 0.0,
                },
            );
        }
    }

    Ok(map)
}

pub async fn fetch_klines(
    ticker_info: TickerInfo,
    timeframe: Timeframe,
    range: Option<(u64, u64)>,
) -> Result<Vec<Kline>, AdapterError> {
    let cfg = oanda_config()?;
    let mut aggregate_interval: Option<u64> = None;
    let granularity = match timeframe {
        Timeframe::M3 => {
            aggregate_interval = Some(Timeframe::M3.to_milliseconds());
            "M1"
        }
        _ => timeframe_to_granularity(timeframe)?,
    };

    let mut url = format!(
        "{}/v3/instruments/{}/candles?price=M&granularity={}",
        cfg.rest_base,
        ticker_info.ticker.to_full_symbol_and_type().0,
        granularity
    );

    if let Some((from, to)) = range {
        let from_dt = DateTime::<Utc>::from_timestamp_millis(from as i64)
            .ok_or_else(|| AdapterError::InvalidRequest("invalid from timestamp".to_string()))?;
        let to_dt = DateTime::<Utc>::from_timestamp_millis(to as i64)
            .ok_or_else(|| AdapterError::InvalidRequest("invalid to timestamp".to_string()))?;
        url.push_str(&format!("&from={}&to={}", from_dt.to_rfc3339(), to_dt.to_rfc3339()));
    } else {
        url.push_str("&count=500");
    }

    let resp = reqwest::Client::new()
        .get(url)
        .bearer_auth(&cfg.token)
        .send()
        .await?;
    let data: CandlesResponse = resp.json().await?;

    let mut raw = Vec::with_capacity(data.candles.len());
    for candle in data.candles {
        let time = DateTime::parse_from_rfc3339(&candle.time)
            .map_err(|e| AdapterError::ParseError(e.to_string()))?
            .timestamp_millis() as u64;

        let open = candle.mid.o.parse::<f32>().unwrap_or(0.0);
        let high = candle.mid.h.parse::<f32>().unwrap_or(0.0);
        let low = candle.mid.l.parse::<f32>().unwrap_or(0.0);
        let close = candle.mid.c.parse::<f32>().unwrap_or(0.0);
        let volume = candle.volume.max(0) as f32;

        raw.push(RawCandle {
            time,
            open,
            high,
            low,
            close,
            volume,
        });
    }

    let klines = if let Some(interval_ms) = aggregate_interval {
        aggregate_klines(raw, interval_ms, ticker_info.min_ticksize)
    } else {
        raw.into_iter()
            .map(|c| {
                Kline::new(
                    c.time,
                    c.open,
                    c.high,
                    c.low,
                    c.close,
                    (c.volume, 0.0),
                    ticker_info.min_ticksize,
                )
            })
            .collect()
    };

    Ok(klines)
}

pub fn connect_market_stream(_ticker_info: TickerInfo, _push_freq: PushFrequency) -> impl Stream<Item = Event> {
    stream::channel(1, async move |mut output| {
        let _ = output
            .send(Event::Disconnected(
                Exchange::OandaFx,
                "OANDA does not provide depth streams".to_string(),
            ))
            .await;
    })
}

#[derive(Clone, Copy)]
struct CandleState {
    start: u64,
    open: f32,
    high: f32,
    low: f32,
    close: f32,
    ticks: u32,
}

pub fn connect_kline_stream(
    streams: Vec<(TickerInfo, Timeframe)>,
    _market_type: MarketKind,
) -> impl Stream<Item = Event> {
    stream::channel(100, async move |mut output| {
        let Ok(cfg) = oanda_config() else {
            let _ = output
                .send(Event::Disconnected(
                    Exchange::OandaFx,
                    "Missing OANDA env vars".to_string(),
                ))
                .await;
            return;
        };

        let mut instrument_timeframes: HashMap<String, Vec<Timeframe>> = HashMap::new();
        let mut ticker_map: HashMap<String, TickerInfo> = HashMap::new();

        for (ticker_info, tf) in &streams {
            if !is_supported_timeframe(*tf) {
                continue;
            }
            let instrument = ticker_info.ticker.to_full_symbol_and_type().0;
            instrument_timeframes
                .entry(instrument.clone())
                .or_default()
                .push(*tf);
            ticker_map.entry(instrument).or_insert(*ticker_info);
        }

        let instruments: Vec<String> = instrument_timeframes.keys().cloned().collect();
        if instruments.is_empty() {
            return;
        }

        let mut states: HashMap<(Ticker, Timeframe), CandleState> = HashMap::new();
        let client = reqwest::Client::new();

        loop {
            let url = format!(
                "{}/v3/accounts/{}/pricing/stream?instruments={}&snapshot=true",
                cfg.stream_base,
                cfg.account_id,
                instruments.join(",")
            );

            let resp = client.get(&url).bearer_auth(&cfg.token).send().await;
            let mut resp = match resp {
                Ok(r) => r,
                Err(err) => {
                    let _ = output
                        .send(Event::Disconnected(
                            Exchange::OandaFx,
                            format!("OANDA stream error: {err}"),
                        ))
                        .await;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            if !resp.status().is_success() {
                let _ = output
                    .send(Event::Disconnected(
                        Exchange::OandaFx,
                        format!("OANDA stream status {}", resp.status()),
                    ))
                    .await;
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }

            let _ = output.send(Event::Connected(Exchange::OandaFx)).await;

            let mut buffer: Vec<u8> = Vec::new();
            let mut body = resp.bytes_stream();

            while let Some(chunk) = body.next().await {
                match chunk {
                    Ok(bytes) => {
                        append_and_process(
                            &mut buffer,
                            bytes,
                            &instrument_timeframes,
                            &ticker_map,
                            &mut states,
                            &mut output,
                        )
                        .await;
                    }
                    Err(err) => {
                        let _ = output
                            .send(Event::Disconnected(
                                Exchange::OandaFx,
                                format!("OANDA stream read error: {err}"),
                            ))
                            .await;
                        break;
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

async fn append_and_process(
    buffer: &mut Vec<u8>,
    chunk: Bytes,
    instrument_timeframes: &HashMap<String, Vec<Timeframe>>,
    ticker_map: &HashMap<String, TickerInfo>,
    states: &mut HashMap<(Ticker, Timeframe), CandleState>,
    output: &mut iced_futures::futures::channel::mpsc::Sender<Event>,
) {
    buffer.extend_from_slice(&chunk);

    while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
        let mut line = buffer.drain(..=pos).collect::<Vec<u8>>();
        if let Some(b'\n') = line.last() {
            line.pop();
        }
        if let Some(b'\r') = line.last() {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }

        let Ok(msg) = serde_json::from_slice::<StreamMessage>(&line) else {
            continue;
        };

        match msg {
            StreamMessage::Price {
                instrument,
                time,
                bids,
                asks,
            } => {
                let bid = bids.first().and_then(|b| b.price.parse::<f32>().ok());
                let ask = asks.first().and_then(|a| a.price.parse::<f32>().ok());
                let (Some(bid), Some(ask)) = (bid, ask) else { continue };

                let time_ms = match DateTime::parse_from_rfc3339(&time) {
                    Ok(t) => t.timestamp_millis() as u64,
                    Err(_) => continue,
                };
                let price = (bid + ask) * 0.5;

                let Some(timeframes) = instrument_timeframes.get(&instrument) else {
                    continue;
                };
                let Some(ticker_info) = ticker_map.get(&instrument).copied() else {
                    continue;
                };

                for tf in timeframes {
                    let interval_ms = tf.to_milliseconds();
                    let start = (time_ms / interval_ms) * interval_ms;
                    let key = (ticker_info.ticker, *tf);
                    let state = states.entry(key).or_insert(CandleState {
                        start,
                        open: price,
                        high: price,
                        low: price,
                        close: price,
                        ticks: 0,
                    });

                    if state.start != start {
                        *state = CandleState {
                            start,
                            open: price,
                            high: price,
                            low: price,
                            close: price,
                            ticks: 0,
                        };
                    } else {
                        state.high = state.high.max(price);
                        state.low = state.low.min(price);
                        state.close = price;
                    }
                    state.ticks = state.ticks.saturating_add(1);

                    let kline = Kline::new(
                        state.start,
                        state.open,
                        state.high,
                        state.low,
                        state.close,
                        (state.ticks as f32, 0.0),
                        ticker_info.min_ticksize,
                    );

                    let _ = output
                        .send(Event::KlineReceived(
                            StreamKind::Kline {
                                ticker_info,
                                timeframe: *tf,
                            },
                            kline,
                        ))
                        .await;
                }
            }
            StreamMessage::Heartbeat { .. } => {}
        }
    }
}

fn timeframe_to_granularity(timeframe: Timeframe) -> Result<&'static str, AdapterError> {
    match timeframe {
        Timeframe::M1 => Ok("M1"),
        Timeframe::M5 => Ok("M5"),
        Timeframe::M15 => Ok("M15"),
        Timeframe::M30 => Ok("M30"),
        Timeframe::H1 => Ok("H1"),
        Timeframe::H2 => Ok("H2"),
        Timeframe::H4 => Ok("H4"),
        Timeframe::H12 => Ok("H12"),
        Timeframe::D1 => Ok("D"),
        _ => Err(AdapterError::InvalidRequest(
            "Unsupported OANDA timeframe".to_string(),
        )),
    }
}

fn is_supported_timeframe(timeframe: Timeframe) -> bool {
    matches!(
        timeframe,
        Timeframe::M1
            | Timeframe::M3
            | Timeframe::M5
            | Timeframe::M15
            | Timeframe::M30
            | Timeframe::H1
            | Timeframe::H2
            | Timeframe::H4
            | Timeframe::H12
            | Timeframe::D1
    )
}

struct RawCandle {
    time: u64,
    open: f32,
    high: f32,
    low: f32,
    close: f32,
    volume: f32,
}

fn aggregate_klines(
    candles: Vec<RawCandle>,
    interval_ms: u64,
    min_ticksize: crate::util::MinTicksize,
) -> Vec<Kline> {
    let mut out = Vec::new();
    let mut current: Option<RawCandle> = None;
    let mut current_start: u64 = 0;

    for candle in candles {
        let start = (candle.time / interval_ms) * interval_ms;
        match current.as_mut() {
            Some(cur) if start == current_start => {
                cur.high = cur.high.max(candle.high);
                cur.low = cur.low.min(candle.low);
                cur.close = candle.close;
                cur.volume += candle.volume;
            }
            Some(cur) => {
                out.push(Kline::new(
                    current_start,
                    cur.open,
                    cur.high,
                    cur.low,
                    cur.close,
                    (cur.volume, 0.0),
                    min_ticksize,
                ));
                *cur = candle;
                current_start = start;
            }
            None => {
                current = Some(candle);
                current_start = start;
            }
        }
    }

    if let Some(cur) = current {
        out.push(Kline::new(
            current_start,
            cur.open,
            cur.high,
            cur.low,
            cur.close,
            (cur.volume, 0.0),
            min_ticksize,
        ));
    }

    out
}
