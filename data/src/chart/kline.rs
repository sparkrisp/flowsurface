use exchange::{
    Kline, Trade,
    util::{Price, PriceStep},
};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::aggr::time::DataPoint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum CandleSource {
    Close,
}

impl Default for CandleSource {
    fn default() -> Self {
        Self::Close
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum MovingAverageKind {
    SMA,
    EMA,
}

impl Default for MovingAverageKind {
    fn default() -> Self {
        Self::SMA
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum CandleStudy {
    MovingAverageFast {
        kind: MovingAverageKind,
        period: u16,
        source: CandleSource,
        /// RGB (no alpha). Stored explicitly so it persists per pane.
        color_rgb: [u8; 3],
    },
    MovingAverageSlow {
        kind: MovingAverageKind,
        period: u16,
        source: CandleSource,
        /// RGB (no alpha). Stored explicitly so it persists per pane.
        color_rgb: [u8; 3],
    },
    BollingerBands {
        period: u16,
        source: CandleSource,
        /// Stddev multiplier * 100 (e.g. 200 == 2.00)
        stddev_x100: u16,
        mid_color_rgb: [u8; 3],
        upper_color_rgb: [u8; 3],
        lower_color_rgb: [u8; 3],
    },
    VwapBands {
        source: CandleSource,
        /// Reset VWAP at UTC day boundary (00:00).
        reset_daily_utc: bool,
        /// Stddev multiplier * 100 for bands (e.g. 200 == 2.00)
        band_stddev_x100: u16,
        vwap_color_rgb: [u8; 3],
        upper_color_rgb: [u8; 3],
        lower_color_rgb: [u8; 3],
    },
    Supertrend {
        atr_period: u16,
        /// Multiplier * 100 (e.g. 300 == 3.00)
        multiplier_x100: u16,
        up_color_rgb: [u8; 3],
        down_color_rgb: [u8; 3],
    },
    EmaRibbon {
        min_period: u16,
        max_period: u16,
        step: u16,
        start_color_rgb: [u8; 3],
        end_color_rgb: [u8; 3],
    },
    DonchianChannels {
        period: u16,
        upper_color_rgb: [u8; 3],
        mid_color_rgb: [u8; 3],
        lower_color_rgb: [u8; 3],
    },
    KeltnerChannels {
        ema_period: u16,
        atr_period: u16,
        /// Multiplier * 100 (e.g. 150 == 1.50)
        multiplier_x100: u16,
        mid_color_rgb: [u8; 3],
        upper_color_rgb: [u8; 3],
        lower_color_rgb: [u8; 3],
    },
    Ichimoku {
        tenkan_period: u16,
        kijun_period: u16,
        senkou_period: u16,
        tenkan_color_rgb: [u8; 3],
        kijun_color_rgb: [u8; 3],
        span_a_color_rgb: [u8; 3],
        span_b_color_rgb: [u8; 3],
        lag_color_rgb: [u8; 3],
    },
}

impl CandleStudy {
    pub fn is_same_type(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl std::fmt::Display for CandleStudy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CandleStudy::MovingAverageFast { kind, period, .. } => {
                let name = match kind {
                    MovingAverageKind::SMA => "SMA",
                    MovingAverageKind::EMA => "EMA",
                };
                write!(f, "Fast {name}({period})")
            }
            CandleStudy::MovingAverageSlow { kind, period, .. } => {
                let name = match kind {
                    MovingAverageKind::SMA => "SMA",
                    MovingAverageKind::EMA => "EMA",
                };
                write!(f, "Slow {name}({period})")
            }
            CandleStudy::BollingerBands {
                period,
                stddev_x100,
                ..
            } => {
                let sd = *stddev_x100 as f32 / 100.0;
                write!(f, "Bollinger Bands ({period}, {sd:.2}σ)")
            }
            CandleStudy::VwapBands {
                reset_daily_utc,
                band_stddev_x100,
                ..
            } => {
                let sd = *band_stddev_x100 as f32 / 100.0;
                if *reset_daily_utc {
                    write!(f, "VWAP (daily UTC, {sd:.2}σ)")
                } else {
                    write!(f, "VWAP ({sd:.2}σ)")
                }
            }
            CandleStudy::Supertrend {
                atr_period,
                multiplier_x100,
                ..
            } => {
                let m = *multiplier_x100 as f32 / 100.0;
                write!(f, "Supertrend (ATR {atr_period}, x{m:.2})")
            }
            CandleStudy::EmaRibbon {
                min_period,
                max_period,
                step,
                ..
            } => write!(f, "EMA Ribbon ({min_period}-{max_period} step {step})"),
            CandleStudy::DonchianChannels { period, .. } => write!(f, "Donchian Channels ({period})"),
            CandleStudy::KeltnerChannels {
                ema_period,
                atr_period,
                multiplier_x100,
                ..
            } => {
                let m = *multiplier_x100 as f32 / 100.0;
                write!(f, "Keltner Channels (EMA {ema_period}, ATR {atr_period}, x{m:.2})")
            }
            CandleStudy::Ichimoku {
                tenkan_period,
                kijun_period,
                senkou_period,
                ..
            } => write!(
                f,
                "Ichimoku ({tenkan_period}/{kijun_period}/{senkou_period})"
            ),
        }
    }
}

#[derive(Clone)]
pub struct KlineDataPoint {
    pub kline: Kline,
    pub footprint: KlineTrades,
}

impl KlineDataPoint {
    pub fn max_cluster_qty(&self, cluster_kind: ClusterKind, highest: Price, lowest: Price) -> f32 {
        match cluster_kind {
            ClusterKind::BidAsk => self.footprint.max_qty_by(highest, lowest, f32::max),
            ClusterKind::DeltaProfile => self
                .footprint
                .max_qty_by(highest, lowest, |buy, sell| (buy - sell).abs()),
            ClusterKind::VolumeProfile => {
                self.footprint
                    .max_qty_by(highest, lowest, |buy, sell| buy + sell)
            }
        }
    }

    pub fn add_trade(&mut self, trade: &Trade, step: PriceStep) {
        self.footprint.add_trade_to_nearest_bin(trade, step);
    }

    pub fn poc_price(&self) -> Option<Price> {
        self.footprint.poc_price()
    }

    pub fn set_poc_status(&mut self, status: NPoc) {
        self.footprint.set_poc_status(status);
    }

    pub fn clear_trades(&mut self) {
        self.footprint.clear();
    }

    pub fn calculate_poc(&mut self) {
        self.footprint.calculate_poc();
    }

    pub fn last_trade_time(&self) -> Option<u64> {
        self.footprint.last_trade_t()
    }

    pub fn first_trade_time(&self) -> Option<u64> {
        self.footprint.first_trade_t()
    }
}

impl DataPoint for KlineDataPoint {
    fn add_trade(&mut self, trade: &Trade, step: PriceStep) {
        self.add_trade(trade, step);
    }

    fn clear_trades(&mut self) {
        self.clear_trades();
    }

    fn last_trade_time(&self) -> Option<u64> {
        self.last_trade_time()
    }

    fn first_trade_time(&self) -> Option<u64> {
        self.first_trade_time()
    }

    fn last_price(&self) -> Price {
        self.kline.close
    }

    fn kline(&self) -> Option<&Kline> {
        Some(&self.kline)
    }

    fn value_high(&self) -> Price {
        self.kline.high
    }

    fn value_low(&self) -> Price {
        self.kline.low
    }
}

#[derive(Debug, Clone, Default)]
pub struct GroupedTrades {
    pub buy_qty: f32,
    pub sell_qty: f32,
    pub first_time: u64,
    pub last_time: u64,
    pub buy_count: usize,
    pub sell_count: usize,
}

impl GroupedTrades {
    fn new(trade: &Trade) -> Self {
        Self {
            buy_qty: if trade.is_sell { 0.0 } else { trade.qty },
            sell_qty: if trade.is_sell { trade.qty } else { 0.0 },
            first_time: trade.time,
            last_time: trade.time,
            buy_count: if trade.is_sell { 0 } else { 1 },
            sell_count: if trade.is_sell { 1 } else { 0 },
        }
    }

    fn add_trade(&mut self, trade: &Trade) {
        if trade.is_sell {
            self.sell_qty += trade.qty;
            self.sell_count += 1;
        } else {
            self.buy_qty += trade.qty;
            self.buy_count += 1;
        }
        self.last_time = trade.time;
    }

    pub fn total_qty(&self) -> f32 {
        self.buy_qty + self.sell_qty
    }

    pub fn delta_qty(&self) -> f32 {
        self.buy_qty - self.sell_qty
    }
}

#[derive(Debug, Clone, Default)]
pub struct KlineTrades {
    pub trades: FxHashMap<Price, GroupedTrades>,
    pub poc: Option<PointOfControl>,
}

impl KlineTrades {
    pub fn new() -> Self {
        Self {
            trades: FxHashMap::default(),
            poc: None,
        }
    }

    pub fn first_trade_t(&self) -> Option<u64> {
        self.trades.values().map(|group| group.first_time).min()
    }

    pub fn last_trade_t(&self) -> Option<u64> {
        self.trades.values().map(|group| group.last_time).max()
    }

    /// Add trade to the bin at the step multiple computed with side-based rounding.
    /// Intended for order-book ladder/quotes; Floor for sells, ceil for buys.
    /// Introduces side bias at bin edges and should not be used for OHLC/footprint aggregation
    pub fn add_trade_to_side_bin(&mut self, trade: &Trade, step: PriceStep) {
        let price = trade.price.round_to_side_step(trade.is_sell, step);

        self.trades
            .entry(price)
            .and_modify(|group| group.add_trade(trade))
            .or_insert_with(|| GroupedTrades::new(trade));
    }

    /// Add trade to the bin at the nearest step multiple (side-agnostic).
    /// Ties (exactly half a step) round up to the higher multiple.
    /// Intended for footprint/OHLC trade aggregation
    pub fn add_trade_to_nearest_bin(&mut self, trade: &Trade, step: PriceStep) {
        let price = trade.price.round_to_step(step);

        self.trades
            .entry(price)
            .and_modify(|group| group.add_trade(trade))
            .or_insert_with(|| GroupedTrades::new(trade));
    }

    pub fn max_qty_by<F>(&self, highest: Price, lowest: Price, f: F) -> f32
    where
        F: Fn(f32, f32) -> f32,
    {
        let mut max_qty: f32 = 0.0;
        for (price, group) in &self.trades {
            if *price >= lowest && *price <= highest {
                max_qty = max_qty.max(f(group.buy_qty, group.sell_qty));
            }
        }
        max_qty
    }

    pub fn calculate_poc(&mut self) {
        if self.trades.is_empty() {
            return;
        }

        let mut max_volume = 0.0;
        let mut poc_price = Price::from_f32(0.0);

        for (price, group) in &self.trades {
            let total_volume = group.total_qty();
            if total_volume > max_volume {
                max_volume = total_volume;
                poc_price = *price;
            }
        }

        self.poc = Some(PointOfControl {
            price: poc_price,
            volume: max_volume,
            status: NPoc::default(),
        });
    }

    pub fn set_poc_status(&mut self, status: NPoc) {
        if let Some(poc) = &mut self.poc {
            poc.status = status;
        }
    }

    pub fn poc_price(&self) -> Option<Price> {
        self.poc.map(|poc| poc.price)
    }

    pub fn clear(&mut self) {
        self.trades.clear();
        self.poc = None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum KlineChartKind {
    #[default]
    Candles,
    CandlesStudied {
        #[serde(default)]
        studies: Vec<CandleStudy>,
    },
    Footprint {
        clusters: ClusterKind,
        #[serde(default)]
        scaling: ClusterScaling,
        studies: Vec<FootprintStudy>,
    },
}

impl KlineChartKind {
    pub fn min_scaling(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 0.4,
            KlineChartKind::Candles | KlineChartKind::CandlesStudied { .. } => 0.6,
        }
    }

    pub fn max_scaling(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 1.2,
            KlineChartKind::Candles | KlineChartKind::CandlesStudied { .. } => 2.5,
        }
    }

    pub fn max_cell_width(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 360.0,
            KlineChartKind::Candles | KlineChartKind::CandlesStudied { .. } => 16.0,
        }
    }

    pub fn min_cell_width(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 80.0,
            KlineChartKind::Candles | KlineChartKind::CandlesStudied { .. } => 1.0,
        }
    }

    pub fn max_cell_height(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 90.0,
            KlineChartKind::Candles | KlineChartKind::CandlesStudied { .. } => 8.0,
        }
    }

    pub fn min_cell_height(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 1.0,
            KlineChartKind::Candles | KlineChartKind::CandlesStudied { .. } => 0.001,
        }
    }

    pub fn default_cell_width(&self) -> f32 {
        match self {
            KlineChartKind::Footprint { .. } => 80.0,
            KlineChartKind::Candles | KlineChartKind::CandlesStudied { .. } => 4.0,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum ClusterKind {
    #[default]
    BidAsk,
    VolumeProfile,
    DeltaProfile,
}

impl ClusterKind {
    pub const ALL: [ClusterKind; 3] = [
        ClusterKind::BidAsk,
        ClusterKind::VolumeProfile,
        ClusterKind::DeltaProfile,
    ];
}

impl std::fmt::Display for ClusterKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClusterKind::BidAsk => write!(f, "Bid/Ask"),
            ClusterKind::VolumeProfile => write!(f, "Volume Profile"),
            ClusterKind::DeltaProfile => write!(f, "Delta Profile"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub rsi_period: u16,
    /// RGB (no alpha) for RSI line.
    pub rsi_color_rgb: [u8; 3],
    pub macd_fast: u16,
    pub macd_slow: u16,
    pub macd_signal: u16,
    /// RGB (no alpha) for MACD line.
    pub macd_color_rgb: [u8; 3],
    /// RGB (no alpha) for Signal line.
    pub macd_signal_color_rgb: [u8; 3],

    pub atr_period: u16,
    pub atr_color_rgb: [u8; 3],

    pub stoch_rsi_rsi_period: u16,
    pub stoch_rsi_period: u16,
    pub stoch_rsi_k_smooth: u16,
    pub stoch_rsi_d_smooth: u16,
    pub stoch_rsi_k_color_rgb: [u8; 3],
    pub stoch_rsi_d_color_rgb: [u8; 3],

    pub dmi_period: u16,
    pub dmi_plus_color_rgb: [u8; 3],
    pub dmi_minus_color_rgb: [u8; 3],
    pub adx_color_rgb: [u8; 3],
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rsi_period: 14,
            rsi_color_rgb: [0xB0, 0xB0, 0xB0],
            macd_fast: 12,
            macd_slow: 26,
            macd_signal: 9,
            macd_color_rgb: [0x4C, 0xA3, 0xFF],
            macd_signal_color_rgb: [0xFF, 0xC1, 0x4C],

            atr_period: 14,
            atr_color_rgb: [0xB0, 0xB0, 0xB0],

            stoch_rsi_rsi_period: 14,
            stoch_rsi_period: 14,
            stoch_rsi_k_smooth: 3,
            stoch_rsi_d_smooth: 3,
            stoch_rsi_k_color_rgb: [0x4C, 0xA3, 0xFF],
            stoch_rsi_d_color_rgb: [0xFF, 0xC1, 0x4C],

            dmi_period: 14,
            dmi_plus_color_rgb: [0x6E, 0xE7, 0xB7],
            dmi_minus_color_rgb: [0xFF, 0x9A, 0x9A],
            adx_color_rgb: [0xB0, 0xB0, 0xB0],
        }
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub enum ClusterScaling {
    #[default]
    /// Scale based on the maximum quantity in the visible range.
    VisibleRange,
    /// Blend global VisibleRange and per-cluster Individual using a weight in [0.0, 1.0].
    /// weight = fraction of global contribution (1.0 == all-global, 0.0 == all-individual).
    Hybrid { weight: f32 },
    /// Scale based only on the maximum quantity inside the datapoint (per-candle).
    Datapoint,
}

impl ClusterScaling {
    pub const ALL: [ClusterScaling; 3] = [
        ClusterScaling::VisibleRange,
        ClusterScaling::Hybrid { weight: 0.2 },
        ClusterScaling::Datapoint,
    ];
}

impl std::fmt::Display for ClusterScaling {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClusterScaling::VisibleRange => write!(f, "Visible Range"),
            ClusterScaling::Hybrid { weight } => write!(f, "Hybrid (weight: {:.2})", weight),
            ClusterScaling::Datapoint => write!(f, "Per-candle"),
        }
    }
}

impl std::cmp::Eq for ClusterScaling {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum FootprintStudy {
    NPoC {
        lookback: usize,
    },
    Imbalance {
        threshold: usize,
        color_scale: Option<usize>,
        ignore_zeros: bool,
    },
}

impl FootprintStudy {
    pub fn is_same_type(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (FootprintStudy::NPoC { .. }, FootprintStudy::NPoC { .. })
                | (
                    FootprintStudy::Imbalance { .. },
                    FootprintStudy::Imbalance { .. }
                )
        )
    }
}

impl FootprintStudy {
    pub const ALL: [FootprintStudy; 2] = [
        FootprintStudy::NPoC { lookback: 80 },
        FootprintStudy::Imbalance {
            threshold: 200,
            color_scale: Some(400),
            ignore_zeros: true,
        },
    ];
}

impl std::fmt::Display for FootprintStudy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FootprintStudy::NPoC { .. } => write!(f, "Naked Point of Control"),
            FootprintStudy::Imbalance { .. } => write!(f, "Imbalance"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PointOfControl {
    pub price: Price,
    pub volume: f32,
    pub status: NPoc,
}

impl Default for PointOfControl {
    fn default() -> Self {
        Self {
            price: Price::from_f32(0.0),
            volume: 0.0,
            status: NPoc::default(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NPoc {
    #[default]
    None,
    Naked,
    Filled {
        at: u64,
    },
}

impl NPoc {
    pub fn filled(&mut self, at: u64) {
        *self = NPoc::Filled { at };
    }

    pub fn unfilled(&mut self) {
        *self = NPoc::Naked;
    }
}
