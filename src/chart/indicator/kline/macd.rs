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
pub struct MacdPoint {
    macd: f32,
    signal: f32,
}

pub struct MacdIndicator {
    cache: Caches,
    data: BTreeMap<u64, MacdPoint>,
    fast: u16,
    slow: u16,
    signal: u16,
    macd_color_rgb: [u8; 3],
    signal_color_rgb: [u8; 3],
}

impl MacdIndicator {
    pub fn new() -> Self {
        let cfg = data::chart::kline::Config::default();
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            fast: cfg.macd_fast,
            slow: cfg.macd_slow,
            signal: cfg.macd_signal,
            macd_color_rgb: cfg.macd_color_rgb,
            signal_color_rgb: cfg.macd_signal_color_rgb,
        }
    }

    fn rebuild(&mut self, source: &PlotData<KlineDataPoint>) {
        self.data = calc_macd_series(source, self.fast, self.slow, self.signal);
        self.clear_all_caches();
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let macd_c = iced::Color::from_rgb8(
            self.macd_color_rgb[0],
            self.macd_color_rgb[1],
            self.macd_color_rgb[2],
        );
        let sig_c = iced::Color::from_rgb8(
            self.signal_color_rgb[0],
            self.signal_color_rgb[1],
            self.signal_color_rgb[2],
        );
        let plot = MacdPlot::new(macd_c, sig_c).with_tooltip(|p: &MacdPoint, _next| {
            PlotTooltip::new(format!("MACD: {:.4}\nSignal: {:.4}", p.macd, p.signal))
        });

        column![
            text(format!("MACD ({},{},{})", self.fast, self.slow, self.signal)).size(12),
            indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
        ]
        .spacing(4)
        .into()
    }
}

impl KlineIndicatorImpl for MacdIndicator {
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
        let new_fast = cfg.macd_fast.max(2);
        let new_slow = cfg.macd_slow.max(3);
        let new_signal = cfg.macd_signal.max(2);
        let new_macd_color = cfg.macd_color_rgb;
        let new_sig_color = cfg.macd_signal_color_rgb;
        if self.fast != new_fast
            || self.slow != new_slow
            || self.signal != new_signal
            || self.macd_color_rgb != new_macd_color
            || self.signal_color_rgb != new_sig_color
        {
            self.fast = new_fast;
            self.slow = new_slow;
            self.signal = new_signal;
            self.macd_color_rgb = new_macd_color;
            self.signal_color_rgb = new_sig_color;
            self.rebuild(source);
        }
    }
}

struct MacdPlot {
    tooltip: Option<TooltipFn<MacdPoint>>,
    macd_color: iced::Color,
    signal_color: iced::Color,
}

impl MacdPlot {
    fn new(macd_color: iced::Color, signal_color: iced::Color) -> Self {
        Self {
            tooltip: None,
            macd_color,
            signal_color,
        }
    }

    fn with_tooltip<F>(mut self, tooltip: F) -> Self
    where
        F: Fn(&MacdPoint, Option<&MacdPoint>) -> PlotTooltip + 'static,
    {
        self.tooltip = Some(Box::new(tooltip));
        self
    }
}

impl<S> Plot<S> for MacdPlot
where
    S: Series<Y = MacdPoint>,
{
    fn y_extents(&self, datapoints: &S, range: RangeInclusive<u64>) -> Option<(f32, f32)> {
        let mut min_v = f32::MAX;
        let mut max_v = f32::MIN;
        datapoints.for_each_in(range, |_, p| {
            min_v = min_v.min(p.macd).min(p.signal);
            max_v = max_v.max(p.macd).max(p.signal);
        });
        if min_v == f32::MAX {
            None
        } else {
            Some((min_v, max_v))
        }
    }

    fn adjust_extents(&self, min: f32, max: f32) -> (f32, f32) {
        if max > min {
            let pad = (max - min) * 0.12;
            (min - pad, max + pad)
        } else {
            (min - 1.0, max + 1.0)
        }
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
        let macd_stroke = canvas::Stroke::default()
            .with_color(self.macd_color)
            .with_width(1.0);
        let signal_stroke = canvas::Stroke::default()
            .with_color(self.signal_color)
            .with_width(1.0);

        let mut prev_macd: Option<(f32, f32)> = None;
        let mut prev_signal: Option<(f32, f32)> = None;

        datapoints.for_each_in(range.clone(), |x, p| {
            let sx = ctx.interval_to_x(x) - (ctx.cell_width / 2.0);
            let sy_macd = scale.to_y(p.macd);
            let sy_signal = scale.to_y(p.signal);

            if let Some((px, py)) = prev_macd {
                frame.stroke(&canvas::Path::line(iced::Point::new(px, py), iced::Point::new(sx, sy_macd)), macd_stroke);
            }
            if let Some((px, py)) = prev_signal {
                frame.stroke(&canvas::Path::line(iced::Point::new(px, py), iced::Point::new(sx, sy_signal)), signal_stroke);
            }
            prev_macd = Some((sx, sy_macd));
            prev_signal = Some((sx, sy_signal));
        });
    }

    fn tooltip_fn(&self) -> Option<&TooltipFn<MacdPoint>> {
        self.tooltip.as_ref()
    }
}

fn ema(values: &[f32], period: usize) -> Vec<f32> {
    if values.is_empty() {
        return vec![];
    }
    let period = period.max(1);
    let k = 2.0 / (period as f32 + 1.0);
    let mut out = Vec::with_capacity(values.len());
    let mut prev = values[0];
    out.push(prev);
    for &v in values.iter().skip(1) {
        prev = v * k + prev * (1.0 - k);
        out.push(prev);
    }
    out
}

fn calc_macd_series(
    source: &PlotData<KlineDataPoint>,
    fast: u16,
    slow: u16,
    signal: u16,
) -> BTreeMap<u64, MacdPoint> {
    let fast = fast.max(2) as usize;
    let slow = slow.max(3) as usize;
    let signal = signal.max(2) as usize;

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
    if closes.is_empty() {
        return out;
    }

    let values: Vec<f32> = closes.iter().map(|(_, c)| *c).collect();
    let ema_fast = ema(&values, fast);
    let ema_slow = ema(&values, slow);
    let mut macd_line = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        macd_line.push(ema_fast[i] - ema_slow[i]);
    }
    let signal_line = ema(&macd_line, signal);

    for i in 0..closes.len() {
        out.insert(
            closes[i].0,
            MacdPoint {
                macd: macd_line[i],
                signal: signal_line[i],
            },
        );
    }

    out
}


