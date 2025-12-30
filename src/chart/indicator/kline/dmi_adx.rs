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
pub struct DmiPoint {
    pub plus_di: f32,
    pub minus_di: f32,
    pub adx: f32,
}

pub struct DmiAdxIndicator {
    cache: Caches,
    data: BTreeMap<u64, DmiPoint>,
    period: u16,
    plus_color_rgb: [u8; 3],
    minus_color_rgb: [u8; 3],
    adx_color_rgb: [u8; 3],
}

impl DmiAdxIndicator {
    pub fn new() -> Self {
        let cfg = data::chart::kline::Config::default();
        Self {
            cache: Caches::default(),
            data: BTreeMap::new(),
            period: cfg.dmi_period,
            plus_color_rgb: cfg.dmi_plus_color_rgb,
            minus_color_rgb: cfg.dmi_minus_color_rgb,
            adx_color_rgb: cfg.adx_color_rgb,
        }
    }

    fn rebuild(&mut self, source: &PlotData<KlineDataPoint>) {
        self.data = calc_dmi_adx_series(source, self.period);
        self.clear_all_caches();
    }

    fn indicator_elem<'a>(
        &'a self,
        main_chart: &'a ViewState,
        visible_range: RangeInclusive<u64>,
    ) -> iced::Element<'a, Message> {
        let plus_c = iced::Color::from_rgb8(self.plus_color_rgb[0], self.plus_color_rgb[1], self.plus_color_rgb[2]);
        let minus_c = iced::Color::from_rgb8(self.minus_color_rgb[0], self.minus_color_rgb[1], self.minus_color_rgb[2]);
        let adx_c = iced::Color::from_rgb8(self.adx_color_rgb[0], self.adx_color_rgb[1], self.adx_color_rgb[2]);

        let plot = DmiPlot::new(plus_c, minus_c, adx_c).with_tooltip(|p: &DmiPoint, _next| {
            PlotTooltip::new(format!("+DI: {:.2}\n-DI: {:.2}\nADX: {:.2}", p.plus_di, p.minus_di, p.adx))
        });

        column![
            text(format!("DMI / ADX ({})", self.period)).size(12),
            indicator_row(main_chart, &self.cache, plot, &self.data, visible_range)
        ]
        .spacing(4)
        .into()
    }
}

impl KlineIndicatorImpl for DmiAdxIndicator {
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
        let new_p = cfg.dmi_period.max(2);
        let pc = cfg.dmi_plus_color_rgb;
        let mc = cfg.dmi_minus_color_rgb;
        let ac = cfg.adx_color_rgb;
        if self.period != new_p || self.plus_color_rgb != pc || self.minus_color_rgb != mc || self.adx_color_rgb != ac {
            self.period = new_p;
            self.plus_color_rgb = pc;
            self.minus_color_rgb = mc;
            self.adx_color_rgb = ac;
            self.rebuild(source);
        }
    }
}

struct DmiPlot {
    tooltip: Option<TooltipFn<DmiPoint>>,
    plus_color: iced::Color,
    minus_color: iced::Color,
    adx_color: iced::Color,
}

impl DmiPlot {
    fn new(plus_color: iced::Color, minus_color: iced::Color, adx_color: iced::Color) -> Self {
        Self {
            tooltip: None,
            plus_color,
            minus_color,
            adx_color,
        }
    }

    fn with_tooltip<F>(mut self, tooltip: F) -> Self
    where
        F: Fn(&DmiPoint, Option<&DmiPoint>) -> PlotTooltip + 'static,
    {
        self.tooltip = Some(Box::new(tooltip));
        self
    }
}

impl<S> Plot<S> for DmiPlot
where
    S: Series<Y = DmiPoint>,
{
    fn y_extents(&self, datapoints: &S, range: RangeInclusive<u64>) -> Option<(f32, f32)> {
        let mut min_v = f32::MAX;
        let mut max_v = f32::MIN;
        datapoints.for_each_in(range, |_, p| {
            min_v = min_v.min(p.plus_di).min(p.minus_di).min(p.adx);
            max_v = max_v.max(p.plus_di).max(p.minus_di).max(p.adx);
        });
        if min_v == f32::MAX {
            None
        } else {
            Some((min_v, max_v))
        }
    }

    fn adjust_extents(&self, min: f32, max: f32) -> (f32, f32) {
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
        let p_stroke = canvas::Stroke::default().with_color(self.plus_color).with_width(1.0);
        let m_stroke = canvas::Stroke::default().with_color(self.minus_color).with_width(1.0);
        let a_stroke = canvas::Stroke::default().with_color(self.adx_color).with_width(1.0);

        let mut prev_p: Option<(f32, f32)> = None;
        let mut prev_m: Option<(f32, f32)> = None;
        let mut prev_a: Option<(f32, f32)> = None;

        datapoints.for_each_in(range, |x, p| {
            let sx = ctx.interval_to_x(x) - (ctx.cell_width / 2.0);
            let sp = scale.to_y(p.plus_di);
            let sm = scale.to_y(p.minus_di);
            let sa = scale.to_y(p.adx);

            if let Some((px, py)) = prev_p {
                frame.stroke(&canvas::Path::line(iced::Point::new(px, py), iced::Point::new(sx, sp)), p_stroke);
            }
            if let Some((px, py)) = prev_m {
                frame.stroke(&canvas::Path::line(iced::Point::new(px, py), iced::Point::new(sx, sm)), m_stroke);
            }
            if let Some((px, py)) = prev_a {
                frame.stroke(&canvas::Path::line(iced::Point::new(px, py), iced::Point::new(sx, sa)), a_stroke);
            }
            prev_p = Some((sx, sp));
            prev_m = Some((sx, sm));
            prev_a = Some((sx, sa));
        });
    }

    fn tooltip_fn(&self) -> Option<&TooltipFn<DmiPoint>> {
        self.tooltip.as_ref()
    }
}

fn calc_dmi_adx_series(source: &PlotData<KlineDataPoint>, period: u16) -> BTreeMap<u64, DmiPoint> {
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
    if ohlc.len() < period + 2 {
        return out;
    }

    // Wilder smoothing initialization
    let mut tr14 = 0.0f32;
    let mut plus_dm14 = 0.0f32;
    let mut minus_dm14 = 0.0f32;

    // First TR/DM sums over first `period`
    for i in 1..=period {
        let (_t, high, low, _close) = ohlc[i];
        let prev_high = ohlc[i - 1].1;
        let prev_low = ohlc[i - 1].2;
        let prev_close = ohlc[i - 1].3;

        let up_move = high - prev_high;
        let down_move = prev_low - low;

        let plus_dm = if up_move > down_move && up_move > 0.0 { up_move } else { 0.0 };
        let minus_dm = if down_move > up_move && down_move > 0.0 { down_move } else { 0.0 };

        let tr = (high - low)
            .max((high - prev_close).abs())
            .max((low - prev_close).abs());

        tr14 += tr;
        plus_dm14 += plus_dm;
        minus_dm14 += minus_dm;
    }

    let mut prev_adx: Option<f32> = None;
    let mut dxs: Vec<f32> = vec![];

    // Continue smoothing
    for i in (period + 1)..ohlc.len() {
        let (_t, high, low, _close) = ohlc[i];
        let prev_high = ohlc[i - 1].1;
        let prev_low = ohlc[i - 1].2;
        let prev_close = ohlc[i - 1].3;

        let up_move = high - prev_high;
        let down_move = prev_low - low;

        let plus_dm = if up_move > down_move && up_move > 0.0 { up_move } else { 0.0 };
        let minus_dm = if down_move > up_move && down_move > 0.0 { down_move } else { 0.0 };

        let tr = (high - low)
            .max((high - prev_close).abs())
            .max((low - prev_close).abs());

        tr14 = tr14 - (tr14 / period as f32) + tr;
        plus_dm14 = plus_dm14 - (plus_dm14 / period as f32) + plus_dm;
        minus_dm14 = minus_dm14 - (minus_dm14 / period as f32) + minus_dm;

        if tr14 <= 0.0 {
            continue;
        }

        let plus_di = 100.0 * (plus_dm14 / tr14);
        let minus_di = 100.0 * (minus_dm14 / tr14);
        let denom = (plus_di + minus_di).max(1e-6);
        let dx = 100.0 * ((plus_di - minus_di).abs() / denom);

        dxs.push(dx);

        let adx = if dxs.len() < period {
            // build initial ADX
            continue;
        } else if prev_adx.is_none() {
            let init = dxs.iter().take(period).sum::<f32>() / period as f32;
            prev_adx = Some(init);
            init
        } else {
            let prev = prev_adx.unwrap();
            let next = (prev * (period as f32 - 1.0) + dx) / period as f32;
            prev_adx = Some(next);
            next
        };

        out.insert(ohlc[i].0, DmiPoint { plus_di, minus_di, adx });
    }

    out
}


