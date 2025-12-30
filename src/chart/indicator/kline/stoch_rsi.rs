use crate::chart::{
    Caches, Message, ViewState,
    indicator::{
        indicator_row,
        kline::KlineIndicatorImpl,
        plot::{Plot, PlotTooltip, Series, TooltipFn, YScale},
    },
};

use data::chart::{PlotData, kline::KlineDataPoint};
use exchange::{Kline, Trade};
use iced::{Theme, widget::{canvas, column, text}};

use std::{collections::BTreeMap, ops::RangeInclusive};

#[derive(Debug, Clone, Copy)]
pub struct StochRsiPoint {
    pub k: f32,
    pub d: f32,
}

pub struct StochRsiIndicator {
    cache: Caches,
    data: BTreeMap<u64, StochRsiPoint>,
    rsi_period: u16,
    stoch_period: u16,
    k_smooth: u16,
    d_smooth: u16,
    k_color_rgb: [u8; 3],
    d_color_rgb: [u8; 3],
}

impl StochRsiIndicator {
    pub fn new() -> Self {
        let cfg = data::chart::kline::Config::default();
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            rsi_period: cfg.stoch_rsi_rsi_period,
            stoch_period: cfg.stoch_rsi_period,
            k_smooth: cfg.stoch_rsi_k_smooth,
            d_smooth: cfg.stoch_rsi_d_smooth,
            k_color_rgb: cfg.stoch_rsi_k_color_rgb,
            d_color_rgb: cfg.stoch_rsi_d_color_rgb,
        }
    }

    fn rebuild(&mut self, source: &PlotData<KlineDataPoint>) {
        self.data = calc_stoch_rsi_series(
            source,
            self.rsi_period,
            self.stoch_period,
            self.k_smooth,
            self.d_smooth,
        );
        self.clear_all_caches();
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let k_c = iced::Color::from_rgb8(self.k_color_rgb[0], self.k_color_rgb[1], self.k_color_rgb[2]);
        let d_c = iced::Color::from_rgb8(self.d_color_rgb[0], self.d_color_rgb[1], self.d_color_rgb[2]);

        let plot = StochRsiPlot::new(k_c, d_c).with_tooltip(|p: &StochRsiPoint, _next| {
            PlotTooltip::new(format!("K: {:.2}\nD: {:.2}", p.k, p.d))
        });

        column![
            text(format!(
                "Stoch RSI (rsi {}, stoch {}, k {}, d {})",
                self.rsi_period, self.stoch_period, self.k_smooth, self.d_smooth
            ))
            .size(12),
            indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
        ]
        .spacing(4)
        .into()
    }
}

impl KlineIndicatorImpl for StochRsiIndicator {
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
        let new_rsi = cfg.stoch_rsi_rsi_period.max(2);
        let new_stoch = cfg.stoch_rsi_period.max(2);
        let new_k = cfg.stoch_rsi_k_smooth.max(1);
        let new_d = cfg.stoch_rsi_d_smooth.max(1);
        let new_kc = cfg.stoch_rsi_k_color_rgb;
        let new_dc = cfg.stoch_rsi_d_color_rgb;

        if self.rsi_period != new_rsi
            || self.stoch_period != new_stoch
            || self.k_smooth != new_k
            || self.d_smooth != new_d
            || self.k_color_rgb != new_kc
            || self.d_color_rgb != new_dc
        {
            self.rsi_period = new_rsi;
            self.stoch_period = new_stoch;
            self.k_smooth = new_k;
            self.d_smooth = new_d;
            self.k_color_rgb = new_kc;
            self.d_color_rgb = new_dc;
            self.rebuild(source);
        }
    }
}

struct StochRsiPlot {
    tooltip: Option<TooltipFn<StochRsiPoint>>,
    k_color: iced::Color,
    d_color: iced::Color,
}

impl StochRsiPlot {
    fn new(k_color: iced::Color, d_color: iced::Color) -> Self {
        Self {
            tooltip: None,
            k_color,
            d_color,
        }
    }

    fn with_tooltip<F>(mut self, tooltip: F) -> Self
    where
        F: Fn(&StochRsiPoint, Option<&StochRsiPoint>) -> PlotTooltip + 'static,
    {
        self.tooltip = Some(Box::new(tooltip));
        self
    }
}

impl<S> Plot<S> for StochRsiPlot
where
    S: Series<Y = StochRsiPoint>,
{
    fn y_extents(&self, datapoints: &S, range: RangeInclusive<u64>) -> Option<(f32, f32)> {
        let mut min_v = f32::MAX;
        let mut max_v = f32::MIN;
        datapoints.for_each_in(range, |_, p| {
            min_v = min_v.min(p.k).min(p.d);
            max_v = max_v.max(p.k).max(p.d);
        });
        if min_v == f32::MAX {
            None
        } else {
            Some((min_v, max_v))
        }
    }

    fn adjust_extents(&self, min: f32, max: f32) -> (f32, f32) {
        // keep the classic 0..100 feel when possible
        let min = min.min(0.0);
        let max = max.max(100.0);
        (min, max)
    }

    fn draw(
        &self,
        frame: &mut canvas::Frame,
        ctx: &ViewState,
        theme: &Theme,
        datapoints: &S,
        range: RangeInclusive<u64>,
        scale: &YScale,
    ) {
        let _palette = theme.extended_palette();
        let k_stroke = canvas::Stroke::default().with_color(self.k_color).with_width(1.0);
        let d_stroke = canvas::Stroke::default().with_color(self.d_color).with_width(1.0);

        let mut prev_k: Option<(f32, f32)> = None;
        let mut prev_d: Option<(f32, f32)> = None;

        datapoints.for_each_in(range, |x, p| {
            let sx = ctx.interval_to_x(x) - (ctx.cell_width / 2.0);
            let sy_k = scale.to_y(p.k);
            let sy_d = scale.to_y(p.d);

            if let Some((px, py)) = prev_k {
                frame.stroke(&canvas::Path::line(iced::Point::new(px, py), iced::Point::new(sx, sy_k)), k_stroke);
            }
            if let Some((px, py)) = prev_d {
                frame.stroke(&canvas::Path::line(iced::Point::new(px, py), iced::Point::new(sx, sy_d)), d_stroke);
            }
            prev_k = Some((sx, sy_k));
            prev_d = Some((sx, sy_d));
        });
    }

    fn tooltip_fn(&self) -> Option<&TooltipFn<StochRsiPoint>> {
        self.tooltip.as_ref()
    }
}

fn calc_rsi_raw(closes: &[f32], period: usize) -> Vec<Option<f32>> {
    let mut out = vec![None; closes.len()];
    if closes.len() < period + 1 {
        return out;
    }
    let mut gains = 0.0f32;
    let mut losses = 0.0f32;
    for i in 1..=period {
        let diff = closes[i] - closes[i - 1];
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
    out[period] = Some(rsi_at(avg_gain, avg_loss));
    for i in (period + 1)..closes.len() {
        let diff = closes[i] - closes[i - 1];
        let gain = if diff > 0.0 { diff } else { 0.0 };
        let loss = if diff < 0.0 { -diff } else { 0.0 };
        avg_gain = (avg_gain * (period as f32 - 1.0) + gain) / period as f32;
        avg_loss = (avg_loss * (period as f32 - 1.0) + loss) / period as f32;
        out[i] = Some(rsi_at(avg_gain, avg_loss));
    }
    out
}

fn sma_opt(values: &[Option<f32>], period: usize) -> Vec<Option<f32>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() < period {
        return out;
    }
    let mut sum = 0.0f32;
    let mut count = 0usize;
    let mut window: std::collections::VecDeque<Option<f32>> = std::collections::VecDeque::new();
    for i in 0..values.len() {
        let v = values[i];
        window.push_back(v);
        if let Some(x) = v {
            sum += x;
            count += 1;
        }
        if window.len() > period {
            if let Some(old) = window.pop_front().flatten() {
                sum -= old;
                count -= 1;
            }
        }
        if window.len() == period && count == period {
            out[i] = Some(sum / period as f32);
        }
    }
    out
}

fn calc_stoch_rsi_series(
    source: &PlotData<KlineDataPoint>,
    rsi_period: u16,
    stoch_period: u16,
    k_smooth: u16,
    d_smooth: u16,
) -> BTreeMap<u64, StochRsiPoint> {
    let rsi_period = rsi_period.max(2) as usize;
    let stoch_period = stoch_period.max(2) as usize;
    let k_smooth = k_smooth.max(1) as usize;
    let d_smooth = d_smooth.max(1) as usize;

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
    if closes.len() < rsi_period + stoch_period + 2 {
        return out;
    }

    let xs: Vec<u64> = closes.iter().map(|(x, _)| *x).collect();
    let vals: Vec<f32> = closes.iter().map(|(_, c)| *c).collect();
    let rsi = calc_rsi_raw(&vals, rsi_period);

    // raw stoch rsi K (0..100)
    let mut k_raw: Vec<Option<f32>> = vec![None; rsi.len()];
    for i in 0..rsi.len() {
        if i + 1 < stoch_period {
            continue;
        }
        let start = i + 1 - stoch_period;
        let window = &rsi[start..=i];
        if window.iter().any(|v| v.is_none()) {
            continue;
        }
        let mut min_v = f32::MAX;
        let mut max_v = f32::MIN;
        for v in window.iter().flatten() {
            min_v = min_v.min(*v);
            max_v = max_v.max(*v);
        }
        let cur = rsi[i].unwrap();
        let denom = (max_v - min_v).max(1e-6);
        k_raw[i] = Some(((cur - min_v) / denom) * 100.0);
    }

    let k_sm = sma_opt(&k_raw, k_smooth);
    let d_sm = sma_opt(&k_sm, d_smooth);

    for i in 0..xs.len() {
        if let (Some(k), Some(d)) = (k_sm[i], d_sm[i]) {
            out.insert(xs[i], StochRsiPoint { k, d });
        }
    }
    out
}


