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

pub struct RsiIndicator {
    cache: Caches,
    data: BTreeMap<u64, f32>,
    period: u16,
    color_rgb: [u8; 3],
}

impl RsiIndicator {
    pub fn new() -> Self {
        let cfg = data::chart::kline::Config::default();
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            period: cfg.rsi_period,
            color_rgb: cfg.rsi_color_rgb,
        }
    }

    fn rebuild(&mut self, source: &PlotData<KlineDataPoint>) {
        self.data = calc_rsi_series(source, self.period);
        self.clear_all_caches();
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let tooltip = |value: &f32, _next: Option<&f32>| PlotTooltip::new(format!("RSI: {value:.2}"));
        let color = iced::Color::from_rgb8(self.color_rgb[0], self.color_rgb[1], self.color_rgb[2]);
        let plot = LinePlot::new(|v: &f32| *v)
            .color(color)
            .stroke_width(1.0)
            .show_points(false)
            .padding(0.06)
            .with_tooltip(tooltip);

        column![
            text(format!("RSI ({})", self.period)).size(12),
            indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
        ]
        .spacing(4)
        .into()
    }
}

impl KlineIndicatorImpl for RsiIndicator {
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
        // cheaper to rebuild in batch than incremental RSI bookkeeping (MVP)
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
        let new_period = cfg.rsi_period.max(2);
        let new_color = cfg.rsi_color_rgb;
        if self.period != new_period || self.color_rgb != new_color {
            self.period = new_period;
            self.color_rgb = new_color;
            self.rebuild(source);
        }
    }
}

fn calc_rsi_series(source: &PlotData<KlineDataPoint>, period: u16) -> BTreeMap<u64, f32> {
    let period = period.max(2) as usize;
    let closes: Vec<(u64, f32)> = match source {
        PlotData::TimeBased(ts) => ts
            .datapoints
            .iter()
            .map(|(t, dp)| (*t, dp.kline.close.to_f32_lossy()))
            .collect(),
        PlotData::TickBased(tick) => tick
            .datapoints
            .iter()
            .rev()
            .enumerate()
            .map(|(idx, dp)| (idx as u64, dp.kline.close.to_f32_lossy()))
            .collect(),
    };

    let mut out = BTreeMap::new();
    if closes.len() < period + 1 {
        return out;
    }

    // Wilder's RSI
    let mut gains = 0.0f32;
    let mut losses = 0.0f32;
    for i in 1..=period {
        let diff = closes[i].1 - closes[i - 1].1;
        if diff >= 0.0 {
            gains += diff;
        } else {
            losses += -diff;
        }
    }
    let mut avg_gain = gains / period as f32;
    let mut avg_loss = losses / period as f32;

    let rsi_at = |avg_gain: f32, avg_loss: f32| -> f32 {
        if avg_loss == 0.0 {
            100.0
        } else {
            let rs = avg_gain / avg_loss;
            100.0 - (100.0 / (1.0 + rs))
        }
    };

    out.insert(closes[period].0, rsi_at(avg_gain, avg_loss));

    for i in (period + 1)..closes.len() {
        let diff = closes[i].1 - closes[i - 1].1;
        let gain = if diff > 0.0 { diff } else { 0.0 };
        let loss = if diff < 0.0 { -diff } else { 0.0 };
        avg_gain = (avg_gain * (period as f32 - 1.0) + gain) / period as f32;
        avg_loss = (avg_loss * (period as f32 - 1.0) + loss) / period as f32;
        out.insert(closes[i].0, rsi_at(avg_gain, avg_loss));
    }

    out
}


