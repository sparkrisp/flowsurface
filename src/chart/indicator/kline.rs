use crate::chart::{Message, ViewState};

use data::chart::PlotData;
use data::chart::indicator::KlineIndicator;
use data::chart::kline::KlineDataPoint;
use exchange::fetcher::FetchRange;
use exchange::{Kline, Timeframe, Trade};

pub mod open_interest;
pub mod rsi;
pub mod volume;
pub mod macd;
pub mod atr;
pub mod stoch_rsi;
pub mod dmi_adx;

pub trait KlineIndicatorImpl {
    /// Clear all caches for a full redraw
    fn clear_all_caches(&mut self);

    /// Clear caches related to crosshair only
    /// e.g. tooltips and scale labels for a partial redraw
    fn clear_crosshair_caches(&mut self);

    fn element<'a>(
        &'a self,
        chart: &'a ViewState,
        visible_range: std::ops::RangeInclusive<u64>,
    ) -> iced::Element<'a, Message>;

    /// If the indicator needs data fetching, return the required range
    fn fetch_range(&mut self, _ctx: &FetchCtx) -> Option<FetchRange> {
        None
    }

    /// Rebuild data using kline(OHLCV) source
    fn rebuild_from_source(&mut self, _source: &PlotData<KlineDataPoint>) {}

    fn on_insert_klines(&mut self, _klines: &[Kline]) {}

    fn on_insert_trades(
        &mut self,
        _trades: &[Trade],
        _old_dp_len: usize,
        _source: &PlotData<KlineDataPoint>,
    ) {
    }

    fn on_ticksize_change(&mut self, _source: &PlotData<KlineDataPoint>) {}

    /// Timeframe/tick interval has changed
    fn on_basis_change(&mut self, _source: &PlotData<KlineDataPoint>) {}

    fn on_open_interest(&mut self, _pairs: &[exchange::OpenInterest]) {}

    /// Called after the chart's underlying data source has been updated (e.g. new klines/trades).
    /// Useful for indicators that are easiest to recompute in batch (MVP).
    fn on_source_updated(&mut self, _source: &PlotData<KlineDataPoint>) {}

    fn on_kline_visual_config_changed(
        &mut self,
        _cfg: &data::chart::kline::Config,
        _source: &PlotData<KlineDataPoint>,
    ) {
    }
}

pub struct FetchCtx<'a> {
    pub main_chart: &'a ViewState,
    pub timeframe: Timeframe,
    pub visible_earliest: u64,
    pub kline_latest: u64,
    pub prefetch_earliest: u64,
}

pub fn make_empty(which: KlineIndicator) -> Box<dyn KlineIndicatorImpl> {
    match which {
        KlineIndicator::Volume => Box::new(super::kline::volume::VolumeIndicator::new()),
        KlineIndicator::OpenInterest => {
            Box::new(super::kline::open_interest::OpenInterestIndicator::new())
        }
        KlineIndicator::Rsi => Box::new(super::kline::rsi::RsiIndicator::new()),
        KlineIndicator::Macd => Box::new(super::kline::macd::MacdIndicator::new()),
        KlineIndicator::Atr => Box::new(super::kline::atr::AtrIndicator::new()),
        KlineIndicator::StochRsi => Box::new(super::kline::stoch_rsi::StochRsiIndicator::new()),
        KlineIndicator::DmiAdx => Box::new(super::kline::dmi_adx::DmiAdxIndicator::new()),
    }
}
