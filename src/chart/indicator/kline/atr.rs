use crate::chart::{
    Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::KlineIndicatorImpl,
        plot::{PlotTooltip, line::LinePlot},
    },
};

use data::chart::{PlotData, kline::KlineDataPoint};
use exchange::{Kline, Trade};
use iced::widget::{column, text};

use std::{collections::BTreeMap, ops::RangeInclusive};

pub struct AtrIndicator {
    cache: Caches,
    data: BTreeMap<u64, f32>,
    period: u16,
    color_rgb: [u8; 3],
}

impl AtrIndicator {
    pub fn new() -> Self {
        let cfg = data::chart::kline::Config::default();
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            period: cfg.atr_period,
            color_rgb: cfg.atr_color_rgb,
        }
    }

    fn rebuild(&mut self, source: &PlotData<KlineDataPoint>) {
        self.data = calc_atr_series(source, self.period);
        self.clear_all_caches();
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let tooltip = |value: &f32, _next: Option<&f32>| PlotTooltip::new(format!("ATR: {value:.4}"));
        let color = iced::Color::from_rgb8(self.color_rgb[0], self.color_rgb[1], self.color_rgb[2]);

        let plot = LinePlot::new(|v: &f32| *v)
            .color(color)
            .stroke_width(1.0)
            .show_points(false)
            .padding(0.08)
            .with_tooltip(tooltip);

        column![
            text(format!("ATR ({})", self.period)).size(12),
            indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
        ]
        .spacing(4)
        .into()
    }
}

impl KlineIndicatorImpl for AtrIndicator {
    fn clear_all_caches(&mut self) {
        self.cache.clear_all();
    }

    fn clear_crosshair_caches(&mut self) {
        self.cache.clear_crosshair();
    }

    fn element<'a>(
        &'a self,
        chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        self.indicator_elem(chart, visible_range)
    }

    fn rebuild_from_source(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild(source);
    }

    fn on_insert_klines(&mut self, _klines: &[Kline]) {
        self.cache.clear_all();
    }

    fn on_insert_trades(&mut self, _trades: &[Trade], _old_dp_len: usize, _source: &PlotData<KlineDataPoint>) {
        self.cache.clear_all();
    }

    fn on_ticksize_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild(source);
    }

    fn on_basis_change(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild(source);
    }

    fn on_source_updated(&mut self, source: &PlotData<KlineDataPoint>) {
        self.rebuild(source);
    }

    fn on_kline_visual_config_changed(
        &mut self,
        cfg: &data::chart::kline::Config,
        source: &PlotData<KlineDataPoint>,
    ) {
        let new_period = cfg.atr_period.max(2);
        let new_color = cfg.atr_color_rgb;
        if self.period != new_period || self.color_rgb != new_color {
            self.period = new_period;
            self.color_rgb = new_color;
            self.rebuild(source);
        }
    }
}

fn calc_atr_series(source: &PlotData<KlineDataPoint>, period: u16) -> BTreeMap<u64, f32> {
    let period = period.max(2) as usize;

    let ohlc: Vec<(u64, f32, f32, f32)> = match source {
        PlotData::TimeBased(ts) => ts
            .datapoints
            .iter()
            .map(|(t, dp)| {
                let k = &dp.kline;
                (*t, k.high.to_f32_lossy(), k.low.to_f32_lossy(), k.close.to_f32_lossy())
            })
            .collect(),
        PlotData::TickBased(tick) => tick
            .datapoints
            .iter()
            .rev()
            .enumerate()
            .map(|(idx, dp)| {
                let k = &dp.kline;
                (idx as u64, k.high.to_f32_lossy(), k.low.to_f32_lossy(), k.close.to_f32_lossy())
            })
            .collect(),
    };

    let mut out = BTreeMap::new();
    if ohlc.len() < period + 1 {
        return out;
    }

    // True Range series
    let mut trs = Vec::with_capacity(ohlc.len());
    trs.push(ohlc[0].1 - ohlc[0].2); // first candle: high-low
    for i in 1..ohlc.len() {
        let (_t, high, low, _close) = ohlc[i];
        let prev_close = ohlc[i - 1].3;
        let tr = (high - low)
            .max((high - prev_close).abs())
            .max((low - prev_close).abs());
        trs.push(tr);
    }

    // Wilder's ATR
    let mut atr = 0.0f32;
    for i in 0..period {
        atr += trs[i];
    }
    atr /= period as f32;
    out.insert(ohlc[period - 1].0, atr);

    for i in period..trs.len() {
        atr = (atr * (period as f32 - 1.0) + trs[i]) / period as f32;
        out.insert(ohlc[i].0, atr);
    }

    out
}


