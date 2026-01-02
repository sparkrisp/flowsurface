use super::{
    super::{
        Exchange, Kline, MarketKind, PushFrequency, StreamKind, Ticker, TickerInfo, TickerStats,
        Timeframe,
        connect::connect_ws,
    },
    AdapterError, Event,
};

use chrono::{DateTime, NaiveDateTime, Utc};
use fastwebsockets::Frame;
use iced_futures::{
    futures::{SinkExt, Stream},
    stream,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

const REST_BASE: &str = "https://api.twelvedata.com";
const WS_DOMAIN: &str = "ws.twelvedata.com";
const WS_URL: &str = "wss://ws.twelvedata.com/v1/quotes/price";

const DEFAULT_SYMBOLS: [&str; 10] = [
    "EUR/USD",
    "GBP/USD",
    "USD/JPY",
    "USD/CHF",
    "AUD/USD",
    "NZD/USD",
    "USD/CAD",
    "EUR/GBP",
    "EUR/JPY",
    "GBP/JPY",
];

#[derive(Clone)]
struct TwelveDataConfig {
    api_key: String,
}

fn td_config() -> Result<TwelveDataConfig, AdapterError> {
    let api_key = std::env::var("TWELVEDATA_API_KEY")
        .map_err(|_| AdapterError::InvalidRequest("Missing env TWELVEDATA_API_KEY".to_string()))?;
    Ok(TwelveDataConfig { api_key })
}

fn maybe_td_config() -> Option<TwelveDataConfig> {
    let api_key = std::env::var("TWELVEDATA_API_KEY").ok()?;
    Some(TwelveDataConfig { api_key })
}

fn configured_symbols() -> Vec<String> {
    let Some(raw) = std::env::var("TWELVEDATA_SYMBOLS").ok() else {
        return DEFAULT_SYMBOLS.iter().map(|s| s.to_string()).collect();
    };

    let list = raw
        .split(',')
        .map(|s| normalize_symbol(s))
        .filter(|s| !s.is_empty())
        .collect::<Vec<String>>();

    if list.is_empty() {
        DEFAULT_SYMBOLS.iter().map(|s| s.to_string()).collect()
    } else {
        list
    }
}

fn normalize_symbol(symbol: &str) -> String {
    let s = symbol.trim().to_uppercase();
    if s.contains('/') {
        return s;
    }
    if s.contains('_') {
        return s.replace('_', "/");
    }
    if s.len() == 6 {
        return format!("{}/{}", &s[..3], &s[3..]);
    }
    s
}

fn guess_min_ticksize(symbol: &str) -> f32 {
    let parts: Vec<&str> = symbol.split('/').collect();
    let quote = parts.get(1).copied().unwrap_or("");
    if quote == "JPY" {
        0.01
    } else {
        0.0001
    }
}

pub async fn fetch_ticker_info(
    _market_type: MarketKind,
) -> Result<HashMap<Ticker, Option<TickerInfo>>, AdapterError> {
    if maybe_td_config().is_none() {
        return Ok(HashMap::new());
    }

    let symbols = configured_symbols();

    let mut map = HashMap::new();
    for symbol in symbols {
        let ticker = Ticker::new(&symbol, Exchange::TwelveDataFx);
        let min_tick = guess_min_ticksize(&symbol);
        let info = TickerInfo::new(ticker, min_tick, 1.0, None);
        map.insert(ticker, Some(info));
    }

    Ok(map)
}

pub async fn fetch_ticker_prices(
    _market_type: MarketKind,
) -> Result<HashMap<Ticker, TickerStats>, AdapterError> {
    let Some(cfg) = maybe_td_config() else {
        return Ok(HashMap::new());
    };

    let symbols = configured_symbols();
    if symbols.is_empty() {
        return Ok(HashMap::new());
    }

    let client = reqwest::Client::new();
    let symbol_list = symbols.join(",");
    let url = format!("{REST_BASE}/price");
    let resp = client
        .get(url)
        .query(&[("symbol", symbol_list), ("apikey", cfg.api_key)])
        .send()
        .await?;

    let text = resp.text().await?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| AdapterError::ParseError(format!("Invalid JSON: {e}")))?;

    if let Some(status) = value.get("status").and_then(|v| v.as_str()) {
        if status == "error" {
            let msg = value
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Twelve Data error");
            return Err(AdapterError::InvalidRequest(msg.to_string()));
        }
    }

    let mut stats = HashMap::new();

    if let Some(price) = value.get("price") {
        let p = price
            .as_str()
            .and_then(|s| s.parse::<f32>().ok())
            .or_else(|| price.as_f64().map(|f| f as f32));
        if let Some(p) = p {
            let symbol = symbols.first().cloned().unwrap_or_default();
            let ticker = Ticker::new(&symbol, Exchange::TwelveDataFx);
            stats.insert(
                ticker,
                TickerStats {
                    mark_price: p,
                    daily_price_chg: 0.0,
                    daily_volume: 0.0,
                },
            );
        }
        return Ok(stats);
    }

    if let Some(obj) = value.as_object() {
        for (symbol, entry) in obj {
            let price = entry
                .get("price")
                .and_then(|v| v.as_str().and_then(|s| s.parse::<f32>().ok())
                    .or_else(|| v.as_f64().map(|f| f as f32)));
            if let Some(p) = price {
                let ticker = Ticker::new(symbol, Exchange::TwelveDataFx);
                stats.insert(
                    ticker,
                    TickerStats {
                        mark_price: p,
                        daily_price_chg: 0.0,
                        daily_volume: 0.0,
                    },
                );
            }
        }
    }

    Ok(stats)
}

#[derive(Deserialize)]
struct TimeSeriesResponse {
    values: Vec<TimeSeriesValue>,
}

#[derive(Deserialize)]
struct TimeSeriesValue {
    datetime: String,
    open: String,
    high: String,
    low: String,
    close: String,
    volume: Option<String>,
}

pub async fn fetch_klines(
    ticker_info: TickerInfo,
    timeframe: Timeframe,
    range: Option<(u64, u64)>,
) -> Result<Vec<Kline>, AdapterError> {
    let cfg = td_config()?;
    let mut aggregate_interval: Option<u64> = None;
    let interval = match timeframe {
        Timeframe::M3 => {
            aggregate_interval = Some(Timeframe::M3.to_milliseconds());
            "1min"
        }
        _ => timeframe_to_interval(timeframe)?,
    };

    let symbol = ticker_info.ticker.to_full_symbol_and_type().0;
    let url = format!("{REST_BASE}/time_series");
    let mut req = reqwest::Client::new()
        .get(url)
        .query(&[
            ("symbol", symbol),
            ("interval", interval.to_string()),
            ("apikey", cfg.api_key.clone()),
            ("outputsize", "500".to_string()),
        ]);

    if let Some((from, to)) = range {
        let from_dt = DateTime::<Utc>::from_timestamp_millis(from as i64)
            .ok_or_else(|| AdapterError::InvalidRequest("invalid from timestamp".to_string()))?;
        let to_dt = DateTime::<Utc>::from_timestamp_millis(to as i64)
            .ok_or_else(|| AdapterError::InvalidRequest("invalid to timestamp".to_string()))?;
        req = req.query(&[
            ("start_date", from_dt.format("%Y-%m-%d %H:%M:%S").to_string()),
            ("end_date", to_dt.format("%Y-%m-%d %H:%M:%S").to_string()),
        ]);
    }

    let resp = req.send().await?;
    let text = resp.text().await?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| AdapterError::ParseError(format!("Invalid JSON: {e}")))?;
    if let Some(status) = value.get("status").and_then(|v| v.as_str()) {
        if status == "error" {
            let msg = value
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Twelve Data error");
            return Err(AdapterError::InvalidRequest(msg.to_string()));
        }
    }

    let data: TimeSeriesResponse = serde_json::from_value(value)
        .map_err(|e| AdapterError::ParseError(format!("Time series parse: {e}")))?;

    let mut raw = Vec::with_capacity(data.values.len());
    for entry in data.values {
        let time = parse_datetime_ms(&entry.datetime).ok_or_else(|| {
            AdapterError::ParseError(format!("Invalid datetime {}", entry.datetime))
        })?;
        let open = entry.open.parse::<f32>().unwrap_or(0.0);
        let high = entry.high.parse::<f32>().unwrap_or(0.0);
        let low = entry.low.parse::<f32>().unwrap_or(0.0);
        let close = entry.close.parse::<f32>().unwrap_or(0.0);
        let volume = entry
            .volume
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);

        raw.push(RawCandle {
            time,
            open,
            high,
            low,
            close,
            volume,
        });
    }

    raw.sort_by_key(|c| c.time);

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
                Exchange::TwelveDataFx,
                "Twelve Data does not provide depth streams".to_string(),
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
        let Ok(cfg) = td_config() else {
            let _ = output
                .send(Event::Disconnected(
                    Exchange::TwelveDataFx,
                    "Missing TWELVEDATA_API_KEY".to_string(),
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

        let symbol_list = instruments.join(",");
        let subscribe = serde_json::json!({
            "action": "subscribe",
            "params": { "symbols": symbol_list }
        });

        let mut states: HashMap<(Ticker, Timeframe), CandleState> = HashMap::new();

        loop {
            let url = format!("{WS_URL}?apikey={}", cfg.api_key);
            match connect_ws(WS_DOMAIN, &url).await {
                Ok(mut websocket) => {
                    let _ = websocket
                        .write_frame(Frame::text(fastwebsockets::Payload::Borrowed(
                            subscribe.to_string().as_bytes(),
                        )))
                        .await;

                    let _ = output.send(Event::Connected(Exchange::TwelveDataFx)).await;

                    loop {
                        match websocket.read_frame().await {
                            Ok(msg) => {
                                if msg.opcode != fastwebsockets::OpCode::Text {
                                    continue;
                                }

                                let Ok(value) = serde_json::from_slice::<Value>(&msg.payload) else {
                                    continue;
                                };

                                let (symbol, price, time_ms) = match parse_price_event(&value) {
                                    Some(v) => v,
                                    None => continue,
                                };

                                let Some(timeframes) = instrument_timeframes.get(&symbol) else {
                                    continue;
                                };
                                let Some(ticker_info) = ticker_map.get(&symbol).copied() else {
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
                            Err(err) => {
                                let _ = output
                                    .send(Event::Disconnected(
                                        Exchange::TwelveDataFx,
                                        format!("Websocket error: {err}"),
                                    ))
                                    .await;
                                break;
                            }
                        }
                    }
                }
                Err(err) => {
                    let _ = output
                        .send(Event::Disconnected(
                            Exchange::TwelveDataFx,
                            format!("Failed to connect: {err}"),
                        ))
                        .await;
                }
            }

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
}

fn parse_price_event(value: &Value) -> Option<(String, f32, u64)> {
    let symbol = value.get("symbol").and_then(|v| v.as_str())?.to_string();
    let parse_num = |v: &Value| -> Option<f32> {
        v.as_str()
            .and_then(|s| s.parse::<f32>().ok())
            .or_else(|| v.as_f64().map(|f| f as f32))
    };

    let price = value
        .get("price")
        .and_then(parse_num)
        .or_else(|| {
            let bid = value.get("bid").and_then(parse_num)?;
            let ask = value.get("ask").and_then(parse_num)?;
            Some((bid + ask) * 0.5)
        })?;

    let time_ms = value
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .map(|ts| ts * 1000)
        .unwrap_or_else(|| Utc::now().timestamp_millis());

    Some((normalize_symbol(&symbol), price, time_ms as u64))
}

fn timeframe_to_interval(timeframe: Timeframe) -> Result<&'static str, AdapterError> {
    match timeframe {
        Timeframe::M1 => Ok("1min"),
        Timeframe::M5 => Ok("5min"),
        Timeframe::M15 => Ok("15min"),
        Timeframe::M30 => Ok("30min"),
        Timeframe::H1 => Ok("1h"),
        Timeframe::H2 => Ok("2h"),
        Timeframe::H4 => Ok("4h"),
        Timeframe::H12 => Ok("12h"),
        Timeframe::D1 => Ok("1day"),
        _ => Err(AdapterError::InvalidRequest(
            "Unsupported Twelve Data timeframe".to_string(),
        )),
    }
}

fn is_supported_timeframe(timeframe: Timeframe) -> bool {
    matches!(timeframe, Timeframe::M1 | Timeframe::M3 | Timeframe::M5 | Timeframe::M15
        | Timeframe::M30 | Timeframe::H1 | Timeframe::H2 | Timeframe::H4 | Timeframe::H12
        | Timeframe::D1)
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

fn parse_datetime_ms(value: &str) -> Option<u64> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Some(dt.timestamp_millis() as u64);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).timestamp_millis() as u64);
    }
    None
}
