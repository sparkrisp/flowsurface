pub mod pane;
pub mod panel;
pub mod sidebar;
pub mod tickers_table;

pub use sidebar::Sidebar;

use super::DashboardError;
use crate::{
    chart,
    modal::pane::{self as pane_modal, Modal as PaneModal},
    screen::dashboard::tickers_table::TickersTable,
    style,
    widget::resize_box::ResizeBox,
    widget::toast::{Notification, Toast},
    window::{self, Window},
};
use crate::audio::SoundType;
use data::{
    UserTimezone,
    layout::{WindowSpec, pane::ContentKind},
};
use data::rules::{RuleCondition, RuleSpec};
use exchange::{
    Kline, PushFrequency, StreamPairKind, TickMultiplier, TickerInfo, Timeframe, Trade,
    adapter::{
        self, AdapterError, Exchange, PersistStreamKind, ResolvedStream, StreamConfig, StreamKind,
        StreamTicksize, UniqueStreams, binance, bybit, hyperliquid, okex,
    },
    depth::Depth,
    fetcher::{FetchRange, FetchedData},
};

use iced::{
    Alignment, Element, Length, Subscription, Task, Vector, padding,
    task::{Straw, sipper},
    widget::{
        PaneGrid, center, container, mouse_area,
        pane_grid::{self, Configuration},
    },
};
use iced_futures::futures::TryFutureExt;
use std::{collections::HashMap, path::PathBuf, time::Instant, vec};

// === Background rule evaluation helpers (CPU thread via Task::perform) ===
#[derive(Clone)]
struct TickEvalSnapshot {
    pane_id: uuid::Uuid,
    rules: Vec<RuleSpec>,
    prev_price: Option<f32>,
    cur_price: Option<f32>,
    kind: Option<data::chart::KlineChartKind>,
    cfg: Option<data::chart::kline::Config>,
    closes: Option<Vec<f32>>,
    ohlcv: Option<Vec<(u64, f32, f32, f32, f32)>>,
}

fn ma_pair_from_kind(kind: &data::chart::KlineChartKind) -> Option<((data::chart::kline::MovingAverageKind, u16), (data::chart::kline::MovingAverageKind, u16))> {
    let data::chart::KlineChartKind::CandlesStudied { studies } = kind else {
        return None;
    };
    let mut fast = None;
    let mut slow = None;
    for s in studies {
        match *s {
            data::chart::kline::CandleStudy::MovingAverageFast { kind, period, .. } => {
                fast = Some((kind, period));
            }
            data::chart::kline::CandleStudy::MovingAverageSlow { kind, period, .. } => {
                slow = Some((kind, period));
            }
            _ => {}
        }
    }
    Some((fast?, slow?))
}

fn vwap_reset_note(kind: &data::chart::KlineChartKind) -> bool {
    let data::chart::KlineChartKind::CandlesStudied { studies } = kind else {
        return false;
    };
    for s in studies {
        if let data::chart::kline::CandleStudy::VwapBands { reset_daily_utc, .. } = *s {
            return reset_daily_utc;
        }
    }
    false
}

fn ma_last_two(closes: &[f32], kind: data::chart::kline::MovingAverageKind, period: u16) -> Option<(f32, f32)> {
    let p = (period as usize).max(2);
    if closes.len() < p + 1 {
        return None;
    }
    match kind {
        data::chart::kline::MovingAverageKind::SMA => {
            let cur = closes[closes.len() - p..].iter().sum::<f32>() / p as f32;
            let prev = closes[closes.len() - p - 1..closes.len() - 1]
                .iter()
                .sum::<f32>()
                / p as f32;
            Some((prev, cur))
        }
        data::chart::kline::MovingAverageKind::EMA => {
            let k = 2.0 / (p as f32 + 1.0);
            let start = closes.len().saturating_sub(p + 2);
            let slice = &closes[start..];
            let mut prev = slice[0];
            let mut out_prev = prev;
            for &v in slice.iter().skip(1) {
                let next = v * k + prev * (1.0 - k);
                out_prev = prev;
                prev = next;
            }
            Some((out_prev, prev))
        }
    }
}

fn rsi_last_two(closes: &[f32], period: u16) -> Option<(f32, f32)> {
    let p = (period as usize).max(2);
    if closes.len() < p + 2 {
        return None;
    }
    let start = closes.len().saturating_sub(p + 2);
    let slice = &closes[start..];
    let mut gains = 0.0f32;
    let mut losses = 0.0f32;
    for i in 1..=p {
        let diff = slice[i] - slice[i - 1];
        if diff >= 0.0 { gains += diff; } else { losses += -diff; }
    }
    let mut avg_gain = gains / p as f32;
    let mut avg_loss = losses / p as f32;
    let rsi = |ag: f32, al: f32| -> f32 {
        if al == 0.0 { 100.0 } else {
            let rs = ag / al;
            100.0 - (100.0 / (1.0 + rs))
        }
    };
    let mut prev_rsi = rsi(avg_gain, avg_loss);
    let mut cur_rsi = prev_rsi;
    for i in (p + 1)..slice.len() {
        let diff = slice[i] - slice[i - 1];
        let gain = if diff > 0.0 { diff } else { 0.0 };
        let loss = if diff < 0.0 { -diff } else { 0.0 };
        avg_gain = (avg_gain * (p as f32 - 1.0) + gain) / p as f32;
        avg_loss = (avg_loss * (p as f32 - 1.0) + loss) / p as f32;
        prev_rsi = cur_rsi;
        cur_rsi = rsi(avg_gain, avg_loss);
    }
    Some((prev_rsi, cur_rsi))
}

fn ema(values: &[f32], period: usize) -> Vec<f32> {
    if values.is_empty() { return vec![]; }
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

fn macd_last_two(closes: &[f32], fast: u16, slow: u16, signal: u16) -> Option<((f32, f32), (f32, f32))> {
    let fast = (fast as usize).max(2);
    let slow = (slow as usize).max(3);
    let signal = (signal as usize).max(2);
    if closes.len() < 3 { return None; }
    let start = closes.len().saturating_sub(slow + signal + 10);
    let slice = &closes[start..];
    let ema_fast = ema(slice, fast);
    let ema_slow = ema(slice, slow);
    let mut macd_line = Vec::with_capacity(slice.len());
    for i in 0..slice.len() { macd_line.push(ema_fast[i] - ema_slow[i]); }
    let signal_line = ema(&macd_line, signal);
    if macd_line.len() >= 2 && signal_line.len() >= 2 {
        let pm = macd_line[macd_line.len() - 2];
        let cm = macd_line[macd_line.len() - 1];
        let ps = signal_line[signal_line.len() - 2];
        let cs = signal_line[signal_line.len() - 1];
        Some(((pm, ps), (cm, cs)))
    } else { None }
}

fn vwap_last_two(ohlcv: &[(u64, f32, f32, f32, f32)], reset_daily_utc: bool) -> Option<(f32, f32)> {
    if ohlcv.len() < 2 { return None; }
    let start_idx = if reset_daily_utc {
        let day_ms = 86_400_000u64;
        let last_day = ohlcv[ohlcv.len() - 1].0 / day_ms;
        ohlcv.iter().rposition(|(t, _, _, _, _)| (*t / day_ms) != last_day).map(|i| i + 1).unwrap_or(0)
    } else { 0 };
    let mut sum_w = 0.0f32;
    let mut sum_wx = 0.0f32;
    let mut out = Vec::with_capacity(ohlcv.len().saturating_sub(start_idx));
    for &(_, _, _, close, vol) in &ohlcv[start_idx..] {
        sum_w += vol;
        sum_wx += close * vol;
        if sum_w > 0.0 { out.push(sum_wx / sum_w); }
    }
    if out.len() >= 2 { Some((out[out.len()-2], out[out.len()-1])) } else { None }
}

fn eval_tick_rules(snapshot: TickEvalSnapshot) -> Vec<uuid::Uuid> {
    let mut triggered = Vec::new();
    let prev = snapshot.prev_price;
    let cur = snapshot.cur_price;
    let kind = snapshot.kind;
    let cfg = snapshot.cfg;
    let closes = snapshot.closes.unwrap_or_default();
    let ohlcv = snapshot.ohlcv.unwrap_or_default();

    for rule in &snapshot.rules {
        if !rule.enabled { continue; }
        if !matches!(rule.evaluation, data::rules::EvaluationMode::OnTick | data::rules::EvaluationMode::Both) {
            continue;
        }
        let ok = match &rule.condition {
            RuleCondition::PriceCrossLevel { level, direction } => {
                match (prev, cur) {
                    (Some(p), Some(c)) => match direction {
                        data::rules::CrossDirection::CrossUp => p < *level && c >= *level,
                        data::rules::CrossDirection::CrossDown => p > *level && c <= *level,
                    },
                    _ => false,
                }
            }
            RuleCondition::MovingAverageCross { direction } => {
                if let Some(kind) = kind.as_ref()
                    && let Some(((fk, fp), (sk, sp))) = ma_pair_from_kind(kind)
                    && let Some((pf, cf)) = ma_last_two(&closes, fk, fp)
                    && let Some((ps, cs)) = ma_last_two(&closes, sk, sp)
                {
                    match direction {
                        data::rules::CrossDirection::CrossUp => pf < ps && cf >= cs,
                        data::rules::CrossDirection::CrossDown => pf > ps && cf <= cs,
                    }
                } else {
                    false
                }
            }
            RuleCondition::RsiCrossLevel { level, direction } => {
                if let Some(cfg) = cfg.as_ref()
                    && let Some((pr, cr)) = rsi_last_two(&closes, cfg.rsi_period)
                {
                    match direction {
                        data::rules::CrossDirection::CrossUp => pr < *level && cr >= *level,
                        data::rules::CrossDirection::CrossDown => pr > *level && cr <= *level,
                    }
                } else {
                    false
                }
            }
            RuleCondition::MacdCrossSignal { direction } => {
                if let Some(cfg) = cfg.as_ref()
                    && let Some(((pm, ps), (cm, cs))) =
                        macd_last_two(&closes, cfg.macd_fast, cfg.macd_slow, cfg.macd_signal)
                {
                    match direction {
                        data::rules::CrossDirection::CrossUp => pm < ps && cm >= cs,
                        data::rules::CrossDirection::CrossDown => pm > ps && cm <= cs,
                    }
                } else {
                    false
                }
            }
            RuleCondition::VwapCross { direction } => {
                if let (Some(p), Some(c)) = (prev, cur) {
                    let reset = kind.as_ref().map(vwap_reset_note).unwrap_or(false);
                    if let Some((_pv, vwap)) = vwap_last_two(&ohlcv, reset) {
                        match direction {
                            data::rules::CrossDirection::CrossUp => p < vwap && c >= vwap,
                            data::rules::CrossDirection::CrossDown => p > vwap && c <= vwap,
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            // Remaining tick-evals are kept on candle-close to avoid heavy math on every trade.
            _ => false,
        };
        if ok { triggered.push(rule.id); }
    }

    triggered
}

#[derive(Debug, Clone)]
pub enum Message {
    Pane(window::Id, pane::Message),
    ChangePaneStatus(uuid::Uuid, pane::Status),
    SavePopoutSpecs(HashMap<window::Id, WindowSpec>),
    ErrorOccurred(Option<uuid::Uuid>, DashboardError),
    Notification(Toast),
    DistributeFetchedData {
        layout_id: uuid::Uuid,
        pane_id: uuid::Uuid,
        stream: StreamKind,
        data: FetchedData,
    },
    ResolveStreams(uuid::Uuid, Vec<PersistStreamKind>),
    PlaySound(SoundType),
    RuleEvalTickDone {
        pane_id: uuid::Uuid,
        triggered: Vec<uuid::Uuid>,
    },
    NoOp,
}

pub struct Dashboard {
    pub panes: pane_grid::State<pane::State>,
    pub focus: Option<(window::Id, pane_grid::Pane)>,
    pub popout: HashMap<window::Id, (pane_grid::State<pane::State>, WindowSpec)>,
    pub streams: UniqueStreams,
    layout_id: uuid::Uuid,
}

impl Default for Dashboard {
    fn default() -> Self {
        Self {
            panes: pane_grid::State::with_configuration(Self::default_pane_config()),
            focus: None,
            streams: UniqueStreams::default(),
            popout: HashMap::new(),
            layout_id: uuid::Uuid::new_v4(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Notification(Toast),
    PlaySound(SoundType),
    DistributeFetchedData {
        layout_id: uuid::Uuid,
        pane_id: uuid::Uuid,
        data: FetchedData,
        stream: StreamKind,
    },
    ResolveStreams {
        pane_id: uuid::Uuid,
        streams: Vec<PersistStreamKind>,
    },
}

impl Dashboard {
    pub fn zoom_focused_pane(&mut self, delta_y: f32, main_window_id: window::Id) {
        let Some((window, pane_id)) = self.focus else { return; };

        if window == main_window_id {
            if let Some(state) = self.panes.get_mut(pane_id) {
                state.zoom_focused_chart(delta_y);
            }
        } else if let Some((panes, _spec)) = self.popout.get_mut(&window) {
            if let Some(state) = panes.get_mut(pane_id) {
                state.zoom_focused_chart(delta_y);
            }
        }
    }
    fn default_pane_config() -> Configuration<pane::State> {
        Configuration::Split {
            axis: pane_grid::Axis::Vertical,
            ratio: 0.8,
            a: Box::new(Configuration::Split {
                axis: pane_grid::Axis::Horizontal,
                ratio: 0.4,
                a: Box::new(Configuration::Split {
                    axis: pane_grid::Axis::Vertical,
                    ratio: 0.5,
                    a: Box::new(Configuration::Pane(pane::State::default())),
                    b: Box::new(Configuration::Pane(pane::State::default())),
                }),
                b: Box::new(Configuration::Split {
                    axis: pane_grid::Axis::Vertical,
                    ratio: 0.5,
                    a: Box::new(Configuration::Pane(pane::State::default())),
                    b: Box::new(Configuration::Pane(pane::State::default())),
                }),
            }),
            b: Box::new(Configuration::Pane(pane::State::default())),
        }
    }

    pub fn from_config(
        panes: Configuration<pane::State>,
        popout_windows: Vec<(Configuration<pane::State>, WindowSpec)>,
        layout_id: uuid::Uuid,
    ) -> Self {
        let panes = pane_grid::State::with_configuration(panes);

        let mut popout = HashMap::new();

        for (pane, specs) in popout_windows {
            popout.insert(
                window::Id::unique(),
                (pane_grid::State::with_configuration(pane), specs),
            );
        }

        Self {
            panes,
            focus: None,
            streams: UniqueStreams::default(),
            popout,
            layout_id,
        }
    }

    pub fn load_layout(&mut self, main_window: window::Id) -> Task<Message> {
        let mut open_popouts_tasks: Vec<Task<Message>> = vec![];
        let mut new_popout = Vec::new();
        let mut keys_to_remove = Vec::new();

        for (old_window_id, (_, specs)) in &self.popout {
            keys_to_remove.push((*old_window_id, *specs));
        }

        // remove keys and open new windows
        for (old_window_id, window_spec) in keys_to_remove {
            let (window, task) = window::open(window::Settings {
                position: window::Position::Specific(window_spec.position()),
                size: window_spec.size(),
                exit_on_close_request: false,
                ..window::settings()
            });

            open_popouts_tasks.push(task.then(|_| Task::none()));

            if let Some((removed_pane, specs)) = self.popout.remove(&old_window_id) {
                new_popout.push((window, (removed_pane, specs)));
            }
        }

        // assign new windows to old panes
        for (window, (pane, specs)) in new_popout {
            self.popout.insert(window, (pane, specs));
        }

        Task::batch(open_popouts_tasks).chain(self.refresh_streams(main_window))
    }

    pub fn update(
        &mut self,
        message: Message,
        main_window: &Window,
        layout_id: &uuid::Uuid,
    ) -> (Task<Message>, Option<Event>) {
        match message {
            Message::RuleEvalTickDone { pane_id, triggered } => {
                let mut tasks: Vec<Task<Message>> = vec![];
                let Some(state) = self.get_mut_pane_state_by_uuid(main_window.id, pane_id) else {
                    return (Task::none(), None);
                };

                for rule_id in triggered {
                    let Some(rule) = state.rules.iter().find(|r| r.id == rule_id).cloned() else {
                        continue;
                    };
                    if !rule.enabled
                        || !matches!(
                            rule.evaluation,
                            data::rules::EvaluationMode::OnTick | data::rules::EvaluationMode::Both
                        )
                    {
                        continue;
                    }

                    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
                    if !state.cooldown_allows(&rule, now_ms) {
                        continue;
                    }

                    let mut parts: Vec<String> = vec!["triggered (tick)".to_string()];
                    let mut toast_msg: Option<String> = None;
                    let mut fill_msg: Option<String> = None;

                    for action in &rule.actions {
                        match action {
                            data::rules::RuleAction::Toast { message } => {
                                parts.push(format!("toast: {message}"));
                                toast_msg = Some(message.clone());
                            }
                            data::rules::RuleAction::PaperTrade {
                                side,
                                percent_of_balance,
                            } => {
                                let Some(price) = state.current_price() else { continue; };
                                if let Some(fill) =
                                    state.paper_trade(*side, *percent_of_balance, price)
                                {
                                    parts.push(fill.clone());
                                    fill_msg = Some(fill);
                                }
                            }
                            data::rules::RuleAction::Sound { enabled } => {
                                if *enabled {
                                    parts.push("sound".to_string());
                                    let dir = match &rule.condition {
                                        data::rules::RuleCondition::PriceCrossLevel { direction, .. }
                                        | data::rules::RuleCondition::CandleCloseCrossLevel { direction, .. }
                                        | data::rules::RuleCondition::MovingAverageCross { direction }
                                        | data::rules::RuleCondition::RsiCrossLevel { direction, .. }
                                        | data::rules::RuleCondition::MacdCrossSignal { direction }
                                        | data::rules::RuleCondition::VwapCross { direction }
                                        | data::rules::RuleCondition::SupertrendFlip { direction }
                                        | data::rules::RuleCondition::SupertrendLineCross { direction }
                                        | data::rules::RuleCondition::DonchianBreakout { direction }
                                        | data::rules::RuleCondition::KeltnerBreakout { direction }
                                        | data::rules::RuleCondition::DmiCross { direction } => *direction,
                                        _ => data::rules::CrossDirection::CrossUp,
                                    };
                                    let sound = match dir {
                                        data::rules::CrossDirection::CrossUp => crate::audio::SoundType::Buy,
                                        data::rules::CrossDirection::CrossDown => crate::audio::SoundType::Sell,
                                    };
                                    tasks.push(Task::done(Message::PlaySound(sound)));
                                }
                            }
                            data::rules::RuleAction::Telegram { enabled } => {
                                if *enabled {
                                    parts.push("telegram".to_string());
                                    let ticker = state
                                        .stream_pair()
                                        .map(|ti| format!("{}", ti.ticker))
                                        .unwrap_or_else(|| "unknown".to_string());
                                    let text = format!("[Rule] {ticker}: {}", rule.name);
                                    tasks.push(Task::perform(crate::telegram::send_message(text), |res| {
                                        match res {
                                            Ok(()) => Message::NoOp,
                                            Err(e) => Message::Notification(Toast::warn(format!(
                                                "Telegram: {e}"
                                            ))),
                                        }
                                    }));
                                }
                            }
                            data::rules::RuleAction::Push { enabled } => {
                                if *enabled {
                                    parts.push("push".to_string());
                                    let ticker = state
                                        .stream_pair()
                                        .map(|ti| format!("{}", ti.ticker))
                                        .unwrap_or_else(|| "unknown".to_string());
                                    let text = format!("[Rule] {ticker}: {} (tick)", rule.name);
                                    tasks.push(Task::perform(crate::push::send_message(text), |res| {
                                        match res {
                                            Ok(()) => Message::NoOp,
                                            Err(e) => Message::Notification(Toast::warn(format!(
                                                "Push: {e}"
                                            ))),
                                        }
                                    }));
                                }
                            }
                        }
                    }

                    if toast_msg.is_some() || fill_msg.is_some() {
                        let mut msg = format!(
                            "[Rule] {}: {}",
                            rule.name,
                            toast_msg.unwrap_or_else(|| "Triggered".to_string())
                        );
                        if let Some(fill) = fill_msg {
                            msg = format!("{msg} | {fill}");
                        }
                        state.push_notification(Toast::new(Notification::Info(msg)));
                    }

                    state.push_rule_log(&rule, parts.join(" | "));
                }

                return (Task::batch(tasks), None);
            }
            Message::SavePopoutSpecs(specs) => {
                for (window_id, new_spec) in specs {
                    if let Some((_, spec)) = self.popout.get_mut(&window_id) {
                        *spec = new_spec;
                    }
                }
            }
            Message::ErrorOccurred(pane_id, err) => match pane_id {
                Some(id) => {
                    if let Some(state) = self.get_mut_pane_state_by_uuid(main_window.id, id) {
                        state.status = pane::Status::Ready;
                        state.notifications.push(Toast::error(err.to_string()));
                    }
                }
                _ => {
                    return (
                        Task::done(Message::Notification(Toast::error(err.to_string()))),
                        None,
                    );
                }
            },
            Message::Pane(window, message) => match message {
                pane::Message::PaneClicked(pane) => {
                    self.focus = Some((window, pane));
                }
                pane::Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                    self.panes.resize(split, ratio);
                }
                pane::Message::PaneDragged(event) => {
                    if let pane_grid::DragEvent::Dropped { pane, target } = event {
                        self.panes.drop(pane, target);
                    }
                }
                pane::Message::SplitPane(axis, pane) => {
                    let focus_pane = if let Some((new_pane, _)) =
                        self.panes.split(axis, pane, pane::State::new())
                    {
                        Some(new_pane)
                    } else {
                        None
                    };

                    if Some(focus_pane).is_some() {
                        self.focus = Some((window, focus_pane.unwrap()));
                    }
                }
                pane::Message::ClosePane(pane) => {
                    if let Some((_, sibling)) = self.panes.close(pane) {
                        self.focus = Some((window, sibling));
                    }
                }
                pane::Message::MaximizePane(pane) => {
                    self.panes.maximize(pane);
                }
                pane::Message::Restore => {
                    self.panes.restore();
                }
                pane::Message::ReplacePane(pane) => {
                    if let Some(pane) = self.panes.get_mut(pane) {
                        *pane = pane::State::new();
                    }

                    return (self.refresh_streams(main_window.id), None);
                }
                pane::Message::VisualConfigChanged(pane, cfg, to_sync) => {
                    let mut refresh_needed = false;
                    let mut fetch_tasks: Vec<Task<Message>> = vec![];

                    if to_sync {
                        if let Some(state) = self.get_pane(main_window.id, window, pane) {
                            let studies_cfg = state.content.studies();
                            let clusters_cfg = match &state.content {
                                pane::Content::Kline {
                                    kind: data::chart::KlineChartKind::Footprint { clusters, .. },
                                    ..
                                } => Some(*clusters),
                                _ => None,
                            };

                            self.iter_all_panes_mut(main_window.id)
                                .for_each(|(_, _, state)| {
                                    let should_apply = match state.settings.visual_config {
                                        Some(ref current_cfg) => {
                                            std::mem::discriminant(current_cfg)
                                                == std::mem::discriminant(&cfg)
                                        }
                                        None => matches!(
                                            (&cfg, &state.content),
                                            (
                                                data::layout::pane::VisualConfig::Kline(_),
                                                pane::Content::Kline { .. }
                                            ) | (
                                                data::layout::pane::VisualConfig::Heatmap(_),
                                                pane::Content::Heatmap { .. }
                                            ) | (
                                                data::layout::pane::VisualConfig::TimeAndSales(_),
                                                pane::Content::TimeAndSales(_)
                                            ) | (
                                                data::layout::pane::VisualConfig::Comparison(_),
                                                pane::Content::Comparison(_)
                                            )
                                        ),
                                    };

                                    if should_apply {
                                        state.settings.visual_config = Some(cfg.clone());
                                        state.content.change_visual_config(cfg.clone());

                                        if let data::layout::pane::VisualConfig::Heatmap(h_cfg) =
                                            &cfg
                                        {
                                            let (changed, fetch_stream) =
                                                state.update_heatmap_overlay_streams(*h_cfg);
                                            if changed {
                                                refresh_needed = true;
                                            }
                                            if let Some(stream) = fetch_stream {
                                                fetch_tasks.push(kline_fetch_task(
                                                    *layout_id,
                                                    state.unique_id(),
                                                    stream,
                                                    None,
                                                    None,
                                                ));
                                            }
                                        }

                                        if let Some(studies) = &studies_cfg {
                                            state.content.update_studies(studies.clone());
                                        }

                                        if let Some(cluster_kind) = &clusters_cfg
                                            && let pane::Content::Kline { chart, .. } =
                                                &mut state.content
                                            && let Some(c) = chart
                                        {
                                            c.set_cluster_kind(*cluster_kind);
                                        }
                                    }
                                });
                        }
                    } else if let Some(state) = self.get_mut_pane(main_window.id, window, pane) {
                        state.settings.visual_config = Some(cfg.clone());
                        state.content.change_visual_config(cfg.clone());

                        if let data::layout::pane::VisualConfig::Heatmap(h_cfg) = &cfg {
                            let (changed, fetch_stream) =
                                state.update_heatmap_overlay_streams(*h_cfg);
                            if changed {
                                refresh_needed = true;
                            }
                            if let Some(stream) = fetch_stream {
                                fetch_tasks.push(kline_fetch_task(
                                    *layout_id,
                                    state.unique_id(),
                                    stream,
                                    None,
                                    None,
                                ));
                            }
                        }
                    }

                    if refresh_needed {
                        let refresh = self.refresh_streams(main_window.id);
                        let task = if fetch_tasks.is_empty() {
                            refresh
                        } else {
                            Task::batch(fetch_tasks).chain(refresh)
                        };
                        return (task, None);
                    }
                }
                pane::Message::SwitchLinkGroup(pane, group) => {
                    if group.is_none() {
                        if let Some(state) = self.get_mut_pane(main_window.id, window, pane) {
                            state.link_group = None;
                        }
                        return (Task::none(), None);
                    }

                    let maybe_ticker_info = self
                        .iter_all_panes(main_window.id)
                        .filter(|(w, p, _)| !(*w == window && *p == pane))
                        .find_map(|(_, _, other_state)| {
                            if other_state.link_group == group {
                                other_state.stream_pair()
                            } else {
                                None
                            }
                        });

                    if let Some(state) = self.get_mut_pane(main_window.id, window, pane) {
                        state.link_group = group;
                        state.modal = None;

                        if let Some(ticker_info) = maybe_ticker_info
                            && state.stream_pair() != Some(ticker_info)
                        {
                            let pane_id = state.unique_id();
                            let content_kind = state.content.kind();

                            let streams =
                                state.set_content_and_streams(vec![ticker_info], content_kind);
                            self.streams.extend(streams.iter());

                            for stream in &streams {
                                if let StreamKind::Kline { .. } = stream {
                                    return (
                                        kline_fetch_task(*layout_id, pane_id, *stream, None, None),
                                        None,
                                    );
                                }
                            }
                        }
                    }
                }
                pane::Message::Popout => {
                    return (self.popout_pane(main_window), None);
                }
                pane::Message::Merge => {
                    return (self.merge_pane(main_window), None);
                }
                pane::Message::PaneEvent(pane, local) => {
                    if let Some(state) = self.get_mut_pane(main_window.id, window, pane) {
                        let Some(effect) = state.update(local) else {
                            return (Task::none(), None);
                        };

                        let task = match effect {
                            pane::Effect::RefreshStreams => self.refresh_streams(main_window.id),
                            pane::Effect::RequestFetch(reqs) => request_fetch_many(
                                state,
                                *layout_id,
                                reqs.into_iter().map(|r| (r.req_id, r.fetch, r.stream)),
                            )
                            .chain(self.refresh_streams(main_window.id)),
                            pane::Effect::SwitchTickersInGroup(ticker_info) => {
                                self.switch_tickers_in_group(main_window.id, ticker_info)
                            }
                            pane::Effect::FocusWidget(id) => {
                                return (iced::widget::operation::focus(id), None);
                            }
                        };
                        return (task, None);
                    }
                }
            },
            Message::ChangePaneStatus(pane_id, status) => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window.id, pane_id) {
                    pane_state.status = status;
                }
            }
            Message::DistributeFetchedData {
                layout_id,
                pane_id,
                data,
                stream,
            } => {
                return (
                    Task::none(),
                    Some(Event::DistributeFetchedData {
                        layout_id,
                        pane_id,
                        data,
                        stream,
                    }),
                );
            }
            Message::ResolveStreams(pane_id, streams) => {
                return (
                    Task::none(),
                    Some(Event::ResolveStreams { pane_id, streams }),
                );
            }
            Message::Notification(toast) => {
                return (Task::none(), Some(Event::Notification(toast)));
            }
            Message::PlaySound(sound) => {
                return (Task::none(), Some(Event::PlaySound(sound)));
            }
            Message::NoOp => {}
        }

        (Task::none(), None)
    }

    fn new_pane(
        &mut self,
        axis: pane_grid::Axis,
        main_window: &Window,
        pane_state: Option<pane::State>,
    ) -> Task<Message> {
        if self
            .focus
            .filter(|(window, _)| *window == main_window.id)
            .is_some()
        {
            // If there is any focused pane on main window, split it
            return self.split_pane(axis, main_window);
        } else {
            // If there is no focused pane, split the last pane or create a new empty grid
            let pane = self.panes.iter().last().map(|(pane, _)| pane).copied();

            if let Some(pane) = pane {
                let result = self.panes.split(axis, pane, pane_state.unwrap_or_default());

                if let Some((pane, _)) = result {
                    return self.focus_pane(main_window.id, pane);
                }
            } else {
                let (state, pane) = pane_grid::State::new(pane_state.unwrap_or_default());
                self.panes = state;

                return self.focus_pane(main_window.id, pane);
            }
        }

        Task::none()
    }

    fn focus_pane(&mut self, window: window::Id, pane: pane_grid::Pane) -> Task<Message> {
        if self.focus != Some((window, pane)) {
            self.focus = Some((window, pane));
        }

        Task::none()
    }

    fn split_pane(&mut self, axis: pane_grid::Axis, main_window: &Window) -> Task<Message> {
        if let Some((window, pane)) = self.focus
            && window == main_window.id
        {
            let result = self.panes.split(axis, pane, pane::State::new());

            if let Some((pane, _)) = result {
                return self.focus_pane(main_window.id, pane);
            }
        }

        Task::none()
    }

    fn popout_pane(&mut self, main_window: &Window) -> Task<Message> {
        if let Some((_, id)) = self.focus.take()
            && let Some((pane, _)) = self.panes.close(id)
        {
            let (window, task) = window::open(window::Settings {
                position: main_window
                    .position
                    .map(|point| window::Position::Specific(point + Vector::new(20.0, 20.0)))
                    .unwrap_or_default(),
                exit_on_close_request: false,
                min_size: Some(iced::Size::new(400.0, 300.0)),
                ..window::settings()
            });

            let (state, id) = pane_grid::State::new(pane);
            self.popout.insert(window, (state, WindowSpec::default()));

            return task.then(move |window| {
                Task::done(Message::Pane(window, pane::Message::PaneClicked(id)))
            });
        }

        Task::none()
    }

    fn merge_pane(&mut self, main_window: &Window) -> Task<Message> {
        if let Some((window, pane)) = self.focus.take()
            && let Some(pane_state) = self
                .popout
                .remove(&window)
                .and_then(|(mut panes, _)| panes.panes.remove(&pane))
        {
            let task = self.new_pane(pane_grid::Axis::Horizontal, main_window, Some(pane_state));

            return Task::batch(vec![window::close(window), task]);
        }

        Task::none()
    }

    pub fn get_pane(
        &self,
        main_window: window::Id,
        window: window::Id,
        pane: pane_grid::Pane,
    ) -> Option<&pane::State> {
        if main_window == window {
            self.panes.get(pane)
        } else {
            self.popout
                .get(&window)
                .and_then(|(panes, _)| panes.get(pane))
        }
    }

    fn get_mut_pane(
        &mut self,
        main_window: window::Id,
        window: window::Id,
        pane: pane_grid::Pane,
    ) -> Option<&mut pane::State> {
        if main_window == window {
            self.panes.get_mut(pane)
        } else {
            self.popout
                .get_mut(&window)
                .and_then(|(panes, _)| panes.get_mut(pane))
        }
    }

    fn get_mut_pane_state_by_uuid(
        &mut self,
        main_window: window::Id,
        uuid: uuid::Uuid,
    ) -> Option<&mut pane::State> {
        self.iter_all_panes_mut(main_window)
            .find(|(_, _, state)| state.unique_id() == uuid)
            .map(|(_, _, state)| state)
    }

    fn iter_all_panes(
        &self,
        main_window: window::Id,
    ) -> impl Iterator<Item = (window::Id, pane_grid::Pane, &pane::State)> {
        self.panes
            .iter()
            .map(move |(pane, state)| (main_window, *pane, state))
            .chain(self.popout.iter().flat_map(|(window_id, (panes, _))| {
                panes.iter().map(|(pane, state)| (*window_id, *pane, state))
            }))
    }

    fn iter_all_panes_mut(
        &mut self,
        main_window: window::Id,
    ) -> impl Iterator<Item = (window::Id, pane_grid::Pane, &mut pane::State)> {
        self.panes
            .iter_mut()
            .map(move |(pane, state)| (main_window, *pane, state))
            .chain(self.popout.iter_mut().flat_map(|(window_id, (panes, _))| {
                panes
                    .iter_mut()
                    .map(|(pane, state)| (*window_id, *pane, state))
            }))
    }

    pub fn view<'a>(
        &'a self,
        main_window: &'a Window,
        tickers_table: &'a TickersTable,
        timezone: UserTimezone,
    ) -> Element<'a, Message> {
        let pane_grid: Element<_> = PaneGrid::new(&self.panes, |id, pane, maximized| {
            let is_focused = self.focus == Some((main_window.id, id));
            pane.view(
                id,
                self.panes.len(),
                is_focused,
                maximized,
                main_window.id,
                main_window,
                timezone,
                tickers_table,
            )
        })
        .min_size(240)
        .on_click(pane::Message::PaneClicked)
        .on_drag(pane::Message::PaneDragged)
        .on_resize(8, pane::Message::PaneResized)
        .spacing(6)
        .style(style::pane_grid)
        .into();

        let base: Element<'a, Message> =
            pane_grid.map(move |message| Message::Pane(main_window.id, message));

        // Window-level modals for the focused pane (so they're not constrained by pane bounds).
        let Some((focus_window, focus_pane)) = self.focus else {
            return base;
        };
        if focus_window != main_window.id {
            return base;
        }

        let Some(state) = self.panes.get(focus_pane) else {
            return base;
        };

        let (modal_kind, modal_content): (
            Option<pane::CenteredModalKind>,
            Option<Element<'a, Message>>,
        ) = match state.modal {
            Some(PaneModal::Rules) => (Some(pane::CenteredModalKind::Rules), Some(
                pane_modal::rules::view(focus_pane, state)
                    .map(move |m| Message::Pane(main_window.id, m)),
            )),
            Some(PaneModal::RuleLog) => (Some(pane::CenteredModalKind::RuleLog), Some(
                pane_modal::rule_log::view(focus_pane, state)
                    .map(move |m| Message::Pane(main_window.id, m)),
            )),
            Some(PaneModal::Indicators) => {
                let market_type = state.stream_pair().map(|i| i.ticker.market_type());
                (Some(pane::CenteredModalKind::Indicators), Some(
                    match &state.content {
                        pane::Content::Kline { indicators, .. } => {
                            pane_modal::indicators::view(focus_pane, state, indicators, market_type)
                        }
                        pane::Content::Heatmap { indicators, .. } => {
                            pane_modal::indicators::view(focus_pane, state, indicators, market_type)
                        }
                        _ => unreachable!(),
                    }
                    .map(move |m| Message::Pane(main_window.id, m)),
                ))
            }
            _ => (None, None),
        };

        if let Some(content) = modal_content {
            let content: Element<'a, Message> = if let Some(kind) = modal_kind {
                let (w, h) = match kind {
                    pane::CenteredModalKind::Rules => state.centered_rules_size,
                    pane::CenteredModalKind::Indicators => state.centered_indicators_size,
                    pane::CenteredModalKind::RuleLog => state.centered_rule_log_size,
                };
                let on_resize = move |nw: f32, nh: f32| {
                    Message::Pane(
                        main_window.id,
                        pane::Message::PaneEvent(
                            focus_pane,
                            pane::Event::ResizeCenteredModal(kind, nw, nh),
                        ),
                    )
                };
                ResizeBox::new(content, w, h, on_resize).into()
            } else {
                content
            };
            // Prevent click-through to the pane grid.
            let content: Element<'a, Message> =
                mouse_area(content).on_press(Message::NoOp).into();
            let on_blur = Message::Pane(main_window.id, pane::Message::PaneEvent(focus_pane, pane::Event::HideModal));
            pane_modal::stack_modal(
                base,
                content,
                on_blur,
                padding::Padding::default(),
                Alignment::Center,
                Alignment::Center,
            )
        } else {
            base
        }
    }

    pub fn view_window<'a>(
        &'a self,
        window: window::Id,
        main_window: &'a Window,
        tickers_table: &'a TickersTable,
        timezone: UserTimezone,
    ) -> Element<'a, Message> {
        if let Some((state, _)) = self.popout.get(&window) {
            let content = container(
                PaneGrid::new(state, |id, pane, _maximized| {
                    let is_focused = self.focus == Some((window, id));
                    pane.view(
                        id,
                        state.len(),
                        is_focused,
                        false,
                        window,
                        main_window,
                        timezone,
                        tickers_table,
                    )
                })
                .on_click(pane::Message::PaneClicked),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(8);

            let base: Element<'a, Message> =
                Element::new(content).map(move |message| Message::Pane(window, message));

            // Window-level modals for the focused pane in this popout window.
            let Some((focus_window, focus_pane)) = self.focus else {
                return base;
            };
            if focus_window != window {
                return base;
            }

            let Some(pane_state) = state.get(focus_pane) else {
                return base;
            };

            let (modal_kind, modal_content): (Option<pane::CenteredModalKind>, Option<Element<'a, Message>>) =
                match pane_state.modal {
                Some(PaneModal::Rules) => (Some(pane::CenteredModalKind::Rules), Some(
                    pane_modal::rules::view(focus_pane, pane_state)
                        .map(move |m| Message::Pane(window, m)),
                )),
                Some(PaneModal::RuleLog) => (Some(pane::CenteredModalKind::RuleLog), Some(
                    pane_modal::rule_log::view(focus_pane, pane_state)
                        .map(move |m| Message::Pane(window, m)),
                )),
                Some(PaneModal::Indicators) => {
                    let market_type = pane_state.stream_pair().map(|i| i.ticker.market_type());
                    (Some(pane::CenteredModalKind::Indicators), Some(
                        match &pane_state.content {
                            pane::Content::Kline { indicators, .. } => {
                                pane_modal::indicators::view(focus_pane, pane_state, indicators, market_type)
                            }
                            pane::Content::Heatmap { indicators, .. } => {
                                pane_modal::indicators::view(focus_pane, pane_state, indicators, market_type)
                            }
                            _ => unreachable!(),
                        }
                        .map(move |m| Message::Pane(window, m)),
                    ))
                }
                _ => (None, None),
            };

            if let Some(content) = modal_content {
                let content: Element<'a, Message> = if let Some(kind) = modal_kind {
                    let (w, h) = match kind {
                        pane::CenteredModalKind::Rules => pane_state.centered_rules_size,
                        pane::CenteredModalKind::Indicators => pane_state.centered_indicators_size,
                        pane::CenteredModalKind::RuleLog => pane_state.centered_rule_log_size,
                    };
                    let on_resize = move |nw: f32, nh: f32| {
                        Message::Pane(
                            window,
                            pane::Message::PaneEvent(
                                focus_pane,
                                pane::Event::ResizeCenteredModal(kind, nw, nh),
                            ),
                        )
                    };
                    ResizeBox::new(content, w, h, on_resize).into()
                } else {
                    content
                };
                // Prevent click-through to the pane grid (which can change focus and effectively
                // "close" the modal). Any click inside the modal publishes NoOp and captures it.
                let content: Element<'a, Message> =
                    mouse_area(content).on_press(Message::NoOp).into();
                let on_blur = Message::Pane(window, pane::Message::PaneEvent(focus_pane, pane::Event::HideModal));
                pane_modal::stack_modal(
                    base,
                    content,
                    on_blur,
                    padding::Padding::default(),
                    Alignment::Center,
                    Alignment::Center,
                )
            } else {
                base
            }
        } else {
            Element::new(center("No pane found for window"))
                .map(move |message| Message::Pane(window, message))
        }
    }

    pub fn go_back(&mut self, main_window: window::Id) -> bool {
        let Some((window, pane)) = self.focus else {
            return false;
        };

        let Some(state) = self.get_mut_pane(main_window, window, pane) else {
            return false;
        };

        if state.modal.is_some() {
            state.modal = None;
            return true;
        }
        false
    }

    fn handle_error(
        &mut self,
        pane_id: Option<uuid::Uuid>,
        err: &DashboardError,
        main_window: window::Id,
    ) -> Task<Message> {
        match pane_id {
            Some(id) => {
                if let Some(state) = self.get_mut_pane_state_by_uuid(main_window, id) {
                    state.status = pane::Status::Ready;
                    state.notifications.push(Toast::error(err.to_string()));
                }
                Task::none()
            }
            _ => Task::done(Message::Notification(Toast::error(err.to_string()))),
        }
    }

    fn init_pane(
        &mut self,
        main_window: window::Id,
        window: window::Id,
        selected_pane: pane_grid::Pane,
        ticker_info: TickerInfo,
        content_kind: ContentKind,
    ) -> Task<Message> {
        if let Some(state) = self.get_mut_pane(main_window, window, selected_pane) {
            let pane_id = state.unique_id();

            let streams = state.set_content_and_streams(vec![ticker_info], content_kind);
            self.streams.extend(streams.iter());

            for stream in &streams {
                if let StreamKind::Kline { .. } = stream {
                    return kline_fetch_task(self.layout_id, pane_id, *stream, None, None);
                }
            }
        }

        Task::none()
    }

    pub fn init_focused_pane(
        &mut self,
        main_window: window::Id,
        ticker_info: TickerInfo,
        content_kind: ContentKind,
    ) -> Task<Message> {
        if self.focus.is_none()
            && self.panes.len() == 1
            && let Some((pane_id, _)) = self.panes.iter().next()
        {
            self.focus = Some((main_window, *pane_id));
        }

        if let Some((window, selected_pane)) = self.focus
            && let Some(state) = self.get_mut_pane(main_window, window, selected_pane)
        {
            let previous_ticker = state.stream_pair();
            if previous_ticker.is_some() && previous_ticker != Some(ticker_info) {
                state.link_group = None;
            }

            let streams = state.set_content_and_streams(vec![ticker_info], content_kind);

            let pane_id = state.unique_id();
            self.streams.extend(streams.iter());

            for stream in &streams {
                if let StreamKind::Kline { .. } = stream {
                    return kline_fetch_task(self.layout_id, pane_id, *stream, None, None);
                }
            }
            return Task::none();
        }

        Task::done(Message::Notification(Toast::warn(
            "No focused pane found".to_string(),
        )))
    }

    pub fn switch_tickers_in_group(
        &mut self,
        main_window: window::Id,
        ticker_info: TickerInfo,
    ) -> Task<Message> {
        if self.focus.is_none()
            && self.panes.len() == 1
            && let Some((pane_id, _)) = self.panes.iter().next()
        {
            self.focus = Some((main_window, *pane_id));
        }

        let link_group = self.focus.and_then(|(window, pane)| {
            self.get_pane(main_window, window, pane)
                .and_then(|state| state.link_group)
        });

        if let Some(group) = link_group {
            let pane_infos: Vec<(window::Id, pane_grid::Pane, ContentKind)> = self
                .iter_all_panes_mut(main_window)
                .filter_map(|(window, pane, state)| {
                    if state.link_group == Some(group) {
                        Some((window, pane, state.content.kind()))
                    } else {
                        None
                    }
                })
                .collect();

            let tasks: Vec<Task<Message>> = pane_infos
                .iter()
                .map(|(window, pane, content_kind)| {
                    self.init_pane(main_window, *window, *pane, ticker_info, *content_kind)
                })
                .collect();

            Task::batch(tasks)
        } else if let Some((window, pane)) = self.focus {
            if let Some(state) = self.get_mut_pane(main_window, window, pane) {
                let content_kind = state.content.kind();
                self.init_focused_pane(main_window, ticker_info, content_kind)
            } else {
                Task::done(Message::Notification(Toast::warn(
                    "Couldn't get focused pane's content".to_string(),
                )))
            }
        } else {
            Task::done(Message::Notification(Toast::warn(
                "No link group or focused pane found".to_string(),
            )))
        }
    }

    pub fn toggle_trade_fetch(&mut self, is_enabled: bool, main_window: &Window) {
        exchange::fetcher::toggle_trade_fetch(is_enabled);

        self.iter_all_panes_mut(main_window.id)
            .for_each(|(_, _, state)| {
                if let pane::Content::Kline { chart, kind, .. } = &mut state.content
                    && matches!(kind, data::chart::KlineChartKind::Footprint { .. })
                    && let Some(c) = chart
                {
                    c.reset_request_handler();

                    if !is_enabled {
                        state.status = pane::Status::Ready;
                    }
                }
            });
    }

    pub fn distribute_fetched_data(
        &mut self,
        main_window: window::Id,
        pane_id: uuid::Uuid,
        data: FetchedData,
        stream_type: StreamKind,
    ) -> Task<Message> {
        match data {
            FetchedData::Trades { batch, until_time } => {
                let last_trade_time = batch.last().map_or(0, |trade| trade.time);

                if last_trade_time < until_time {
                    if let Err(reason) =
                        self.insert_fetched_trades(main_window, pane_id, &batch, false)
                    {
                        return self.handle_error(Some(pane_id), &reason, main_window);
                    }
                } else {
                    let filtered_batch = batch
                        .iter()
                        .filter(|trade| trade.time <= until_time)
                        .copied()
                        .collect::<Vec<_>>();

                    if let Err(reason) =
                        self.insert_fetched_trades(main_window, pane_id, &filtered_batch, true)
                    {
                        return self.handle_error(Some(pane_id), &reason, main_window);
                    }
                }
            }
            FetchedData::Klines { data, req_id } => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window, pane_id) {
                    pane_state.status = pane::Status::Ready;

                    if let StreamKind::Kline {
                        timeframe,
                        ticker_info,
                    } = stream_type
                    {
                        pane_state.insert_hist_klines(req_id, timeframe, ticker_info, &data);
                    }
                }
            }
            FetchedData::OI { data, req_id } => {
                if let Some(pane_state) = self.get_mut_pane_state_by_uuid(main_window, pane_id) {
                    pane_state.status = pane::Status::Ready;

                    if let StreamKind::Kline { .. } = stream_type {
                        pane_state.insert_hist_oi(req_id, &data);
                    }
                }
            }
        }

        Task::none()
    }

    fn insert_fetched_trades(
        &mut self,
        main_window: window::Id,
        pane_id: uuid::Uuid,
        trades: &[Trade],
        is_batches_done: bool,
    ) -> Result<(), DashboardError> {
        let pane_state = self
            .get_mut_pane_state_by_uuid(main_window, pane_id)
            .ok_or_else(|| {
                DashboardError::Unknown(
                    "No matching pane state found for fetched trades".to_string(),
                )
            })?;

        match &mut pane_state.status {
            pane::Status::Loading(exchange::fetcher::InfoKind::FetchingTrades(count)) => {
                *count += trades.len();
            }
            _ => {
                pane_state.status = pane::Status::Loading(
                    exchange::fetcher::InfoKind::FetchingTrades(trades.len()),
                );
            }
        }

        match &mut pane_state.content {
            pane::Content::Kline { chart, .. } => {
                if let Some(c) = chart {
                    c.insert_raw_trades(trades.to_owned(), is_batches_done);

                    if is_batches_done {
                        pane_state.status = pane::Status::Ready;
                    }
                    Ok(())
                } else {
                    Err(DashboardError::Unknown(
                        "fetched trades but no chart found".to_string(),
                    ))
                }
            }
            _ => Err(DashboardError::Unknown(
                "No matching chart found for fetched trades".to_string(),
            )),
        }
    }

    pub fn update_latest_klines(
        &mut self,
        stream: &StreamKind,
        kline: &Kline,
        main_window: window::Id,
    ) -> Task<Message> {
        let mut found_match = false;

        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, pane_state)| {
                if pane_state.matches_stream(stream) {
                    if matches!(stream, StreamKind::Kline { .. }) {
                        pane_state.on_kline_update(kline);
                    }
                    match &mut pane_state.content {
                        pane::Content::Kline { chart: Some(c), .. } => {
                            c.update_latest_kline(kline);
                        }
                        pane::Content::Comparison(Some(c)) => {
                            c.update_latest_kline(&stream.ticker_info(), kline);
                        }
                        pane::Content::Heatmap { chart: Some(c), .. } => {
                            if c.visual_config().show_candles {
                                c.on_insert_klines(&[*kline]);
                            }
                        }
                        _ => {}
                    }
                    found_match = true;
                }
            });

        if found_match {
            Task::none()
        } else {
            log::debug!("{stream:?} stream had no matching panes - dropping");
            self.refresh_streams(main_window)
        }
    }

    pub fn update_depth_and_trades(
        &mut self,
        stream: &StreamKind,
        depth_update_t: u64,
        depth: &Depth,
        trades_buffer: &[Trade],
        main_window: window::Id,
    ) -> Task<Message> {
        let mut found_match = false;

        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, pane_state)| {
                if pane_state.matches_stream(stream) {
                    pane_state.on_trades_buffer(trades_buffer);
                    match &mut pane_state.content {
                        pane::Content::Heatmap { chart, .. } => {
                            if let Some(c) = chart {
                                c.insert_datapoint(trades_buffer, depth_update_t, depth);
                            }
                        }
                        pane::Content::Kline { chart, .. } => {
                            if let Some(c) = chart {
                                c.insert_trades_buffer(trades_buffer);
                            }
                        }
                        pane::Content::TimeAndSales(panel) => {
                            if let Some(p) = panel {
                                p.insert_buffer(trades_buffer);
                            }
                        }
                        pane::Content::Ladder(panel) => {
                            if let Some(panel) = panel {
                                panel.insert_buffers(depth_update_t, depth, trades_buffer);
                            }
                        }
                        _ => {
                            log::error!("No chart found for the stream: {stream:?}");
                        }
                    }
                    found_match = true;
                }
            });

        if found_match {
            Task::none()
        } else {
            log::debug!("No matching pane found for the stream: {stream:?}");
            self.refresh_streams(main_window)
        }
    }

    pub fn invalidate_all_panes(&mut self, main_window: window::Id) {
        self.iter_all_panes_mut(main_window)
            .for_each(|(_, _, state)| {
                let _ = state.invalidate(Instant::now());
            });
    }

    pub fn tick(&mut self, now: Instant, main_window: window::Id) -> Task<Message> {
        let mut tasks = vec![];
        let layout_id = self.layout_id;

        self.iter_all_panes_mut(main_window)
            .for_each(|(_window_id, _pane, state)| match state.tick(now) {
                Some(pane::Action::Chart(action)) => match action {
                    chart::Action::ErrorOccurred(err) => {
                        state.status = pane::Status::Ready;
                        state.notifications.push(Toast::error(err.to_string()));
                    }
                    chart::Action::RequestFetch(reqs) => {
                        tasks.push(request_fetch_many(
                            state,
                            layout_id,
                            reqs.into_iter().map(|r| (r.req_id, r.fetch, r.stream)),
                        ));
                    }
                },
                Some(pane::Action::Panel(_action)) => {}
                Some(pane::Action::ResolveStreams(streams)) => {
                    tasks.push(Task::done(Message::ResolveStreams(
                        state.unique_id(),
                        streams,
                    )));
                }
                Some(pane::Action::ResolveContent) => match state.stream_pair_kind() {
                    Some(StreamPairKind::MultiSource(tickers)) => {
                        state.set_content_and_streams(tickers, state.content.kind());
                    }
                    Some(StreamPairKind::SingleSource(ticker)) => {
                        state.set_content_and_streams(vec![ticker], state.content.kind());
                    }
                    None => {}
                },
                None => {}
            });

        // Rules evaluation (MVP): OnTick (price crosses) + OnCandleClose (close/volume)
        self.iter_all_panes_mut(main_window).for_each(|(_, _, state)| {
            let rules_snapshot = state.rules.clone();

            // OnTick evaluation (background): schedule heavy rule math off the UI thread.
            let wants_tick = rules_snapshot.iter().any(|r| {
                r.enabled
                    && matches!(
                        r.evaluation,
                        data::rules::EvaluationMode::OnTick | data::rules::EvaluationMode::Both
                    )
            });
            let allow_tick_eval = wants_tick
                && state.rule_tick_dirty
                && state.rule_tick_last_eval.elapsed().as_millis() >= 150;

            if allow_tick_eval {
                state.rule_tick_dirty = false;
                state.rule_tick_last_eval = now;

                // Only snapshot series if any rule needs them.
                let needs_closes = rules_snapshot.iter().any(|r| {
                    matches!(
                        r.condition,
                        RuleCondition::MovingAverageCross { .. }
                            | RuleCondition::RsiCrossLevel { .. }
                            | RuleCondition::MacdCrossSignal { .. }
                    )
                });
                let needs_ohlcv = rules_snapshot
                    .iter()
                    .any(|r| matches!(r.condition, RuleCondition::VwapCross { .. }));

                let (kind, cfg, closes, ohlcv) = match &state.content {
                    pane::Content::Kline { chart: Some(c), kind, .. } => (
                        Some(kind.clone()),
                        Some(c.visual_config()),
                        needs_closes.then(|| c.close_series()),
                        needs_ohlcv.then(|| c.ohlcv_series()),
                    ),
                    _ => (None, None, None, None),
                };

                let snapshot = TickEvalSnapshot {
                    pane_id: state.unique_id(),
                    rules: rules_snapshot.clone(),
                    prev_price: state.prev_trade_price,
                    cur_price: state.last_trade_price,
                    kind,
                    cfg,
                    closes,
                    ohlcv,
                };
                let pane_id = snapshot.pane_id;
                tasks.push(Task::perform(async move { eval_tick_rules(snapshot) }, move |triggered| {
                    Message::RuleEvalTickDone { pane_id, triggered }
                }));
            }

            // Candle-close evaluation (best-effort based on kline time updates)
            if let Some((_t, close, vol)) = state.pending_candle_close.take() {
                for rule in &rules_snapshot {
                    if !rule.enabled {
                        continue;
                    }
                    if matches!(
                        rule.evaluation,
                        data::rules::EvaluationMode::OnCandleClose | data::rules::EvaluationMode::Both
                    ) && state.eval_condition_candle_close(rule, close, vol)
                    {
                        let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
                        if !state.cooldown_allows(rule, now_ms) {
                            continue;
                        }

                        let mut parts: Vec<String> = vec!["triggered (candle close)".to_string()];
                        let mut toast_msg: Option<String> = None;
                        let mut fill_msg: Option<String> = None;

                        for action in &rule.actions {
                            match action {
                                data::rules::RuleAction::Toast { message } => {
                                    parts.push(format!("toast: {message}"));
                                    toast_msg = Some(message.clone());
                                }
                                data::rules::RuleAction::PaperTrade { side, percent_of_balance } => {
                                    if let Some(fill) = state.paper_trade(*side, *percent_of_balance, close) {
                                        parts.push(fill.clone());
                                        fill_msg = Some(format!("{fill} (on close)"));
                                    }
                                }
                                data::rules::RuleAction::Sound { enabled } => {
                                    if *enabled {
                                        parts.push("sound".to_string());
                                        let dir = match &rule.condition {
                                            data::rules::RuleCondition::PriceCrossLevel { direction, .. }
                                            | data::rules::RuleCondition::CandleCloseCrossLevel { direction, .. }
                                            | data::rules::RuleCondition::MovingAverageCross { direction }
                                            | data::rules::RuleCondition::RsiCrossLevel { direction, .. }
                                            | data::rules::RuleCondition::MacdCrossSignal { direction }
                                            | data::rules::RuleCondition::VwapCross { direction }
                                            | data::rules::RuleCondition::SupertrendFlip { direction }
                                            | data::rules::RuleCondition::SupertrendLineCross { direction }
                                            | data::rules::RuleCondition::DonchianBreakout { direction }
                                            | data::rules::RuleCondition::KeltnerBreakout { direction }
                                            | data::rules::RuleCondition::DmiCross { direction } => *direction,
                                            _ => data::rules::CrossDirection::CrossUp,
                                        };
                                        let sound = match dir {
                                            data::rules::CrossDirection::CrossUp => crate::audio::SoundType::Buy,
                                            data::rules::CrossDirection::CrossDown => crate::audio::SoundType::Sell,
                                        };
                                        tasks.push(Task::done(Message::PlaySound(sound)));
                                    }
                                }
                                data::rules::RuleAction::Telegram { enabled } => {
                                    if *enabled {
                                        parts.push("telegram".to_string());
                                        let ticker = state
                                            .stream_pair()
                                            .map(|ti| format!("{}", ti.ticker))
                                            .unwrap_or_else(|| "unknown".to_string());
                                        let text = format!("[Rule] {ticker}: {} (close)", rule.name);
                                        tasks.push(Task::perform(
                                            crate::telegram::send_message(text),
                                            |res| match res {
                                                Ok(()) => Message::NoOp,
                                                Err(e) => Message::Notification(Toast::warn(format!(
                                                    "Telegram: {e}"
                                                ))),
                                            },
                                        ));
                                    }
                                }
                                data::rules::RuleAction::Push { enabled } => {
                                    if *enabled {
                                        parts.push("push".to_string());
                                        let ticker = state
                                            .stream_pair()
                                            .map(|ti| format!("{}", ti.ticker))
                                            .unwrap_or_else(|| "unknown".to_string());
                                        let text = format!("[Rule] {ticker}: {} (close)", rule.name);
                                        tasks.push(Task::perform(
                                            crate::push::send_message(text),
                                            |res| match res {
                                                Ok(()) => Message::NoOp,
                                                Err(e) => Message::Notification(Toast::warn(format!(
                                                    "Push: {e}"
                                                ))),
                                            },
                                        ));
                                    }
                                }
                            }
                        }

                        if toast_msg.is_some() || fill_msg.is_some() {
                            let mut msg = format!(
                                "[Rule] {}: {}",
                                rule.name,
                                toast_msg.unwrap_or_else(|| "Triggered".to_string())
                            );
                            if let Some(fill) = fill_msg {
                                msg = format!("{msg} | {fill}");
                            }
                            state.push_notification(Toast::new(Notification::Info(msg)));
                        }

                        // persistent in-session log (survives toast timeout)
                        state.push_rule_log(rule, parts.join(" | "));
                    }
                }
            }
        });

        Task::batch(tasks)
    }

    pub fn resolve_streams(
        &mut self,
        main_window: window::Id,
        pane_id: uuid::Uuid,
        streams: Vec<StreamKind>,
    ) -> Task<Message> {
        if let Some(state) = self.get_mut_pane_state_by_uuid(main_window, pane_id) {
            state.streams = ResolvedStream::Ready(streams.clone());
        }
        self.refresh_streams(main_window)
    }

    pub fn market_subscriptions(&self) -> Subscription<exchange::Event> {
        let unique_streams = self
            .streams
            .combined_used()
            .flat_map(|(exchange, specs)| {
                let mut subs = vec![];

                if !specs.depth.is_empty() {
                    let depth_subs = specs
                        .depth
                        .iter()
                        .map(|(ticker, aggr, push_freq)| {
                            let tick_mltp = match aggr {
                                StreamTicksize::Client => None,
                                StreamTicksize::ServerSide(tick_mltp) => Some(*tick_mltp),
                            };
                            depth_subscription(*ticker, tick_mltp, *push_freq)
                        })
                        .collect::<Vec<_>>();

                    if !depth_subs.is_empty() {
                        subs.push(Subscription::batch(depth_subs));
                    }
                }

                let kline_params = specs
                    .kline
                    .iter()
                    .map(|(ticker, timeframe)| (*ticker, *timeframe))
                    .collect::<Vec<_>>();

                if !kline_params.is_empty() {
                    subs.push(kline_subscription(exchange, kline_params));
                }

                subs
            })
            .collect::<Vec<Subscription<exchange::Event>>>();

        Subscription::batch(unique_streams)
    }

    fn refresh_streams(&mut self, main_window: window::Id) -> Task<Message> {
        let all_pane_streams = self
            .iter_all_panes(main_window)
            .flat_map(|(_, _, pane_state)| pane_state.streams.ready_iter().into_iter().flatten());
        self.streams = UniqueStreams::from(all_pane_streams);

        Task::none()
    }
}

fn request_fetch(
    state: &mut pane::State,
    layout_id: uuid::Uuid,
    req_id: uuid::Uuid,
    fetch: FetchRange,
    stream: Option<StreamKind>,
) -> Task<Message> {
    let pane_id = state.unique_id();

    match fetch {
        FetchRange::Kline(from, to) => {
            let kline_stream = {
                if let Some(s) = stream {
                    Some((s, pane_id))
                } else {
                    state.streams.find_ready_map(|stream| {
                        if let StreamKind::Kline { .. } = stream {
                            Some((*stream, pane_id))
                        } else {
                            None
                        }
                    })
                }
            };

            if let Some((stream, pane_uid)) = kline_stream {
                return kline_fetch_task(
                    layout_id,
                    pane_uid,
                    stream,
                    Some(req_id),
                    Some((from, to)),
                );
            }
        }
        FetchRange::OpenInterest(from, to) => {
            let kline_stream = {
                if let Some(s) = stream {
                    Some((s, pane_id))
                } else {
                    state.streams.find_ready_map(|stream| {
                        if let StreamKind::Kline { .. } = stream {
                            Some((*stream, pane_id))
                        } else {
                            None
                        }
                    })
                }
            };

            if let Some((stream, pane_uid)) = kline_stream {
                return oi_fetch_task(layout_id, pane_uid, stream, Some(req_id), Some((from, to)));
            }
        }
        FetchRange::Trades(from_time, to_time) => {
            let trade_info = state.streams.find_ready_map(|stream| {
                if let StreamKind::DepthAndTrades { ticker_info, .. } = stream {
                    Some((*ticker_info, pane_id, *stream))
                } else {
                    None
                }
            });

            if let Some((ticker_info, pane_id, stream)) = trade_info {
                let is_binance = matches!(
                    ticker_info.exchange(),
                    Exchange::BinanceSpot | Exchange::BinanceLinear | Exchange::BinanceInverse
                );

                if is_binance {
                    let data_path = data::data_path(Some("market_data/binance/"));

                    let (task, handle) = Task::sip(
                        fetch_trades_batched(ticker_info, from_time, to_time, data_path),
                        move |batch| {
                            let data = FetchedData::Trades {
                                batch,
                                until_time: to_time,
                            };
                            Message::DistributeFetchedData {
                                layout_id,
                                pane_id,
                                data,
                                stream,
                            }
                        },
                        move |result| match result {
                            Ok(()) => Message::ChangePaneStatus(pane_id, pane::Status::Ready),
                            Err(err) => Message::ErrorOccurred(
                                Some(pane_id),
                                DashboardError::Fetch(err.to_string()),
                            ),
                        },
                    )
                    .abortable();

                    if let pane::Content::Kline { chart, .. } = &mut state.content
                        && let Some(c) = chart
                    {
                        c.set_handle(handle.abort_on_drop());
                    }

                    return task;
                }
            }
        }
    }

    Task::none()
}

fn request_fetch_many(
    state: &mut pane::State,
    layout_id: uuid::Uuid,
    reqs: impl IntoIterator<Item = (uuid::Uuid, FetchRange, Option<StreamKind>)>,
) -> Task<Message> {
    let tasks = reqs
        .into_iter()
        .map(|(req_id, fetch, stream)| request_fetch(state, layout_id, req_id, fetch, stream))
        .collect::<Vec<_>>();
    Task::batch(tasks)
}

fn oi_fetch_task(
    layout_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    stream: StreamKind,
    req_id: Option<uuid::Uuid>,
    range: Option<(u64, u64)>,
) -> Task<Message> {
    let update_status = Task::done(Message::ChangePaneStatus(
        pane_id,
        pane::Status::Loading(exchange::fetcher::InfoKind::FetchingOI),
    ));

    let fetch_task = match stream {
        StreamKind::Kline {
            ticker_info,
            timeframe,
        } => Task::perform(
            adapter::fetch_open_interest(ticker_info.ticker, timeframe, range)
                .map_err(|err| format!("{err}")),
            move |result| match result {
                Ok(oi) => {
                    let data = FetchedData::OI { data: oi, req_id };
                    Message::DistributeFetchedData {
                        layout_id,
                        pane_id,
                        data,
                        stream,
                    }
                }
                Err(err) => Message::ErrorOccurred(Some(pane_id), DashboardError::Fetch(err)),
            },
        ),
        _ => Task::none(),
    };

    update_status.chain(fetch_task)
}

fn kline_fetch_task(
    layout_id: uuid::Uuid,
    pane_id: uuid::Uuid,
    stream: StreamKind,
    req_id: Option<uuid::Uuid>,
    range: Option<(u64, u64)>,
) -> Task<Message> {
    let update_status = Task::done(Message::ChangePaneStatus(
        pane_id,
        pane::Status::Loading(exchange::fetcher::InfoKind::FetchingKlines),
    ));

    let fetch_task = match stream {
        StreamKind::Kline {
            ticker_info,
            timeframe,
        } => Task::perform(
            adapter::fetch_klines(ticker_info, timeframe, range)
                .map_err(|err| err.to_user_message()),
            move |result| match result {
                Ok(klines) => {
                    let data = FetchedData::Klines {
                        data: klines,
                        req_id,
                    };
                    Message::DistributeFetchedData {
                        layout_id,
                        pane_id,
                        data,
                        stream,
                    }
                }
                Err(err) => {
                    Message::ErrorOccurred(Some(pane_id), DashboardError::Fetch(err.to_string()))
                }
            },
        ),
        _ => Task::none(),
    };

    update_status.chain(fetch_task)
}

pub fn fetch_trades_batched(
    ticker_info: TickerInfo,
    from_time: u64,
    to_time: u64,
    data_path: PathBuf,
) -> impl Straw<(), Vec<Trade>, AdapterError> {
    sipper(async move |mut progress| {
        let mut latest_trade_t = from_time;

        while latest_trade_t < to_time {
            match binance::fetch_trades(ticker_info, latest_trade_t, data_path.clone()).await {
                Ok(batch) => {
                    if batch.is_empty() {
                        break;
                    }

                    latest_trade_t = batch.last().map_or(latest_trade_t, |trade| trade.time);

                    let () = progress.send(batch).await;
                }
                Err(err) => return Err(err),
            }
        }

        Ok(())
    })
}

pub fn depth_subscription(
    ticker_info: TickerInfo,
    tick_mlpt: Option<TickMultiplier>,
    push_freq: PushFrequency,
) -> Subscription<exchange::Event> {
    let exchange = ticker_info.exchange();

    let config = StreamConfig::new(ticker_info, exchange, tick_mlpt, push_freq);

    match exchange {
        Exchange::BinanceSpot | Exchange::BinanceInverse | Exchange::BinanceLinear => {
            let builder = |cfg: &StreamConfig<TickerInfo>| {
                binance::connect_market_stream(cfg.id, cfg.push_freq)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::BybitSpot | Exchange::BybitLinear | Exchange::BybitInverse => {
            let builder = |cfg: &StreamConfig<TickerInfo>| {
                bybit::connect_market_stream(cfg.id, cfg.push_freq)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::HyperliquidSpot | Exchange::HyperliquidLinear => {
            let builder = |cfg: &StreamConfig<TickerInfo>| {
                hyperliquid::connect_market_stream(cfg.id, cfg.tick_mltp, cfg.push_freq)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::OkexLinear | Exchange::OkexInverse | Exchange::OkexSpot => {
            let builder =
                |cfg: &StreamConfig<TickerInfo>| okex::connect_market_stream(cfg.id, cfg.push_freq);
            Subscription::run_with(config, builder)
        }
    }
}

pub fn kline_subscription(
    exchange: Exchange,
    kline_subs: Vec<(TickerInfo, Timeframe)>,
) -> Subscription<exchange::Event> {
    let config = StreamConfig::new(kline_subs, exchange, None, PushFrequency::ServerDefault);
    match exchange {
        Exchange::BinanceSpot | Exchange::BinanceInverse | Exchange::BinanceLinear => {
            let builder = |cfg: &StreamConfig<Vec<(TickerInfo, Timeframe)>>| {
                binance::connect_kline_stream(cfg.id.clone(), cfg.market_type)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::BybitSpot | Exchange::BybitInverse | Exchange::BybitLinear => {
            let builder = |cfg: &StreamConfig<Vec<(TickerInfo, Timeframe)>>| {
                bybit::connect_kline_stream(cfg.id.clone(), cfg.market_type)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::HyperliquidSpot | Exchange::HyperliquidLinear => {
            let builder = |cfg: &StreamConfig<Vec<(TickerInfo, Timeframe)>>| {
                hyperliquid::connect_kline_stream(cfg.id.clone(), cfg.market_type)
            };
            Subscription::run_with(config, builder)
        }
        Exchange::OkexLinear | Exchange::OkexInverse | Exchange::OkexSpot => {
            let builder = |cfg: &StreamConfig<Vec<(TickerInfo, Timeframe)>>| {
                okex::connect_kline_stream(cfg.id.clone(), cfg.market_type)
            };
            Subscription::run_with(config, builder)
        }
    }
}
