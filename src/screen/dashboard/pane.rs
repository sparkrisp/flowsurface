use crate::{
    chart::{self, comparison::ComparisonChart, heatmap::HeatmapChart, kline::KlineChart},
    modal::{
        self, ModifierKind,
        pane::{
            Modal,
            mini_tickers_list::MiniPanel,
            settings::{comparison_cfg_view, heatmap_cfg_view, kline_cfg_view},
            stack_context_menu,
            stack_modal,
        },
    },
    screen::dashboard::{
        panel::{self, ladder::Ladder, timeandsales::TimeAndSales},
        tickers_table::TickersTable,
    },
    style::{self, Icon, icon_text},
    widget::{self, button_with_tooltip, column_drag, link_group_button, toast::Toast},
    window::{self, Window},
};
use data::{
    UserTimezone,
    chart::{
        Basis, ViewConfig,
        indicator::{HeatmapIndicator, Indicator, KlineIndicator, UiIndicator},
    },
    layout::pane::{ContentKind, LinkGroup, PaneSetup, Settings, VisualConfig},
    rules::{EvaluationMode, RuleAction, RuleCondition, RuleSpec},
};
use exchange::{
    Kline, OpenInterest, StreamPairKind, TickMultiplier, TickerInfo, Timeframe,
    adapter::{MarketKind, PersistStreamKind, ResolvedStream, StreamKind, StreamTicksize},
    fetcher::FetchRequests,
};
use iced::{
    Alignment, Element, Length, Renderer, Theme,
    alignment::Vertical,
    padding,
    widget::{button, center, column, container, pane_grid, pick_list, row, text, tooltip},
};
use chrono::Local;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[derive(Debug, Clone)]
pub enum Effect {
    RefreshStreams,
    RequestFetch(FetchRequests),
    SwitchTickersInGroup(TickerInfo),
    FocusWidget(iced::widget::Id),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum Status {
    #[default]
    Ready,
    Loading(exchange::fetcher::InfoKind),
    Stale(String),
}

pub enum Action {
    Chart(chart::Action),
    Panel(panel::Action),
    ResolveStreams(Vec<PersistStreamKind>),
    ResolveContent,
}

#[derive(Debug, Clone)]
pub enum Message {
    PaneClicked(pane_grid::Pane),
    PaneResized(pane_grid::ResizeEvent),
    PaneDragged(pane_grid::DragEvent),
    ClosePane(pane_grid::Pane),
    SplitPane(pane_grid::Axis, pane_grid::Pane),
    MaximizePane(pane_grid::Pane),
    Restore,
    ReplacePane(pane_grid::Pane),
    Popout,
    Merge,
    SwitchLinkGroup(pane_grid::Pane, Option<LinkGroup>),
    VisualConfigChanged(pane_grid::Pane, VisualConfig, bool),
    PaneEvent(pane_grid::Pane, Event),
}

#[derive(Debug, Clone)]
pub enum Event {
    ShowModal(Modal),
    HideModal,
    ContentSelected(ContentKind),
    ChartInteraction(super::chart::Message),
    PanelInteraction(super::panel::Message),
    ToggleIndicator(UiIndicator),
    DeleteNotification(usize),
    ReorderIndicator(column_drag::DragEvent),
    ClusterKindSelected(data::chart::kline::ClusterKind),
    ClusterScalingSelected(data::chart::kline::ClusterScaling),
    StudyConfigurator(modal::pane::settings::study::StudyMessage),
    StreamModifierChanged(modal::stream::Message),
    ComparisonChartInteraction(super::chart::comparison::Message),
    MiniTickersListInteraction(modal::pane::mini_tickers_list::Message),

    // Rules (candlestick/kline panes)
    AddRule,
    DeleteRule(uuid::Uuid),
    ToggleRule(uuid::Uuid, bool),
    ToggleRuleCard(uuid::Uuid),
    UpdateRuleName(uuid::Uuid, String),
    UpdateRuleEvaluation(uuid::Uuid, EvaluationMode),
    UpdateRuleConditionKind(uuid::Uuid, modal::pane::rules::ConditionKind),
    UpdateRuleCrossDirection(uuid::Uuid, data::rules::CrossDirection),
    UpdateRuleCompareDirection(uuid::Uuid, data::rules::CompareDirection),
    UpdateRuleLevel(uuid::Uuid, String),
    ToggleRuleActionToast(uuid::Uuid, bool),
    UpdateRuleToastMessage(uuid::Uuid, String),
    ToggleRuleActionSound(uuid::Uuid, bool),
    ToggleRuleActionTelegram(uuid::Uuid, bool),
    ToggleRuleActionPush(uuid::Uuid, bool),
    ToggleRuleActionPaperTrade(uuid::Uuid, bool),
    UpdateRulePaperPercent(uuid::Uuid, String),
    AddRuleAndOpen,
    ToggleSettingsColors(SettingsColorsSection),
    IndicatorsQueryChanged(String),
    IndicatorsSidebarSelected(IndicatorsSidebar),
    IndicatorsSourceSelected(IndicatorsSource),

    // Rule trigger log
    ClearRuleLog,

    // Chart navigation
    GoToLatest,

    // Centered modal resizing (window-level)
    ResizeCenteredModal(CenteredModalKind, f32, f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CenteredModalKind {
    Rules,
    Indicators,
    RuleLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsColorsSection {
    Rsi,
    Macd,
    Atr,
    StochRsi,
    DmiAdx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndicatorsSidebar {
    Enabled,
    Volume,
    Derivatives,
    Oscillators,
    Trend,
    Volatility,
    Overlays,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndicatorsSource {
    BuiltIn,
    Community,
}

pub struct State {
    id: uuid::Uuid,
    pub modal: Option<Modal>,
    pub content: Content,
    pub settings: Settings,
    pub notifications: Vec<Toast>,
    pub streams: ResolvedStream,
    pub status: Status,
    pub link_group: Option<LinkGroup>,

    // Rule system (per pane, currently only meaningful for kline/candlestick)
    pub rules: Vec<RuleSpec>,
    pub(crate) rules_expanded: Option<uuid::Uuid>,
    pub rule_log: Vec<RuleLogEntry>,
    pub paper: PaperAccount,
    rule_last_triggered_ms: HashMap<uuid::Uuid, u64>,
    pub settings_colors_expanded: HashSet<SettingsColorsSection>,
    pub indicators_query: String,
    pub indicators_sidebar: IndicatorsSidebar,
    pub indicators_source: IndicatorsSource,
    pub(crate) last_trade_price: Option<f32>,
    pub(crate) prev_trade_price: Option<f32>,
    pub(crate) pending_candle_close: Option<(u64, f32, f32)>, // (time, close, volume_total)

    // Performance: rule evaluation throttling for OnTick (to avoid doing heavy indicator math at frame-rate)
    pub rule_tick_dirty: bool,
    pub rule_tick_last_eval: Instant,

    // Window-level modal sizes (per pane)
    pub centered_rules_size: (f32, f32),
    pub centered_indicators_size: (f32, f32),
    pub centered_rule_log_size: (f32, f32),
}

#[derive(Debug, Clone, Copy)]
pub struct PaperAccount {
    pub balance_quote: f32,
    pub position_base: f32,
}

impl Default for PaperAccount {
    fn default() -> Self {
        Self {
            balance_quote: 10_000.0,
            position_base: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuleLogEntry {
    pub time_hms: String,
    pub rule_id: uuid::Uuid,
    pub rule_name: String,
    pub message: String,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_config(
        content: Content,
        streams: Vec<PersistStreamKind>,
        settings: Settings,
        link_group: Option<LinkGroup>,
        rules: Vec<RuleSpec>,
    ) -> Self {
        Self {
            content,
            settings,
            streams: ResolvedStream::Waiting(streams),
            link_group,
            rules,
            ..Default::default()
        }
    }

    fn upsert_action(rule: &mut RuleSpec, action: RuleAction, enabled: bool) {
        let discr = std::mem::discriminant(&action);
        let exists = rule.actions.iter().any(|a| std::mem::discriminant(a) == discr);
        if enabled && !exists {
            rule.actions.push(action);
        } else if !enabled && exists {
            rule.actions.retain(|a| std::mem::discriminant(a) != discr);
        }
    }

    fn set_toast_message(rule: &mut RuleSpec, msg: String) {
        for a in &mut rule.actions {
            if let RuleAction::Toast { message } = a {
                *message = msg;
                return;
            }
        }
        rule.actions.push(RuleAction::Toast { message: msg });
    }

    pub fn push_rule_log(&mut self, rule: &RuleSpec, message: String) {
        let time_hms = Local::now().format("%H:%M:%S").to_string();

        self.rule_log.push(RuleLogEntry {
            time_hms,
            rule_id: rule.id,
            rule_name: rule.name.clone(),
            message,
        });

        const MAX: usize = 1_000;
        if self.rule_log.len() > MAX {
            let drain = self.rule_log.len() - MAX;
            self.rule_log.drain(0..drain);
        }
    }

    pub fn push_notification(&mut self, toast: Toast) {
        self.notifications.push(toast);
        const MAX: usize = 200;
        if self.notifications.len() > MAX {
            let drain = self.notifications.len() - MAX;
            self.notifications.drain(0..drain);
        }
    }

    pub fn cooldown_allows(&mut self, rule: &RuleSpec, now_ms: u64) -> bool {
        if rule.cooldown_ms == 0 {
            // still store the last trigger to support future enhancements
            self.rule_last_triggered_ms.insert(rule.id, now_ms);
            return true;
        }
        if let Some(last) = self.rule_last_triggered_ms.get(&rule.id) {
            if now_ms.saturating_sub(*last) < rule.cooldown_ms {
                return false;
            }
        }
        self.rule_last_triggered_ms.insert(rule.id, now_ms);
        true
    }

    pub fn on_trades_buffer(&mut self, trades_buffer: &[exchange::Trade]) {
        if let Some(last) = trades_buffer.last() {
            self.prev_trade_price = self.last_trade_price;
            self.last_trade_price = Some(last.price.to_f32_lossy());
            self.rule_tick_dirty = true;
        }
    }

    pub fn current_price(&self) -> Option<f32> {
        self.last_trade_price
    }

    pub fn paper_trade(&mut self, side: data::rules::Side, pct: f32, price: f32) -> Option<String> {
        if !price.is_finite() || price <= 0.0 {
            return None;
        }
        let pct = pct.clamp(0.0, 100.0);
        match side {
            data::rules::Side::Buy => {
                let spend = self.paper.balance_quote * (pct / 100.0);
                if spend <= 0.0 {
                    return None;
                }
                let qty = spend / price;
                self.paper.balance_quote -= spend;
                self.paper.position_base += qty;
                Some(format!(
                    "paper fill BUY {:.6} @ {:.4} (spent {:.2}, bal {:.2}, pos {:.6})",
                    qty, price, spend, self.paper.balance_quote, self.paper.position_base
                ))
            }
            data::rules::Side::Sell => {
                let qty = self.paper.position_base * (pct / 100.0);
                if qty <= 0.0 {
                    return None;
                }
                let gain = qty * price;
                self.paper.position_base -= qty;
                self.paper.balance_quote += gain;
                Some(format!(
                    "paper fill SELL {:.6} @ {:.4} (gain {:.2}, bal {:.2}, pos {:.6})",
                    qty, price, gain, self.paper.balance_quote, self.paper.position_base
                ))
            }
        }
    }

    pub fn zoom_focused_chart(&mut self, delta_y: f32) {
        match &mut self.content {
            Content::Kline { chart, .. } => {
                if let Some(c) = chart.as_mut() {
                    crate::chart::zoom_center(c, delta_y);
                }
            }
            Content::Heatmap { chart, .. } => {
                if let Some(c) = chart.as_mut() {
                    crate::chart::zoom_center(c, delta_y);
                }
            }
            _ => {}
        }
    }

    pub fn on_kline_update(&mut self, kline: &exchange::Kline) {
        let total_vol = kline.volume.0 + kline.volume.1;
        self.pending_candle_close = Some((kline.time, kline.close.to_f32_lossy(), total_vol));
    }

    pub(crate) fn eval_condition_tick(&self, rule: &RuleSpec) -> bool {
        // helper: get the kline chart if present
        let chart = match &self.content {
            Content::Kline { chart, .. } => chart.as_ref(),
            _ => None,
        };

        match &rule.condition {
            RuleCondition::PriceCrossLevel { level, direction } => {
                let (Some(prev), Some(cur)) = (self.prev_trade_price, self.last_trade_price) else {
                    return false;
                };
                match direction {
                    data::rules::CrossDirection::CrossUp => prev < *level && cur >= *level,
                    data::rules::CrossDirection::CrossDown => prev > *level && cur <= *level,
                }
            }
            RuleCondition::VwapCross { direction } => {
                let Some(chart) = chart else { return false; };
                let (Some(prev), Some(cur)) = (self.prev_trade_price, self.last_trade_price) else {
                    return false;
                };
                let Some(cfg) = vwap_cfg_from_kind(chart.kind()) else { return false; };
                let ohlcv = chart.ohlcv_series();
                let Some((_, vwap)) = vwap_last_two(&ohlcv, cfg.reset_daily_utc) else { return false; };
                match direction {
                    data::rules::CrossDirection::CrossUp => prev < vwap && cur >= vwap,
                    data::rules::CrossDirection::CrossDown => prev > vwap && cur <= vwap,
                }
            }
            RuleCondition::SupertrendLineCross { direction } => {
                let Some(chart) = chart else { return false; };
                let (Some(prev), Some(cur)) = (self.prev_trade_price, self.last_trade_price) else {
                    return false;
                };
                let Some(cfg) = supertrend_cfg_from_kind(chart.kind()) else { return false; };
                let ohlcv = chart.ohlcv_series();
                let Some((_, (_, line))) =
                    supertrend_last_two(&ohlcv, cfg.atr_period, cfg.multiplier_x100)
                else {
                    return false;
                };
                match direction {
                    data::rules::CrossDirection::CrossUp => prev < line && cur >= line,
                    data::rules::CrossDirection::CrossDown => prev > line && cur <= line,
                }
            }
            RuleCondition::DonchianBreakout { direction } => {
                let Some(chart) = chart else { return false; };
                let (Some(prev), Some(cur)) = (self.prev_trade_price, self.last_trade_price) else {
                    return false;
                };
                let Some(cfg) = donchian_cfg_from_kind(chart.kind()) else { return false; };
                let ohlcv = chart.ohlcv_series();
                let Some((_, (upper, lower))) = donchian_last_two(&ohlcv, cfg.period) else {
                    return false;
                };
                match direction {
                    data::rules::CrossDirection::CrossUp => prev < upper && cur >= upper,
                    data::rules::CrossDirection::CrossDown => prev > lower && cur <= lower,
                }
            }
            RuleCondition::KeltnerBreakout { direction } => {
                let Some(chart) = chart else { return false; };
                let (Some(prev), Some(cur)) = (self.prev_trade_price, self.last_trade_price) else {
                    return false;
                };
                let Some(cfg) = keltner_cfg_from_kind(chart.kind()) else { return false; };
                let ohlcv = chart.ohlcv_series();
                let Some((_, (upper, lower))) = keltner_last_two(
                    &ohlcv,
                    cfg.ema_period,
                    cfg.atr_period,
                    cfg.multiplier_x100,
                ) else {
                    return false;
                };
                match direction {
                    data::rules::CrossDirection::CrossUp => prev < upper && cur >= upper,
                    data::rules::CrossDirection::CrossDown => prev > lower && cur <= lower,
                }
            }
            RuleCondition::MovingAverageCross { direction } => {
                let Some(chart) = chart else { return false; };
                let closes = chart.close_series();
                let Some((fast, slow)) = ma_pair_from_kind(chart.kind()) else { return false; };

                let Some((pf, cf)) = ma_last_two(&closes, fast.kind, fast.period) else { return false; };
                let Some((ps, cs)) = ma_last_two(&closes, slow.kind, slow.period) else { return false; };

                match direction {
                    data::rules::CrossDirection::CrossUp => pf < ps && cf >= cs,
                    data::rules::CrossDirection::CrossDown => pf > ps && cf <= cs,
                }
            }
            RuleCondition::RsiCrossLevel { level, direction } => {
                let Some(chart) = chart else { return false; };
                let closes = chart.close_series();
                let cfg = chart.visual_config();
                let Some((prev_rsi, cur_rsi)) = rsi_last_two(&closes, cfg.rsi_period) else {
                    return false;
                };
                match direction {
                    data::rules::CrossDirection::CrossUp => prev_rsi < *level && cur_rsi >= *level,
                    data::rules::CrossDirection::CrossDown => prev_rsi > *level && cur_rsi <= *level,
                }
            }
            RuleCondition::MacdCrossSignal { direction } => {
                let Some(chart) = chart else { return false; };
                let closes = chart.close_series();
                let cfg = chart.visual_config();
                let Some(((pm, ps), (cm, cs))) =
                    macd_last_two(&closes, cfg.macd_fast, cfg.macd_slow, cfg.macd_signal)
                else {
                    return false;
                };
                match direction {
                    data::rules::CrossDirection::CrossUp => pm < ps && cm >= cs,
                    data::rules::CrossDirection::CrossDown => pm > ps && cm <= cs,
                }
            }
            // event-style conditions based on candle math: evaluate only on candle close to avoid re-triggering on every tick
            RuleCondition::SupertrendFlip { .. } | RuleCondition::DmiCross { .. } | RuleCondition::AdxIs { .. } => {
                false
            }
            _ => false,
        }
    }

    pub(crate) fn eval_condition_candle_close(&self, rule: &RuleSpec, _close: f32, vol: f32) -> bool {
        let chart = match &self.content {
            Content::Kline { chart, .. } => chart.as_ref(),
            _ => None,
        };
        match &rule.condition {
            RuleCondition::CandleCloseCrossLevel { level, direction } => {
                let Some(chart) = chart else { return false; };
                let Some((prev_close, cur_close)) = chart.last_two_closes() else { return false; };
                match direction {
                    data::rules::CrossDirection::CrossUp => prev_close < *level && cur_close >= *level,
                    data::rules::CrossDirection::CrossDown => prev_close > *level && cur_close <= *level,
                }
            }
            RuleCondition::VolumeIs { value, direction } => match direction {
                data::rules::CompareDirection::Above => vol >= *value,
                data::rules::CompareDirection::Below => vol <= *value,
            },
            RuleCondition::VwapCross { direction } => {
                let Some(chart) = chart else { return false; };
                let Some(cfg) = vwap_cfg_from_kind(chart.kind()) else { return false; };
                let ohlcv = chart.ohlcv_series();
                let Some((pvwap, cvwap)) = vwap_last_two(&ohlcv, cfg.reset_daily_utc) else {
                    return false;
                };
                let Some((pclose, cclose)) = chart.last_two_closes() else { return false; };
                match direction {
                    data::rules::CrossDirection::CrossUp => pclose < pvwap && cclose >= cvwap,
                    data::rules::CrossDirection::CrossDown => pclose > pvwap && cclose <= cvwap,
                }
            }
            RuleCondition::SupertrendFlip { direction } => {
                let Some(chart) = chart else { return false; };
                let Some(cfg) = supertrend_cfg_from_kind(chart.kind()) else { return false; };
                let ohlcv = chart.ohlcv_series();
                let Some(((p_up, _), (c_up, _))) =
                    supertrend_last_two(&ohlcv, cfg.atr_period, cfg.multiplier_x100)
                else {
                    return false;
                };
                match direction {
                    data::rules::CrossDirection::CrossUp => !p_up && c_up,
                    data::rules::CrossDirection::CrossDown => p_up && !c_up,
                }
            }
            RuleCondition::SupertrendLineCross { direction } => {
                let Some(chart) = chart else { return false; };
                let Some(cfg) = supertrend_cfg_from_kind(chart.kind()) else { return false; };
                let ohlcv = chart.ohlcv_series();
                let Some(((_, pline), (_, cline))) =
                    supertrend_last_two(&ohlcv, cfg.atr_period, cfg.multiplier_x100)
                else {
                    return false;
                };
                let Some((pclose, cclose)) = chart.last_two_closes() else { return false; };
                match direction {
                    data::rules::CrossDirection::CrossUp => pclose < pline && cclose >= cline,
                    data::rules::CrossDirection::CrossDown => pclose > pline && cclose <= cline,
                }
            }
            RuleCondition::DonchianBreakout { direction } => {
                let Some(chart) = chart else { return false; };
                let Some(cfg) = donchian_cfg_from_kind(chart.kind()) else { return false; };
                let ohlcv = chart.ohlcv_series();
                let Some(((pupper, plower), (cupper, clower))) = donchian_last_two(&ohlcv, cfg.period) else {
                    return false;
                };
                let Some((pclose, cclose)) = chart.last_two_closes() else { return false; };
                match direction {
                    data::rules::CrossDirection::CrossUp => pclose < pupper && cclose >= cupper,
                    data::rules::CrossDirection::CrossDown => pclose > plower && cclose <= clower,
                }
            }
            RuleCondition::KeltnerBreakout { direction } => {
                let Some(chart) = chart else { return false; };
                let Some(cfg) = keltner_cfg_from_kind(chart.kind()) else { return false; };
                let ohlcv = chart.ohlcv_series();
                let Some(((pupper, plower), (cupper, clower))) = keltner_last_two(
                    &ohlcv,
                    cfg.ema_period,
                    cfg.atr_period,
                    cfg.multiplier_x100,
                ) else {
                    return false;
                };
                let Some((pclose, cclose)) = chart.last_two_closes() else { return false; };
                match direction {
                    data::rules::CrossDirection::CrossUp => pclose < pupper && cclose >= cupper,
                    data::rules::CrossDirection::CrossDown => pclose > plower && cclose <= clower,
                }
            }
            RuleCondition::DmiCross { direction } => {
                let Some(chart) = chart else { return false; };
                let cfg = chart.visual_config();
                let ohlcv = chart.ohlcv_series();
                let Some(((p_plus, p_minus, _), (c_plus, c_minus, _))) =
                    dmi_adx_last_two(&ohlcv, cfg.dmi_period)
                else {
                    return false;
                };
                match direction {
                    data::rules::CrossDirection::CrossUp => p_plus < p_minus && c_plus >= c_minus,
                    data::rules::CrossDirection::CrossDown => p_plus > p_minus && c_plus <= c_minus,
                }
            }
            RuleCondition::AdxIs { value, direction } => {
                let Some(chart) = chart else { return false; };
                let cfg = chart.visual_config();
                let ohlcv = chart.ohlcv_series();
                let Some(((_, _, p_adx), (_, _, c_adx))) = dmi_adx_last_two(&ohlcv, cfg.dmi_period) else {
                    return false;
                };
                match direction {
                    // treat as a threshold CROSS to avoid spamming every candle while condition holds
                    data::rules::CompareDirection::Above => p_adx < *value && c_adx >= *value,
                    data::rules::CompareDirection::Below => p_adx > *value && c_adx <= *value,
                }
            }
            // for candle-close evaluation, reuse the same computations based on latest chart state
            RuleCondition::MovingAverageCross { .. }
            | RuleCondition::RsiCrossLevel { .. }
            | RuleCondition::MacdCrossSignal { .. } => self.eval_condition_tick(rule),
            _ => false,
        }
    }

    pub fn stream_pair(&self) -> Option<TickerInfo> {
        self.streams.find_ready_map(|stream| match stream {
            StreamKind::DepthAndTrades { ticker_info, .. }
            | StreamKind::Kline { ticker_info, .. } => Some(*ticker_info),
        })
    }

    pub fn stream_pair_kind(&self) -> Option<StreamPairKind> {
        let ready_streams = self.streams.ready_iter()?;
        let mut unique = vec![];

        for stream in ready_streams {
            let ticker = stream.ticker_info();
            if !unique.contains(&ticker) {
                unique.push(ticker);
            }
        }

        match unique.len() {
            0 => None,
            1 => Some(StreamPairKind::SingleSource(unique[0])),
            _ => Some(StreamPairKind::MultiSource(unique)),
        }
    }

    pub fn set_content_and_streams(
        &mut self,
        tickers: Vec<TickerInfo>,
        kind: ContentKind,
    ) -> Vec<StreamKind> {
        if !(self.content.kind() == kind) {
            self.settings.selected_basis = None;
            self.settings.tick_multiply = None;
        }

        let base_ticker = tickers[0];
        let prev_base_ticker = self.stream_pair();

        let derived_plan = PaneSetup::new(
            kind,
            base_ticker,
            prev_base_ticker,
            self.settings.selected_basis,
            self.settings.tick_multiply,
        );

        let mut selected_basis = derived_plan.basis;
        let heatmap_cfg = match kind {
            ContentKind::HeatmapChart | ContentKind::CandlesHeatmapChart => {
                let cfg = self.settings.visual_config.clone().and_then(|cfg| cfg.heatmap());
                if let Some(cfg) = cfg {
                    if cfg.show_candles && cfg.sync_heatmap_to_candles {
                        if let Some(tf) = resolve_heatmap_candle_tf(cfg, selected_basis) {
                            selected_basis = Some(Basis::Time(tf));
                        }
                    }
                }
                cfg
            }
            _ => None,
        };

        self.settings.selected_basis = selected_basis;
        self.settings.tick_multiply = derived_plan.tick_multiplier;

        let (content, streams) = {
            let kline_stream = |ti: TickerInfo, tf: Timeframe| StreamKind::Kline {
                ticker_info: ti,
                timeframe: tf,
            };
            let depth_stream = |derived_plan: &PaneSetup| StreamKind::DepthAndTrades {
                ticker_info: derived_plan.ticker_info,
                depth_aggr: derived_plan.depth_aggr,
                push_freq: derived_plan.push_freq,
            };

            match kind {
                ContentKind::HeatmapChart | ContentKind::CandlesHeatmapChart => {
                    let content = Content::new_heatmap(
                        &self.content,
                        derived_plan.ticker_info,
                        &self.settings,
                        derived_plan.tick_size,
                    );

                    let streams = {
                        let wants_candles = heatmap_cfg.map(|c| c.show_candles).unwrap_or(false);
                        let candle_tf = heatmap_cfg
                            .and_then(|cfg| resolve_heatmap_candle_tf(cfg, selected_basis));
                        if wants_candles && matches!(selected_basis, Some(Basis::Time(_))) {
                            if let Some(k_tf) = candle_tf {
                                vec![
                                    depth_stream(&derived_plan),
                                    kline_stream(derived_plan.ticker_info, k_tf),
                                ]
                            } else {
                                vec![depth_stream(&derived_plan)]
                            }
                        } else {
                            vec![depth_stream(&derived_plan)]
                        }
                    };

                    (content, streams)
                }
                ContentKind::FootprintChart => {
                    let content = Content::new_kline(
                        kind,
                        &self.content,
                        derived_plan.ticker_info,
                        &self.settings,
                        derived_plan.tick_size,
                    );

                    let streams = by_basis_default(
                        derived_plan.basis,
                        Timeframe::M5,
                        |tf| {
                            vec![
                                depth_stream(&derived_plan),
                                kline_stream(derived_plan.ticker_info, tf),
                            ]
                        },
                        || vec![depth_stream(&derived_plan)],
                    );

                    (content, streams)
                }
                ContentKind::CandlestickChart => {
                    let content = {
                        let base_ticker = tickers[0];
                        Content::new_kline(
                            kind,
                            &self.content,
                            derived_plan.ticker_info,
                            &self.settings,
                            base_ticker.min_ticksize.into(),
                        )
                    };

                    let streams = by_basis_default(
                        derived_plan.basis,
                        Timeframe::M15,
                        |tf| vec![kline_stream(derived_plan.ticker_info, tf)],
                        || {
                            let depth_aggr = derived_plan
                                .ticker_info
                                .exchange()
                                .stream_ticksize(None, TickMultiplier(50));
                            let temp = PaneSetup {
                                depth_aggr,
                                ..derived_plan
                            };
                            vec![depth_stream(&temp)]
                        },
                    );

                    (content, streams)
                }
                ContentKind::TimeAndSales => {
                    let config = self
                        .settings
                        .visual_config
                        .clone()
                        .and_then(|cfg| cfg.time_and_sales());
                    let content = Content::TimeAndSales(Some(TimeAndSales::new(
                        config,
                        derived_plan.ticker_info,
                    )));

                    let temp = PaneSetup {
                        push_freq: exchange::PushFrequency::ServerDefault,
                        ..derived_plan
                    };

                    (content, vec![depth_stream(&temp)])
                }
                ContentKind::Ladder => {
                    let config = self
                        .settings
                        .visual_config
                        .clone()
                        .and_then(|cfg| cfg.ladder());
                    let content = Content::Ladder(Some(Ladder::new(
                        config,
                        derived_plan.ticker_info,
                        derived_plan.tick_size,
                    )));

                    (content, vec![depth_stream(&derived_plan)])
                }
                ContentKind::ComparisonChart => {
                    let config = self
                        .settings
                        .visual_config
                        .clone()
                        .and_then(|cfg| cfg.comparison());
                    let basis = derived_plan.basis.unwrap_or(Basis::Time(Timeframe::M15));
                    let content =
                        Content::Comparison(Some(ComparisonChart::new(basis, &tickers, config)));

                    let streams = by_basis_default(
                        derived_plan.basis,
                        Timeframe::M15,
                        |tf| {
                            tickers
                                .iter()
                                .copied()
                                .map(|ti| kline_stream(ti, tf))
                                .collect()
                        },
                        || todo!("WIP: ComparisonChart does not support tick basis"),
                    );

                    (content, streams)
                }
                ContentKind::Starter => unreachable!(),
            }
        };

        self.content = content;
        self.streams = ResolvedStream::Ready(streams.clone());

        streams
    }

    pub fn update_heatmap_overlay_streams(
        &mut self,
        cfg: data::chart::heatmap::Config,
    ) -> (bool, Option<StreamKind>) {
        let desired_tf = resolve_heatmap_candle_tf(cfg, self.settings.selected_basis);
        let base_ticker = self.stream_pair();

        let mut changed = false;
        let mut fetch_stream = None;

        if let ResolvedStream::Ready(streams) = &mut self.streams {
            if let Some(tf) = desired_tf {
                let mut found = false;
                for stream in streams.iter_mut() {
                    if let StreamKind::Kline { timeframe, .. } = stream {
                        found = true;
                        if *timeframe != tf {
                            *timeframe = tf;
                            changed = true;
                            fetch_stream = Some(*stream);
                        }
                    }
                }

                if !found {
                    if let Some(ticker_info) = base_ticker {
                        let stream = StreamKind::Kline {
                            ticker_info,
                            timeframe: tf,
                        };
                        streams.push(stream);
                        changed = true;
                        fetch_stream = Some(stream);
                    }
                }
            } else {
                let before = streams.len();
                streams.retain(|stream| !matches!(stream, StreamKind::Kline { .. }));
                if streams.len() != before {
                    changed = true;
                }
            }
        }

        (changed, fetch_stream)
    }

    pub fn apply_heatmap_sync_basis(&mut self, cfg: data::chart::heatmap::Config) -> bool {
        if !cfg.sync_heatmap_to_candles || !cfg.show_candles {
            return false;
        }

        let Some(tf) = resolve_heatmap_candle_tf(cfg, self.settings.selected_basis) else {
            return false;
        };
        let desired_basis = Basis::Time(tf);

        if self.settings.selected_basis == Some(desired_basis) {
            return false;
        }

        self.settings.selected_basis = Some(desired_basis);
        if let Content::Heatmap { chart: Some(c), .. } = &mut self.content {
            c.set_basis(desired_basis);
        }

        true
    }

    pub fn insert_hist_oi(&mut self, req_id: Option<uuid::Uuid>, oi: &[OpenInterest]) {
        match &mut self.content {
            Content::Kline { chart, .. } => {
                let Some(chart) = chart else {
                    panic!("Kline chart wasn't initialized when inserting open interest");
                };
                chart.insert_open_interest(req_id, oi);
            }
            _ => {
                log::error!("pane content not candlestick");
            }
        }
    }

    pub fn insert_hist_klines(
        &mut self,
        req_id: Option<uuid::Uuid>,
        timeframe: Timeframe,
        ticker_info: TickerInfo,
        klines: &[Kline],
    ) {
        match &mut self.content {
            Content::Heatmap { chart: Some(c), .. } => {
                // Candles overlay on heatmap: accept fetched kline history too (not just realtime).
                if c.visual_config().show_candles {
                    c.on_insert_klines(klines);
                }
            }
            Content::Kline {
                chart, indicators, ..
            } => {
                let Some(chart) = chart else {
                    panic!("chart wasn't initialized when inserting klines");
                };

                if let Some(id) = req_id {
                    if chart.basis() != Basis::Time(timeframe) {
                        log::warn!(
                            "Ignoring stale kline fetch for timeframe {:?}; chart basis = {:?}",
                            timeframe,
                            chart.basis()
                        );
                        return;
                    }
                    chart.insert_hist_klines(id, klines);
                } else {
                    let (raw_trades, tick_size) = (chart.raw_trades(), chart.tick_size());
                    let layout = chart.chart_layout();

                    let mut new_chart = KlineChart::new(
                        layout,
                        Basis::Time(timeframe),
                        tick_size,
                        klines,
                        raw_trades,
                        indicators,
                        ticker_info,
                        chart.kind(),
                    );

                    let visual_cfg = self
                        .settings
                        .visual_config
                        .clone()
                        .and_then(|cfg| cfg.kline())
                        .unwrap_or_default();
                    new_chart.set_visual_config(visual_cfg);

                    *chart = new_chart;
                }
            }
            Content::Comparison(chart) => {
                let Some(chart) = chart else {
                    panic!("Comparison chart wasn't initialized when inserting klines");
                };

                if let Some(id) = req_id {
                    if chart.timeframe != timeframe {
                        log::warn!(
                            "Ignoring stale kline fetch for timeframe {:?}; chart timeframe = {:?}",
                            timeframe,
                            chart.timeframe
                        );
                        return;
                    }
                    chart.insert_history(id, ticker_info, klines);
                } else {
                    *chart = ComparisonChart::new(
                        Basis::Time(timeframe),
                        &[ticker_info],
                        Some(chart.serializable_config()),
                    );
                }
            }
            _ => {
                log::error!("pane content not candlestick or footprint");
            }
        }
    }

    fn has_stream(&self) -> bool {
        match &self.streams {
            ResolvedStream::Ready(streams) => !streams.is_empty(),
            ResolvedStream::Waiting(streams) => !streams.is_empty(),
        }
    }

    pub fn view<'a>(
        &'a self,
        id: pane_grid::Pane,
        panes: usize,
        is_focused: bool,
        maximized: bool,
        window: window::Id,
        main_window: &'a Window,
        timezone: UserTimezone,
        tickers_table: &'a TickersTable,
    ) -> pane_grid::Content<'a, Message, Theme, Renderer> {
        let mut stream_info_element = if Content::Starter == self.content {
            row![]
        } else {
            row![link_group_button(id, self.link_group, |id| {
                Message::PaneEvent(id, Event::ShowModal(Modal::LinkGroup))
            })]
        };

        if let Some(kind) = self.stream_pair_kind() {
            let (base_ti, extra) = match kind {
                StreamPairKind::MultiSource(list) => (list[0], list.len().saturating_sub(1)),
                StreamPairKind::SingleSource(ti) => (ti, 0),
            };

            let exchange_icon = icon_text(style::exchange_icon(base_ti.ticker.exchange), 14);
            let mut label = {
                let symbol = base_ti.ticker.display_symbol_and_type().0;
                match base_ti.ticker.market_type() {
                    MarketKind::Spot => symbol,
                    MarketKind::LinearPerps | MarketKind::InversePerps => symbol + " PERP",
                }
            };
            if extra > 0 {
                label = format!("{label} +{extra}");
            }

            let content = row![exchange_icon, text(label).size(14)]
                .align_y(Vertical::Center)
                .spacing(4);

            let tickers_list_btn = button(content)
                .on_press(Message::PaneEvent(
                    id,
                    Event::ShowModal(Modal::MiniTickersList(MiniPanel::new())),
                ))
                .style(|theme, status| {
                    style::button::modifier(
                        theme,
                        status,
                        !matches!(self.modal, Some(Modal::MiniTickersList(_))),
                    )
                })
                .padding([4, 10]);

            stream_info_element = stream_info_element.push(tickers_list_btn);
        } else if !matches!(self.content, Content::Starter) && !self.has_stream() {
            let content = row![text("Choose a ticker").size(13)]
                .align_y(Alignment::Center)
                .spacing(4);

            let tickers_list_btn = button(content)
                .on_press(Message::PaneEvent(
                    id,
                    Event::ShowModal(Modal::MiniTickersList(MiniPanel::new())),
                ))
                .style(|theme, status| {
                    style::button::modifier(
                        theme,
                        status,
                        !matches!(self.modal, Some(Modal::MiniTickersList(_))),
                    )
                })
                .padding([4, 10]);

            stream_info_element = stream_info_element.push(tickers_list_btn);
        }

        let modifier: Option<modal::stream::Modifier> = self.modal.clone().and_then(|m| {
            if let Modal::StreamModifier(modifier) = m {
                Some(modifier)
            } else {
                None
            }
        });

        let compact_controls = if self.modal == Some(Modal::Controls) {
            Some(
                container(self.view_controls(id, panes, maximized, window != main_window.id))
                    .style(style::chart_modal)
                    .into(),
            )
        } else {
            None
        };

        let uninitialized_base = |kind: ContentKind| -> Element<'a, Message> {
            if self.has_stream() {
                center(text("Loading…").size(16)).into()
            } else {
                let content = column![
                    text(kind.to_string()).size(16),
                    text("No ticker selected").size(14)
                ]
                .spacing(8)
                .align_x(Alignment::Center);

                center(content).into()
            }
        };

        let body = match &self.content {
            Content::Starter => {
                let content_picklist =
                    pick_list(ContentKind::ALL, Some(ContentKind::Starter), move |kind| {
                        Message::PaneEvent(id, Event::ContentSelected(kind))
                    });

                let base: Element<_> = widget::toast::Manager::new(
                    center(
                        column![
                            text("Choose a view to get started").size(16),
                            content_picklist
                        ]
                        .align_x(Alignment::Center)
                        .spacing(12),
                    ),
                    &self.notifications,
                    Alignment::End,
                    move |msg| Message::PaneEvent(id, Event::DeleteNotification(msg)),
                )
                .into();

                self.compose_stack_view(
                    base,
                    id,
                    None,
                    compact_controls,
                    || column![].into(),
                    None,
                    tickers_table,
                )
            }
            Content::Comparison(chart) => {
                if let Some(c) = chart {
                    let selected_basis = self
                        .settings
                        .selected_basis
                        .unwrap_or(Timeframe::M15.into());
                    let kind = ModifierKind::Comparison(selected_basis);

                    let modifiers =
                        row![basis_modifier(id, selected_basis, modifier, kind),].spacing(4);

                    stream_info_element = stream_info_element.push(modifiers);

                    let base = c.view(timezone).map(move |message| {
                        Message::PaneEvent(id, Event::ComparisonChartInteraction(message))
                    });

                    let settings_modal = || comparison_cfg_view(id, c);

                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        settings_modal,
                        Some(c.selected_tickers()),
                        tickers_table,
                    )
                } else {
                    let base = uninitialized_base(ContentKind::ComparisonChart);
                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        || column![].into(),
                        None,
                        tickers_table,
                    )
                }
            }
            Content::TimeAndSales(panel) => {
                if let Some(panel) = panel {
                    let base = panel::view(panel, timezone).map(move |message| {
                        Message::PaneEvent(id, Event::PanelInteraction(message))
                    });

                    let settings_modal =
                        || modal::pane::settings::timesales_cfg_view(panel.config, id);

                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        settings_modal,
                        None,
                        tickers_table,
                    )
                } else {
                    let base = uninitialized_base(ContentKind::TimeAndSales);
                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        || column![].into(),
                        None,
                        tickers_table,
                    )
                }
            }
            Content::Ladder(panel) => {
                if let Some(panel) = panel {
                    let basis = self
                        .settings
                        .selected_basis
                        .unwrap_or(Basis::default_heatmap_time(self.stream_pair()));
                    let tick_multiply = self.settings.tick_multiply.unwrap_or(TickMultiplier(1));

                    let kind = ModifierKind::Orderbook(basis, tick_multiply);

                    let base_ticksize = tick_multiply.base(panel.tick_size());
                    let exchange = self.stream_pair().map(|ti| ti.ticker.exchange);

                    let modifiers = ticksize_modifier(
                        id,
                        base_ticksize,
                        tick_multiply,
                        modifier,
                        kind,
                        exchange,
                    );

                    stream_info_element = stream_info_element.push(modifiers);

                    let base = panel::view(panel, timezone).map(move |message| {
                        Message::PaneEvent(id, Event::PanelInteraction(message))
                    });

                    let settings_modal =
                        || modal::pane::settings::ladder_cfg_view(panel.config, id);

                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        settings_modal,
                        None,
                        tickers_table,
                    )
                } else {
                    let base = uninitialized_base(ContentKind::Ladder);
                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        || column![].into(),
                        None,
                        tickers_table,
                    )
                }
            }
            Content::Heatmap {
                chart, indicators, ..
            } => {
                if let Some(chart) = chart {
                    let ticker_info = self.stream_pair();
                    let exchange = ticker_info.as_ref().map(|info| info.ticker.exchange);

                    let basis = self
                        .settings
                        .selected_basis
                        .unwrap_or(Basis::default_heatmap_time(ticker_info));
                    let tick_multiply = self.settings.tick_multiply.unwrap_or(TickMultiplier(5));

                    let kind = ModifierKind::Heatmap(basis, tick_multiply);
                    let base_ticksize = tick_multiply.base(chart.tick_size());

                    let modifiers = {
                        let mut r = row![basis_modifier(id, basis, modifier, kind)];
                        if chart.visual_config().show_candles {
                            r = r.push(go_to_latest_button(id));
                        }
                        r = r.push(ticksize_modifier(
                            id,
                            base_ticksize,
                            tick_multiply,
                            modifier,
                            kind,
                            exchange,
                        ));
                        r.spacing(4)
                    };

                    stream_info_element = stream_info_element.push(modifiers);

                    let base = chart::view(chart, indicators, timezone).map(move |message| {
                        Message::PaneEvent(id, Event::ChartInteraction(message))
                    });
                    let settings_modal = || {
                        heatmap_cfg_view(
                            chart.visual_config(),
                            id,
                            chart.study_configurator(),
                            &chart.studies,
                            basis,
                        )
                    };

                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        settings_modal,
                        None,
                        tickers_table,
                    )
                } else {
                    let base = uninitialized_base(ContentKind::HeatmapChart);
                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        || column![].into(),
                        None,
                        tickers_table,
                    )
                }
            }
            Content::Kline {
                chart,
                indicators,
                kind: chart_kind,
                ..
            } => {
                if let Some(chart) = chart {
                    match chart_kind {
                        data::chart::KlineChartKind::Footprint { .. } => {
                            let basis =
                                self.settings.selected_basis.unwrap_or(Timeframe::M5.into());
                            let tick_multiply =
                                self.settings.tick_multiply.unwrap_or(TickMultiplier(10));

                            let kind = ModifierKind::Footprint(basis, tick_multiply);
                            let base_ticksize = tick_multiply.base(chart.tick_size());

                            let exchange =
                                self.stream_pair().as_ref().map(|info| info.ticker.exchange);

                            let modifiers = row![
                                basis_modifier(id, basis, modifier, kind),
                                ticksize_modifier(
                                    id,
                                    base_ticksize,
                                    tick_multiply,
                                    modifier,
                                    kind,
                                    exchange
                                ),
                            ]
                            .spacing(4);

                            stream_info_element = stream_info_element.push(modifiers);
                        }
                        data::chart::KlineChartKind::Candles => {
                            let selected_basis = self
                                .settings
                                .selected_basis
                                .unwrap_or(Timeframe::M15.into());
                            let kind = ModifierKind::Candlestick(selected_basis);

                            let modifiers =
                                row![
                                    basis_modifier(id, selected_basis, modifier, kind),
                                    go_to_latest_button(id),
                                ]
                                    .spacing(4);

                            stream_info_element = stream_info_element.push(modifiers);
                        }
                        data::chart::KlineChartKind::CandlesStudied { .. } => {
                            let selected_basis = self
                                .settings
                                .selected_basis
                                .unwrap_or(Timeframe::M15.into());
                            let kind = ModifierKind::Candlestick(selected_basis);

                            let modifiers =
                                row![
                                    basis_modifier(id, selected_basis, modifier, kind),
                                    go_to_latest_button(id),
                                ]
                                    .spacing(4);

                            stream_info_element = stream_info_element.push(modifiers);
                        }
                    }

                    let base = chart::view(chart, indicators, timezone).map(move |message| {
                        Message::PaneEvent(id, Event::ChartInteraction(message))
                    });
                    let settings_modal = || {
                        kline_cfg_view(
                            chart.study_configurator(),
                            chart.candle_study_configurator(),
                            chart.visual_config(),
                            chart_kind,
                            id,
                            chart.basis(),
                            &self.settings_colors_expanded,
                        )
                    };

                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        settings_modal,
                        None,
                        tickers_table,
                    )
                } else {
                    let content_kind = match chart_kind {
                        data::chart::KlineChartKind::Candles => ContentKind::CandlestickChart,
                        data::chart::KlineChartKind::CandlesStudied { .. } => {
                            ContentKind::CandlestickChart
                        }
                        data::chart::KlineChartKind::Footprint { .. } => {
                            ContentKind::FootprintChart
                        }
                    };
                    let base = uninitialized_base(content_kind);
                    self.compose_stack_view(
                        base,
                        id,
                        None,
                        compact_controls,
                        || column![].into(),
                        None,
                        tickers_table,
                    )
                }
            }
        };

        match &self.status {
            Status::Loading(exchange::fetcher::InfoKind::FetchingKlines) => {
                stream_info_element = stream_info_element.push(text("Fetching Klines..."));
            }
            Status::Loading(exchange::fetcher::InfoKind::FetchingTrades(count)) => {
                stream_info_element =
                    stream_info_element.push(text(format!("Fetching Trades... {count} fetched")));
            }
            Status::Loading(exchange::fetcher::InfoKind::FetchingOI) => {
                stream_info_element = stream_info_element.push(text("Fetching Open Interest..."));
            }
            Status::Stale(msg) => {
                stream_info_element = stream_info_element.push(text(msg));
            }
            Status::Ready => {}
        }

        let content = pane_grid::Content::new(body)
            .style(move |theme| style::pane_background(theme, is_focused));

        let controls = {
            let compact_control = container(
                button(text("...").size(13).align_y(Alignment::End))
                    .on_press(Message::PaneEvent(id, Event::ShowModal(Modal::Controls)))
                    .style(move |theme, status| {
                        style::button::transparent(
                            theme,
                            status,
                            self.modal == Some(Modal::Controls)
                                || self.modal == Some(Modal::Settings),
                        )
                    }),
            )
            .align_y(Alignment::Center)
            .height(Length::Fixed(32.0))
            .padding(4);

            if self.modal == Some(Modal::Controls) {
                pane_grid::Controls::new(compact_control)
            } else {
                pane_grid::Controls::dynamic(
                    self.view_controls(id, panes, maximized, window != main_window.id),
                    compact_control,
                )
            }
        };

        let title_bar = pane_grid::TitleBar::new(
            stream_info_element
                .padding(padding::left(4).top(1))
                .align_y(Vertical::Center)
                .spacing(8)
                .height(Length::Fixed(32.0)),
        )
        .controls(controls)
        .style(style::pane_title_bar);

        content.title_bar(if self.modal.is_none() {
            title_bar
        } else {
            title_bar.always_show_controls()
        })
    }

    pub fn update(&mut self, msg: Event) -> Option<Effect> {
        match msg {
            Event::ShowModal(requested_modal) => {
                return self.show_modal_with_focus(requested_modal);
            }
            Event::HideModal => {
                self.modal = None;
            }
            Event::ContentSelected(kind) => {
                self.content = Content::placeholder(kind);

                // Candles+Heatmap is a Heatmap pane with candle overlay enabled by default.
                if matches!(kind, ContentKind::CandlesHeatmapChart) {
                    let cfg = self
                        .settings
                        .visual_config
                        .clone()
                        .and_then(|vc| vc.heatmap())
                        .unwrap_or_default();
                    let cfg = data::chart::heatmap::Config {
                        show_candles: true,
                        ..cfg
                    };
                    self.settings.visual_config = Some(VisualConfig::Heatmap(cfg));
                }

                if !matches!(kind, ContentKind::Starter) {
                    self.streams = ResolvedStream::Waiting(vec![]);
                    let modal = Modal::MiniTickersList(MiniPanel::new());

                    if let Some(effect) = self.show_modal_with_focus(modal) {
                        return Some(effect);
                    }
                }
            }
            Event::ChartInteraction(msg) => match &mut self.content {
                Content::Heatmap { chart: Some(c), .. } => {
                    if let crate::chart::Message::ContextMenuRequested(_pos) = msg {
                        // no context menu for heatmap panes (yet)
                        return None;
                    }
                    super::chart::update(c, &msg);
                }
                Content::Kline { chart: Some(c), kind, .. } => {
                    if let crate::chart::Message::ContextMenuRequested(pos) = msg {
                        // only for Candlestick panes (candles/candles+studies)
                        if matches!(
                            kind,
                            data::chart::KlineChartKind::Candles
                                | data::chart::KlineChartKind::CandlesStudied { .. }
                        ) {
                            let pos = iced::Point::new(pos.x.max(0.0), pos.y.max(0.0));
                            let _ = self.show_modal_with_focus(Modal::ContextMenu(pos));
                        }
                        return None;
                    }
                    super::chart::update(c, &msg);
                }
                _ => {}
            },
            Event::GoToLatest => {
                match &mut self.content {
                    Content::Kline { chart: Some(c), .. } => {
                        super::chart::update(c, &crate::chart::Message::DoubleClick(crate::chart::AxisScaleClicked::X));
                    }
                    _ => {}
                }
            },
            Event::ResizeCenteredModal(kind, w, h) => {
                // Keep sizes sane
                let w = w.clamp(280.0, 1200.0);
                let h = h.clamp(220.0, 1000.0);
                match kind {
                    CenteredModalKind::Rules => self.centered_rules_size = (w, h),
                    CenteredModalKind::Indicators => self.centered_indicators_size = (w, h),
                    CenteredModalKind::RuleLog => self.centered_rule_log_size = (w, h),
                }
            }
            Event::PanelInteraction(msg) => match &mut self.content {
                Content::Ladder(Some(p)) => super::panel::update(p, msg),
                Content::TimeAndSales(Some(p)) => super::panel::update(p, msg),
                _ => {}
            },
            Event::ToggleIndicator(ind) => {
                self.content.toggle_indicator(ind);
            }
            Event::AddRule => {
                self.rules.push(RuleSpec::default());
            }
            Event::AddRuleAndOpen => {
                let rule = RuleSpec::default();
                let id = rule.id;
                self.rules.push(rule);
                self.rules_expanded = Some(id);
                let _ = self.show_modal_with_focus(Modal::Rules);
            }
            Event::DeleteRule(id) => {
                self.rules.retain(|r| r.id != id);
                if self.rules_expanded == Some(id) {
                    self.rules_expanded = None;
                }
            }
            Event::ToggleRule(id, enabled) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    rule.enabled = enabled;
                }
            }
            Event::ToggleRuleCard(id) => {
                self.rules_expanded = if self.rules_expanded == Some(id) {
                    None
                } else {
                    Some(id)
                };
            }
            Event::UpdateRuleName(id, name) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    rule.name = name;
                }
            }
            Event::UpdateRuleEvaluation(id, mode) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    rule.evaluation = mode;
                }
            }
            Event::UpdateRuleConditionKind(id, kind) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    rule.condition = match kind {
                        modal::pane::rules::ConditionKind::PriceCrossLevel => {
                            RuleCondition::PriceCrossLevel {
                                level: 0.0,
                                direction: data::rules::CrossDirection::CrossUp,
                            }
                        }
                        modal::pane::rules::ConditionKind::CandleCloseCrossLevel => {
                            RuleCondition::CandleCloseCrossLevel {
                                level: 0.0,
                                direction: data::rules::CrossDirection::CrossUp,
                            }
                        }
                        modal::pane::rules::ConditionKind::VolumeIs => RuleCondition::VolumeIs {
                            value: 0.0,
                            direction: data::rules::CompareDirection::Above,
                        },
                        modal::pane::rules::ConditionKind::MovingAverageCross => {
                            RuleCondition::MovingAverageCross {
                                direction: data::rules::CrossDirection::CrossUp,
                            }
                        }
                        modal::pane::rules::ConditionKind::RsiCrossLevel => {
                            RuleCondition::RsiCrossLevel {
                                level: 50.0,
                                direction: data::rules::CrossDirection::CrossUp,
                            }
                        }
                        modal::pane::rules::ConditionKind::MacdCrossSignal => {
                            RuleCondition::MacdCrossSignal {
                                direction: data::rules::CrossDirection::CrossUp,
                            }
                        }
                        modal::pane::rules::ConditionKind::VwapCross => RuleCondition::VwapCross {
                            direction: data::rules::CrossDirection::CrossUp,
                        },
                        modal::pane::rules::ConditionKind::SupertrendFlip => {
                            RuleCondition::SupertrendFlip {
                                direction: data::rules::CrossDirection::CrossUp,
                            }
                        }
                        modal::pane::rules::ConditionKind::SupertrendLineCross => {
                            RuleCondition::SupertrendLineCross {
                                direction: data::rules::CrossDirection::CrossUp,
                            }
                        }
                        modal::pane::rules::ConditionKind::DonchianBreakout => {
                            RuleCondition::DonchianBreakout {
                                direction: data::rules::CrossDirection::CrossUp,
                            }
                        }
                        modal::pane::rules::ConditionKind::KeltnerBreakout => {
                            RuleCondition::KeltnerBreakout {
                                direction: data::rules::CrossDirection::CrossUp,
                            }
                        }
                        modal::pane::rules::ConditionKind::DmiCross => RuleCondition::DmiCross {
                            direction: data::rules::CrossDirection::CrossUp,
                        },
                        modal::pane::rules::ConditionKind::AdxIs => RuleCondition::AdxIs {
                            value: 20.0,
                            direction: data::rules::CompareDirection::Above,
                        },
                    };
                }
            }
            Event::UpdateRuleCrossDirection(id, dir) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    match &mut rule.condition {
                        RuleCondition::PriceCrossLevel { direction, .. }
                        | RuleCondition::CandleCloseCrossLevel { direction, .. }
                        | RuleCondition::MovingAverageCross { direction }
                        | RuleCondition::RsiCrossLevel { direction, .. }
                        | RuleCondition::MacdCrossSignal { direction }
                        | RuleCondition::VwapCross { direction }
                        | RuleCondition::SupertrendFlip { direction }
                        | RuleCondition::SupertrendLineCross { direction }
                        | RuleCondition::DonchianBreakout { direction }
                        | RuleCondition::KeltnerBreakout { direction }
                        | RuleCondition::DmiCross { direction } => {
                            *direction = dir;
                        }
                        _ => {}
                    }
                }
            }
            Event::UpdateRuleCompareDirection(id, dir) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    match &mut rule.condition {
                        RuleCondition::VolumeIs { direction, .. }
                        | RuleCondition::AdxIs { direction, .. } => {
                            *direction = dir;
                        }
                        _ => {}
                    }
                }
            }
            Event::UpdateRuleLevel(id, raw) => {
                let Ok(v) = raw.trim().parse::<f32>() else { return None; };
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    match &mut rule.condition {
                        RuleCondition::PriceCrossLevel { level, .. }
                        | RuleCondition::CandleCloseCrossLevel { level, .. }
                        | RuleCondition::RsiCrossLevel { level, .. } => {
                            *level = v;
                        }
                        RuleCondition::VolumeIs { value, .. } => {
                            *value = v;
                        }
                        RuleCondition::AdxIs { value, .. } => {
                            *value = v;
                        }
                        _ => {}
                    }
                }
            }
            Event::ToggleRuleActionToast(id, enabled) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    State::upsert_action(
                        rule,
                        RuleAction::Toast {
                            message: "Rule triggered".to_string(),
                        },
                        enabled,
                    );
                }
            }
            Event::UpdateRuleToastMessage(id, msg) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    State::set_toast_message(rule, msg);
                }
            }
            Event::ToggleRuleActionSound(id, enabled) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    State::upsert_action(rule, RuleAction::Sound { enabled: true }, enabled);
                }
            }
            Event::ToggleRuleActionTelegram(id, enabled) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    State::upsert_action(rule, RuleAction::Telegram { enabled: true }, enabled);
                }
            }
            Event::ToggleRuleActionPush(id, enabled) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    State::upsert_action(rule, RuleAction::Push { enabled: true }, enabled);
                }
            }
            Event::ToggleSettingsColors(section) => {
                if self.settings_colors_expanded.contains(&section) {
                    self.settings_colors_expanded.remove(&section);
                } else {
                    self.settings_colors_expanded.insert(section);
                }
            }
            Event::IndicatorsQueryChanged(q) => {
                self.indicators_query = q;
            }
            Event::IndicatorsSidebarSelected(sidebar) => {
                self.indicators_sidebar = sidebar;
            }
            Event::IndicatorsSourceSelected(source) => {
                self.indicators_source = source;
                // reset filters when switching source
                self.indicators_query.clear();
                self.indicators_sidebar = IndicatorsSidebar::All;
            }
            Event::ToggleRuleActionPaperTrade(id, enabled) => {
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    State::upsert_action(
                        rule,
                        RuleAction::PaperTrade {
                            side: data::rules::Side::Buy,
                            percent_of_balance: 25.0,
                        },
                        enabled,
                    );
                }
            }
            Event::UpdateRulePaperPercent(id, raw) => {
                let Ok(v) = raw.trim().parse::<f32>() else { return None; };
                if let Some(rule) = self.rules.iter_mut().find(|r| r.id == id) {
                    for a in &mut rule.actions {
                        if let RuleAction::PaperTrade { percent_of_balance, .. } = a {
                            *percent_of_balance = v;
                        }
                    }
                }
            }
            Event::ClearRuleLog => {
                self.rule_log.clear();
            }
            Event::DeleteNotification(idx) => {
                if idx < self.notifications.len() {
                    self.notifications.remove(idx);
                }
            }
            Event::ReorderIndicator(e) => {
                self.content.reorder_indicators(&e);
            }
            Event::ClusterKindSelected(kind) => {
                if let Content::Kline {
                    chart, kind: cur, ..
                } = &mut self.content
                    && let Some(c) = chart
                {
                    c.set_cluster_kind(kind);
                    *cur = c.kind.clone();
                }
            }
            Event::ClusterScalingSelected(scaling) => {
                if let Content::Kline { chart, kind, .. } = &mut self.content
                    && let Some(c) = chart
                {
                    c.set_cluster_scaling(scaling);
                    *kind = c.kind.clone();
                }
            }
            Event::StudyConfigurator(study_msg) => match study_msg {
                modal::pane::settings::study::StudyMessage::Footprint(m) => {
                    if let Content::Kline { chart, kind, .. } = &mut self.content
                        && let Some(c) = chart
                    {
                        c.update_study_configurator(m);
                        *kind = c.kind.clone();
                    }
                }
                modal::pane::settings::study::StudyMessage::Heatmap(m) => {
                    if let Content::Heatmap { chart, studies, .. } = &mut self.content
                        && let Some(c) = chart
                    {
                        c.update_study_configurator(m);
                        *studies = c.studies.clone();
                    }
                }
                modal::pane::settings::study::StudyMessage::Candle(m) => {
                    if let Content::Kline { chart, kind, .. } = &mut self.content
                        && let Some(c) = chart
                    {
                        c.update_candle_study_configurator(m);
                        *kind = c.kind.clone();
                    }
                }
            },
            Event::StreamModifierChanged(message) => {
                if let Some(Modal::StreamModifier(mut modifier)) = self.modal.take() {
                    let mut effect: Option<Effect> = None;

                    if let Some(action) = modifier.update(message) {
                        match action {
                            modal::stream::Action::TabSelected(tab) => {
                                modifier.tab = tab;
                            }
                            modal::stream::Action::TicksizeSelected(tm) => {
                                modifier.update_kind_with_multiplier(tm);
                                self.settings.tick_multiply = Some(tm);

                                if let Some(ticker) = self.stream_pair() {
                                    match &mut self.content {
                                        Content::Kline { chart: Some(c), .. } => {
                                            c.change_tick_size(
                                                tm.multiply_with_min_tick_size(ticker),
                                            );
                                            c.reset_request_handler();
                                        }
                                        Content::Heatmap { chart: Some(c), .. } => {
                                            c.change_tick_size(
                                                tm.multiply_with_min_tick_size(ticker),
                                            );
                                        }
                                        Content::Ladder(Some(p)) => {
                                            p.set_tick_size(tm.multiply_with_min_tick_size(ticker));
                                        }
                                        _ => {}
                                    }
                                }

                                let is_client = self
                                    .stream_pair()
                                    .map(|ti| ti.exchange().is_depth_client_aggr())
                                    .unwrap_or(false);

                                if let Some(mut it) = self.streams.ready_iter_mut() {
                                    for s in &mut it {
                                        if let StreamKind::DepthAndTrades { depth_aggr, .. } = s {
                                            *depth_aggr = if is_client {
                                                StreamTicksize::Client
                                            } else {
                                                StreamTicksize::ServerSide(tm)
                                            };
                                        }
                                    }
                                }
                                if !is_client {
                                    effect = Some(Effect::RefreshStreams);
                                }
                            }
                            modal::stream::Action::BasisSelected(new_basis) => {
                                modifier.update_kind_with_basis(new_basis);
                                self.settings.selected_basis = Some(new_basis);

                                let base_ticker = self.stream_pair();

                                match &mut self.content {
                                    Content::Heatmap { chart: Some(c), .. } => {
                                        c.set_basis(new_basis);

                                        if let Some(stream_type) =
                                            self.streams.ready_iter_mut().and_then(|mut it| {
                                                it.find(|s| {
                                                    matches!(s, StreamKind::DepthAndTrades { .. })
                                                })
                                            })
                                            && let StreamKind::DepthAndTrades {
                                                push_freq,
                                                ticker_info,
                                                ..
                                            } = stream_type
                                            && ticker_info.exchange().is_custom_push_freq()
                                        {
                                            match new_basis {
                                                Basis::Time(tf) => {
                                                    *push_freq = exchange::PushFrequency::Custom(tf)
                                                }
                                                Basis::Tick(_) => {
                                                    *push_freq =
                                                        exchange::PushFrequency::ServerDefault
                                                }
                                            }
                                        }

                                        effect = Some(Effect::RefreshStreams);
                                    }
                                    Content::Kline { chart: Some(c), .. } => {
                                        if let Some(base_ticker) = base_ticker {
                                            match new_basis {
                                                Basis::Time(tf) => {
                                                    let kline_stream = StreamKind::Kline {
                                                        ticker_info: base_ticker,
                                                        timeframe: tf,
                                                    };
                                                    let mut streams = vec![kline_stream];

                                                    if matches!(
                                                        c.kind,
                                                        data::chart::KlineChartKind::Footprint { .. }
                                                    ) {
                                                        let depth_aggr = if base_ticker
                                                            .exchange()
                                                            .is_depth_client_aggr()
                                                        {
                                                            StreamTicksize::Client
                                                        } else {
                                                            StreamTicksize::ServerSide(
                                                                self.settings
                                                                    .tick_multiply
                                                                    .unwrap_or(TickMultiplier(1)),
                                                            )
                                                        };
                                                        streams.push(StreamKind::DepthAndTrades {
                                                            ticker_info: base_ticker,
                                                            depth_aggr,
                                                            push_freq: exchange::PushFrequency::ServerDefault,
                                                        });
                                                    }

                                                    self.streams = ResolvedStream::Ready(streams);
                                                    let action = c.set_basis(new_basis);

                                                    if let Some(chart::Action::RequestFetch(
                                                        fetch,
                                                    )) = action
                                                    {
                                                        effect = Some(Effect::RequestFetch(fetch));
                                                    }
                                                }
                                                Basis::Tick(_) => {
                                                    let depth_aggr = if base_ticker
                                                        .exchange()
                                                        .is_depth_client_aggr()
                                                    {
                                                        StreamTicksize::Client
                                                    } else {
                                                        StreamTicksize::ServerSide(
                                                            self.settings
                                                                .tick_multiply
                                                                .unwrap_or(TickMultiplier(1)),
                                                        )
                                                    };

                                                    self.streams = ResolvedStream::Ready(vec![
                                                        StreamKind::DepthAndTrades {
                                                            ticker_info: base_ticker,
                                                            depth_aggr,
                                                            push_freq: exchange::PushFrequency::ServerDefault,
                                                        },
                                                    ]);
                                                    c.set_basis(new_basis);
                                                    effect = Some(Effect::RefreshStreams);
                                                }
                                            }
                                        }
                                    }
                                    Content::Comparison(Some(c)) => {
                                        if let Basis::Time(tf) = new_basis {
                                            let streams: Vec<StreamKind> = c
                                                .selected_tickers()
                                                .iter()
                                                .copied()
                                                .map(|ti| StreamKind::Kline {
                                                    ticker_info: ti,
                                                    timeframe: tf,
                                                })
                                                .collect();

                                            self.streams = ResolvedStream::Ready(streams);
                                            let action = c.set_basis(new_basis);

                                            if let Some(chart::Action::RequestFetch(fetch)) = action
                                            {
                                                effect = Some(Effect::RequestFetch(fetch));
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    self.modal = Some(Modal::StreamModifier(modifier));

                    if let Some(e) = effect {
                        return Some(e);
                    }
                }
            }
            Event::ComparisonChartInteraction(message) => {
                if let Content::Comparison(chart_opt) = &mut self.content
                    && let Some(chart) = chart_opt
                    && let Some(action) = chart.update(message)
                {
                    match action {
                        super::chart::comparison::Action::SeriesColorChanged(t, color) => {
                            chart.set_series_color(t, color);
                        }
                        super::chart::comparison::Action::SeriesNameChanged(t, name) => {
                            chart.set_series_name(t, name);
                        }
                        super::chart::comparison::Action::OpenSeriesEditor => {
                            self.modal = Some(Modal::Settings);
                        }
                        super::chart::comparison::Action::RemoveSeries(ti) => {
                            let rebuilt = chart.remove_ticker(&ti);
                            self.streams = ResolvedStream::Ready(rebuilt);

                            return Some(Effect::RefreshStreams);
                        }
                    }
                }
            }
            Event::MiniTickersListInteraction(message) => {
                if let Some(Modal::MiniTickersList(ref mut mini_panel)) = self.modal
                    && let Some(action) = mini_panel.update(message)
                {
                    self.modal = Some(Modal::MiniTickersList(mini_panel.clone()));

                    let crate::modal::pane::mini_tickers_list::Action::RowSelected(sel) = action;
                    match sel {
                        crate::modal::pane::mini_tickers_list::RowSelection::Add(ti) => {
                            if let Content::Comparison(chart) = &mut self.content
                                && let Some(c) = chart
                            {
                                let rebuilt = c.add_ticker(&ti);
                                self.streams = ResolvedStream::Ready(rebuilt);
                                return Some(Effect::RefreshStreams);
                            }
                        }
                        crate::modal::pane::mini_tickers_list::RowSelection::Remove(ti) => {
                            if let Content::Comparison(chart) = &mut self.content
                                && let Some(c) = chart
                            {
                                let rebuilt = c.remove_ticker(&ti);
                                self.streams = ResolvedStream::Ready(rebuilt);
                                return Some(Effect::RefreshStreams);
                            }
                        }
                        crate::modal::pane::mini_tickers_list::RowSelection::Switch(ti) => {
                            return Some(Effect::SwitchTickersInGroup(ti));
                        }
                    }
                }
            }
        }
        None
    }

    fn view_controls(
        &'_ self,
        pane: pane_grid::Pane,
        total_panes: usize,
        is_maximized: bool,
        is_popout: bool,
    ) -> Element<'_, Message> {
        let modal_btn_style = |modal: Modal| {
            let is_active = self.modal == Some(modal);
            move |theme: &Theme, status: button::Status| {
                style::button::transparent(theme, status, is_active)
            }
        };

        let control_btn_style = |is_active: bool| {
            move |theme: &Theme, status: button::Status| {
                style::button::transparent(theme, status, is_active)
            }
        };

        let treat_as_starter =
            matches!(&self.content, Content::Starter) || !self.content.initialized();

        let tooltip_pos = tooltip::Position::Bottom;
        let mut buttons = row![];

        let show_modal = |modal: Modal| Message::PaneEvent(pane, Event::ShowModal(modal));

        if !treat_as_starter {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Cog, 12),
                show_modal(Modal::Settings),
                None,
                tooltip_pos,
                modal_btn_style(Modal::Settings),
            ));
        }
        if !treat_as_starter
            && matches!(
                &self.content,
                Content::Heatmap { .. } | Content::Kline { .. }
            )
        {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::ChartOutline, 12),
                show_modal(Modal::Indicators),
                Some("Indicators"),
                tooltip_pos,
                modal_btn_style(Modal::Indicators),
            ));
        }

        if !treat_as_starter && matches!(&self.content, Content::Kline { .. }) {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Edit, 12),
                show_modal(Modal::Rules),
                Some("Rules"),
                tooltip_pos,
                modal_btn_style(Modal::Rules),
            ));

            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Search, 12),
                show_modal(Modal::RuleLog),
                Some("Rule log"),
                tooltip_pos,
                modal_btn_style(Modal::RuleLog),
            ));
        }

        if is_popout {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Popout, 12),
                Message::Merge,
                Some("Merge"),
                tooltip_pos,
                control_btn_style(is_popout),
            ));
        } else if total_panes > 1 {
            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Popout, 12),
                Message::Popout,
                Some("Pop out"),
                tooltip_pos,
                control_btn_style(is_popout),
            ));
        }

        if total_panes > 1 {
            let (resize_icon, message) = if is_maximized {
                (Icon::ResizeSmall, Message::Restore)
            } else {
                (Icon::ResizeFull, Message::MaximizePane(pane))
            };

            buttons = buttons.push(button_with_tooltip(
                icon_text(resize_icon, 12),
                message,
                None,
                tooltip_pos,
                control_btn_style(is_maximized),
            ));

            buttons = buttons.push(button_with_tooltip(
                icon_text(Icon::Close, 12),
                Message::ClosePane(pane),
                None,
                tooltip_pos,
                control_btn_style(false),
            ));
        }

        buttons
            .padding(padding::right(4).left(4))
            .align_y(Vertical::Center)
            .height(Length::Fixed(32.0))
            .into()
    }

    fn compose_stack_view<'a, F>(
        &'a self,
        base: Element<'a, Message>,
        pane: pane_grid::Pane,
        _indicator_modal: Option<Element<'a, Message>>,
        compact_controls: Option<Element<'a, Message>>,
        settings_modal: F,
        selected_tickers: Option<&'a [TickerInfo]>,
        tickers_table: &'a TickersTable,
    ) -> Element<'a, Message>
    where
        F: FnOnce() -> Element<'a, Message>,
    {
        let base =
            widget::toast::Manager::new(base, &self.notifications, Alignment::End, move |msg| {
                Message::PaneEvent(pane, Event::DeleteNotification(msg))
            })
            .into();

        let on_blur = Message::PaneEvent(pane, Event::HideModal);

        match &self.modal {
            Some(Modal::LinkGroup) => {
                let content = link_group_modal(pane, self.link_group);

                stack_modal(
                    base,
                    content,
                    on_blur,
                    padding::right(12).left(4),
                    Alignment::Start,
                    Alignment::Start,
                )
            }
            Some(Modal::StreamModifier(modifier)) => stack_modal(
                base,
                modifier.view(self.stream_pair()).map(move |message| {
                    Message::PaneEvent(pane, Event::StreamModifierChanged(message))
                }),
                Message::PaneEvent(pane, Event::HideModal),
                padding::right(12).left(48),
                Alignment::Start,
                Alignment::Start,
            ),
            Some(Modal::MiniTickersList(panel)) => {
                let mini_list = panel
                    .view(tickers_table, selected_tickers, self.stream_pair())
                    .map(move |msg| {
                        Message::PaneEvent(pane, Event::MiniTickersListInteraction(msg))
                    });

                let content: Element<_> = container(mini_list)
                    .max_width(260)
                    .padding(16)
                    .style(style::chart_modal)
                    .into();

                stack_modal(
                    base,
                    content,
                    Message::PaneEvent(pane, Event::HideModal),
                    padding::left(12),
                    Alignment::Start,
                    Alignment::Start,
                )
            }
            Some(Modal::Settings) => stack_modal(
                base,
                settings_modal(),
                on_blur,
                padding::right(12).left(12),
                Alignment::End,
                Alignment::Center,
            ),
            // NOTE: Indicators/Rules/RuleLog are rendered as window-level (global) modals in `dashboard::view`
            // so they are not constrained by the pane size.
            Some(Modal::Indicators) | Some(Modal::Rules) | Some(Modal::RuleLog) => base,
            Some(Modal::Controls) => stack_modal(
                base,
                if let Some(controls) = compact_controls {
                    controls
                } else {
                    column![].into()
                },
                on_blur,
                padding::left(12),
                Alignment::End,
                Alignment::Center,
            ),
            Some(Modal::ContextMenu(pos)) => {
                let menu: Element<_> = container(
                    column![
                        button("Rules…").on_press(Message::PaneEvent(pane, Event::ShowModal(Modal::Rules))),
                        button("Indicators…")
                            .on_press(Message::PaneEvent(pane, Event::ShowModal(Modal::Indicators))),
                        button("Add rule…").on_press(Message::PaneEvent(pane, Event::AddRuleAndOpen)),
                    ]
                    .spacing(6),
                )
                .padding(10)
                .style(style::chart_modal)
                .into();

                stack_context_menu(base, menu, on_blur, *pos)
            }
            None => base,
        }
    }

    pub fn matches_stream(&self, stream: &StreamKind) -> bool {
        self.streams.matches_stream(stream)
    }

    fn show_modal_with_focus(&mut self, requested_modal: Modal) -> Option<Effect> {
        let should_toggle_close = match (&self.modal, &requested_modal) {
            (Some(Modal::StreamModifier(open)), Modal::StreamModifier(req)) => {
                open.view_mode == req.view_mode
            }
            (Some(open), req) => core::mem::discriminant(open) == core::mem::discriminant(req),
            _ => false,
        };

        if should_toggle_close {
            self.modal = None;
            return None;
        }

        let focus_widget_id = match &requested_modal {
            Modal::MiniTickersList(m) => Some(m.search_box_id.clone()),
            _ => None,
        };

        self.modal = Some(requested_modal);
        focus_widget_id.map(Effect::FocusWidget)
    }

    pub fn invalidate(&mut self, now: Instant) -> Option<Action> {
        match &mut self.content {
            Content::Heatmap { chart, .. } => chart
                .as_mut()
                .and_then(|c| c.invalidate(Some(now)).map(Action::Chart)),
            Content::Kline { chart, .. } => chart
                .as_mut()
                .and_then(|c| c.invalidate(Some(now)).map(Action::Chart)),
            Content::TimeAndSales(panel) => panel
                .as_mut()
                .and_then(|p| p.invalidate(Some(now)).map(Action::Panel)),
            Content::Ladder(panel) => panel
                .as_mut()
                .and_then(|p| p.invalidate(Some(now)).map(Action::Panel)),
            Content::Starter => None,
            Content::Comparison(chart) => chart
                .as_mut()
                .and_then(|c| c.invalidate(Some(now)).map(Action::Chart)),
        }
    }

    pub fn update_interval(&self) -> Option<u64> {
        match &self.content {
            Content::Kline { .. } | Content::Comparison(_) => Some(1000),
            Content::Heatmap { chart, .. } => {
                if let Some(chart) = chart {
                    chart.basis_interval()
                } else {
                    None
                }
            }
            Content::Ladder(_) | Content::TimeAndSales(_) => Some(100),
            Content::Starter => None,
        }
    }

    pub fn last_tick(&self) -> Option<Instant> {
        self.content.last_tick()
    }

    pub fn tick(&mut self, now: Instant) -> Option<Action> {
        let invalidate_interval: Option<u64> = self.update_interval();
        let last_tick: Option<Instant> = self.last_tick();

        if let Some(streams) = self.streams.waiting_to_resolve()
            && !streams.is_empty()
        {
            return Some(Action::ResolveStreams(streams.to_vec()));
        }

        if !self.content.initialized() {
            return Some(Action::ResolveContent);
        }

        match (invalidate_interval, last_tick) {
            (Some(interval_ms), Some(previous_tick_time)) => {
                if interval_ms > 0 {
                    let interval_duration = std::time::Duration::from_millis(interval_ms);
                    if now.duration_since(previous_tick_time) >= interval_duration {
                        return self.invalidate(now);
                    }
                }
            }
            (Some(interval_ms), None) => {
                if interval_ms > 0 {
                    return self.invalidate(now);
                }
            }
            (None, _) => {}
        }

        None
    }

    pub fn unique_id(&self) -> uuid::Uuid {
        self.id
    }

    // === Indicator math helpers (MVP) ===
}

#[derive(Clone, Copy)]
struct MaCfg {
    kind: data::chart::kline::MovingAverageKind,
    period: u16,
}

fn ma_pair_from_kind(kind: &data::chart::KlineChartKind) -> Option<(MaCfg, MaCfg)> {
    let data::chart::KlineChartKind::CandlesStudied { studies } = kind else {
        return None;
    };

    let mut fast: Option<MaCfg> = None;
    let mut slow: Option<MaCfg> = None;
    for s in studies {
        match *s {
            data::chart::kline::CandleStudy::MovingAverageFast { kind, period, .. } => {
                fast = Some(MaCfg { kind, period });
            }
            data::chart::kline::CandleStudy::MovingAverageSlow { kind, period, .. } => {
                slow = Some(MaCfg { kind, period });
            }
            _ => {}
        }
    }
    Some((fast?, slow?))
}

#[derive(Clone, Copy)]
struct VwapCfg {
    reset_daily_utc: bool,
}

fn vwap_cfg_from_kind(kind: &data::chart::KlineChartKind) -> Option<VwapCfg> {
    let data::chart::KlineChartKind::CandlesStudied { studies } = kind else {
        return None;
    };
    for s in studies {
        if let data::chart::kline::CandleStudy::VwapBands { reset_daily_utc, .. } = *s {
            return Some(VwapCfg { reset_daily_utc });
        }
    }
    None
}

#[derive(Clone, Copy)]
struct SupertrendCfg {
    atr_period: u16,
    multiplier_x100: u16,
}

fn supertrend_cfg_from_kind(kind: &data::chart::KlineChartKind) -> Option<SupertrendCfg> {
    let data::chart::KlineChartKind::CandlesStudied { studies } = kind else {
        return None;
    };
    for s in studies {
        if let data::chart::kline::CandleStudy::Supertrend {
            atr_period,
            multiplier_x100,
            ..
        } = *s
        {
            return Some(SupertrendCfg {
                atr_period,
                multiplier_x100,
            });
        }
    }
    None
}

#[derive(Clone, Copy)]
struct DonchianCfg {
    period: u16,
}

fn donchian_cfg_from_kind(kind: &data::chart::KlineChartKind) -> Option<DonchianCfg> {
    let data::chart::KlineChartKind::CandlesStudied { studies } = kind else {
        return None;
    };
    for s in studies {
        if let data::chart::kline::CandleStudy::DonchianChannels { period, .. } = *s {
            return Some(DonchianCfg { period });
        }
    }
    None
}

#[derive(Clone, Copy)]
struct KeltnerCfg {
    ema_period: u16,
    atr_period: u16,
    multiplier_x100: u16,
}

fn keltner_cfg_from_kind(kind: &data::chart::KlineChartKind) -> Option<KeltnerCfg> {
    let data::chart::KlineChartKind::CandlesStudied { studies } = kind else {
        return None;
    };
    for s in studies {
        if let data::chart::kline::CandleStudy::KeltnerChannels {
            ema_period,
            atr_period,
            multiplier_x100,
            ..
        } = *s
        {
            return Some(KeltnerCfg {
                ema_period,
                atr_period,
                multiplier_x100,
            });
        }
    }
    None
}

fn ma_last_two(
    closes: &[f32],
    kind: data::chart::kline::MovingAverageKind,
    period: u16,
) -> Option<(f32, f32)> {
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
            let mut out = Vec::with_capacity(slice.len());
            out.push(prev);
            for &v in slice.iter().skip(1) {
                prev = v * k + prev * (1.0 - k);
                out.push(prev);
            }
            if out.len() >= 2 {
                Some((out[out.len() - 2], out[out.len() - 1]))
            } else {
                None
            }
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

    // initial avg gains/losses over first p diffs
    let mut gains = 0.0f32;
    let mut losses = 0.0f32;
    for i in 1..=p {
        let diff = slice[i] - slice[i - 1];
        if diff >= 0.0 {
            gains += diff;
        } else {
            losses += -diff;
        }
    }
    let mut avg_gain = gains / p as f32;
    let mut avg_loss = losses / p as f32;

    let rsi = |ag: f32, al: f32| -> f32 {
        if al == 0.0 {
            100.0
        } else {
            let rs = ag / al;
            100.0 - (100.0 / (1.0 + rs))
        }
    };

    // rsi at index p
    let mut prev_rsi = rsi(avg_gain, avg_loss);
    let mut cur_rsi = prev_rsi;

    // process remaining diffs (we only need last 2 rsi values)
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
    if values.is_empty() {
        return vec![];
    }
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

fn macd_last_two(
    closes: &[f32],
    fast: u16,
    slow: u16,
    signal: u16,
) -> Option<((f32, f32), (f32, f32))> {
    let fast = (fast as usize).max(2);
    let slow = (slow as usize).max(3);
    let signal = (signal as usize).max(2);
    if closes.len() < slow + signal + 2 {
        // not enough for stable MACD; still allow best-effort with what we have
        if closes.len() < 3 {
            return None;
        }
    }
    let start = closes.len().saturating_sub(slow + signal + 10);
    let slice = &closes[start..];

    let ema_fast = ema(slice, fast);
    let ema_slow = ema(slice, slow);
    let mut macd_line = Vec::with_capacity(slice.len());
    for i in 0..slice.len() {
        macd_line.push(ema_fast[i] - ema_slow[i]);
    }
    let signal_line = ema(&macd_line, signal);

    if macd_line.len() >= 2 && signal_line.len() >= 2 {
        let pm = macd_line[macd_line.len() - 2];
        let cm = macd_line[macd_line.len() - 1];
        let ps = signal_line[signal_line.len() - 2];
        let cs = signal_line[signal_line.len() - 1];
        Some(((pm, ps), (cm, cs)))
    } else {
        None
    }
}

fn vwap_last_two(ohlcv: &[(u64, f32, f32, f32, f32)], reset_daily_utc: bool) -> Option<(f32, f32)> {
    if ohlcv.len() < 2 {
        return None;
    }

    let start_idx = if reset_daily_utc {
        let day_ms = 86_400_000u64;
        let last_day = ohlcv[ohlcv.len() - 1].0 / day_ms;
        ohlcv
            .iter()
            .rposition(|(t, _, _, _, _)| (*t / day_ms) != last_day)
            .map(|idx| idx + 1)
            .unwrap_or(0)
    } else {
        0
    };

    let mut sum_w = 0.0f32;
    let mut sum_wx = 0.0f32;
    let mut out = Vec::with_capacity(ohlcv.len() - start_idx);
    for &(_, _, _, close, vol) in &ohlcv[start_idx..] {
        if vol > 0.0 {
            sum_w += vol;
            sum_wx += close * vol;
        }
        if sum_w > 0.0 {
            out.push(sum_wx / sum_w);
        } else {
            out.push(close);
        }
    }

    if out.len() >= 2 {
        Some((out[out.len() - 2], out[out.len() - 1]))
    } else {
        None
    }
}

fn donchian_last_two(
    ohlcv: &[(u64, f32, f32, f32, f32)],
    period: u16,
) -> Option<((f32, f32), (f32, f32))> {
    let p = (period as usize).max(2);
    if ohlcv.len() < p + 1 {
        return None;
    }

    let channel_at = |end_idx: usize| -> (f32, f32) {
        let start = end_idx + 1 - p;
        let mut upper = f32::NEG_INFINITY;
        let mut lower = f32::INFINITY;
        for &(_, high, low, _, _) in &ohlcv[start..=end_idx] {
            if high > upper {
                upper = high;
            }
            if low < lower {
                lower = low;
            }
        }
        (upper, lower)
    };

    let prev_end = ohlcv.len() - 2;
    let cur_end = ohlcv.len() - 1;
    Some((channel_at(prev_end), channel_at(cur_end)))
}

fn atr_series_wilder(ohlcv: &[(u64, f32, f32, f32, f32)], period: usize) -> Option<Vec<f32>> {
    if ohlcv.len() < period + 1 {
        return None;
    }
    let mut tr = Vec::with_capacity(ohlcv.len());
    tr.push((ohlcv[0].1 - ohlcv[0].2).abs());
    for i in 1..ohlcv.len() {
        let (_, high, low, _close, _) = ohlcv[i];
        let prev_close = ohlcv[i - 1].3;
        let a = (high - low).abs();
        let b = (high - prev_close).abs();
        let c = (low - prev_close).abs();
        tr.push(a.max(b).max(c));
    }

    let mut atr = vec![0.0f32; ohlcv.len()];
    let mut sum = 0.0f32;
    for i in 1..=period {
        sum += tr[i];
    }
    let mut prev = sum / period as f32;
    atr[period] = prev;
    for i in (period + 1)..ohlcv.len() {
        prev = (prev * (period as f32 - 1.0) + tr[i]) / period as f32;
        atr[i] = prev;
    }
    // fill leading values with the first computed ATR so consumers can still index safely
    for i in 0..period {
        atr[i] = atr[period];
    }
    Some(atr)
}

fn keltner_last_two(
    ohlcv: &[(u64, f32, f32, f32, f32)],
    ema_period: u16,
    atr_period: u16,
    multiplier_x100: u16,
) -> Option<((f32, f32), (f32, f32))> {
    if ohlcv.len() < 3 {
        return None;
    }
    let ema_period = (ema_period as usize).max(2);
    let atr_period = (atr_period as usize).max(2);
    let mult = multiplier_x100 as f32 / 100.0;

    let closes: Vec<f32> = ohlcv.iter().map(|&(_, _, _, c, _)| c).collect();
    let mid = ema(&closes, ema_period);
    let atr = atr_series_wilder(ohlcv, atr_period)?;

    let prev = ohlcv.len() - 2;
    let cur = ohlcv.len() - 1;
    let p_upper = mid[prev] + atr[prev] * mult;
    let p_lower = mid[prev] - atr[prev] * mult;
    let c_upper = mid[cur] + atr[cur] * mult;
    let c_lower = mid[cur] - atr[cur] * mult;
    Some(((p_upper, p_lower), (c_upper, c_lower)))
}

fn supertrend_last_two(
    ohlcv: &[(u64, f32, f32, f32, f32)],
    atr_period: u16,
    multiplier_x100: u16,
) -> Option<((bool, f32), (bool, f32))> {
    let atr_period = (atr_period as usize).max(2);
    let mult = multiplier_x100 as f32 / 100.0;
    let atr = atr_series_wilder(ohlcv, atr_period)?;
    if ohlcv.len() < atr_period + 2 {
        return None;
    }

    let mut final_upper = 0.0f32;
    let mut final_lower = 0.0f32;
    let mut trend_up = true;

    let mut prev_pair: Option<(bool, f32)> = None;
    let mut cur_pair: Option<(bool, f32)> = None;

    for i in 1..ohlcv.len() {
        let (_, high, low, close, _) = ohlcv[i];
        let prev_close = ohlcv[i - 1].3;
        let hl2 = (high + low) * 0.5;
        let basic_upper = hl2 + mult * atr[i];
        let basic_lower = hl2 - mult * atr[i];

        if i == 1 {
            final_upper = basic_upper;
            final_lower = basic_lower;
        } else {
            // final upper
            if basic_upper < final_upper || prev_close > final_upper {
                final_upper = basic_upper;
            }
            // final lower
            if basic_lower > final_lower || prev_close < final_lower {
                final_lower = basic_lower;
            }
        }

        // trend
        if trend_up {
            if close < final_lower {
                trend_up = false;
            }
        } else if close > final_upper {
            trend_up = true;
        }

        let supertrend = if trend_up { final_lower } else { final_upper };

        if i == ohlcv.len() - 2 {
            prev_pair = Some((trend_up, supertrend));
        }
        if i == ohlcv.len() - 1 {
            cur_pair = Some((trend_up, supertrend));
        }
    }

    Some((prev_pair?, cur_pair?))
}

fn dmi_adx_last_two(
    ohlcv: &[(u64, f32, f32, f32, f32)],
    period: u16,
) -> Option<((f32, f32, f32), (f32, f32, f32))> {
    let p = (period as usize).max(2);
    if ohlcv.len() < (2 * p + 3) {
        return None;
    }

    let mut tr = Vec::with_capacity(ohlcv.len());
    let mut plus_dm = Vec::with_capacity(ohlcv.len());
    let mut minus_dm = Vec::with_capacity(ohlcv.len());
    tr.push((ohlcv[0].1 - ohlcv[0].2).abs());
    plus_dm.push(0.0);
    minus_dm.push(0.0);

    for i in 1..ohlcv.len() {
        let (_, high, low, _close, _) = ohlcv[i];
        let (_, prev_high, prev_low, prev_close, _) = ohlcv[i - 1];

        let up_move = high - prev_high;
        let down_move = prev_low - low;
        let pdm = if up_move > down_move && up_move > 0.0 { up_move } else { 0.0 };
        let mdm = if down_move > up_move && down_move > 0.0 { down_move } else { 0.0 };

        let a = (high - low).abs();
        let b = (high - prev_close).abs();
        let c = (low - prev_close).abs();
        let t = a.max(b).max(c);

        tr.push(t);
        plus_dm.push(pdm);
        minus_dm.push(mdm);
    }

    // Wilder smoothing for TR/+DM/-DM
    let mut sm_tr = tr[1..=p].iter().sum::<f32>();
    let mut sm_pdm = plus_dm[1..=p].iter().sum::<f32>();
    let mut sm_mdm = minus_dm[1..=p].iter().sum::<f32>();

    let mut di_plus = vec![0.0f32; ohlcv.len()];
    let mut di_minus = vec![0.0f32; ohlcv.len()];
    let mut dx = vec![0.0f32; ohlcv.len()];

    for i in p..ohlcv.len() {
        if i > p {
            sm_tr = sm_tr - (sm_tr / p as f32) + tr[i];
            sm_pdm = sm_pdm - (sm_pdm / p as f32) + plus_dm[i];
            sm_mdm = sm_mdm - (sm_mdm / p as f32) + minus_dm[i];
        }

        let pdi = if sm_tr == 0.0 { 0.0 } else { 100.0 * (sm_pdm / sm_tr) };
        let mdi = if sm_tr == 0.0 { 0.0 } else { 100.0 * (sm_mdm / sm_tr) };
        di_plus[i] = pdi;
        di_minus[i] = mdi;

        let denom = pdi + mdi;
        dx[i] = if denom == 0.0 {
            0.0
        } else {
            100.0 * (pdi - mdi).abs() / denom
        };
    }

    // ADX as Wilder smoothing of DX, starting with average of first p DX values after DI starts
    let adx_start = p * 2;
    if adx_start + 1 >= ohlcv.len() {
        return None;
    }
    let mut adx = vec![0.0f32; ohlcv.len()];
    let mut sum_dx = 0.0f32;
    for i in (p + 1)..=adx_start {
        sum_dx += dx[i];
    }
    let mut prev_adx = sum_dx / p as f32;
    adx[adx_start] = prev_adx;
    for i in (adx_start + 1)..ohlcv.len() {
        prev_adx = (prev_adx * (p as f32 - 1.0) + dx[i]) / p as f32;
        adx[i] = prev_adx;
    }
    // fill leading
    for i in 0..adx_start {
        adx[i] = adx[adx_start];
    }

    let prev = ohlcv.len() - 2;
    let cur = ohlcv.len() - 1;
    Some((
        (di_plus[prev], di_minus[prev], adx[prev]),
        (di_plus[cur], di_minus[cur], adx[cur]),
    ))
}

impl Default for State {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            modal: None,
            content: Content::Starter,
            settings: Settings::default(),
            streams: ResolvedStream::Waiting(vec![]),
            notifications: vec![],
            status: Status::Ready,
            link_group: None,
            rules: vec![],
            rules_expanded: None,
            rule_log: vec![],
            paper: PaperAccount::default(),
            rule_last_triggered_ms: HashMap::new(),
            settings_colors_expanded: HashSet::new(),
            indicators_query: String::new(),
            indicators_sidebar: IndicatorsSidebar::All,
            indicators_source: IndicatorsSource::BuiltIn,
            last_trade_price: None,
            prev_trade_price: None,
            pending_candle_close: None,
            rule_tick_dirty: false,
            rule_tick_last_eval: Instant::now(),
            centered_rules_size: (320.0, 560.0),
            centered_indicators_size: (640.0, 560.0),
            centered_rule_log_size: (420.0, 560.0),
        }
    }
}

#[derive(Default)]
pub enum Content {
    #[default]
    Starter,
    Heatmap {
        chart: Option<HeatmapChart>,
        indicators: Vec<HeatmapIndicator>,
        layout: data::chart::ViewConfig,
        studies: Vec<data::chart::heatmap::HeatmapStudy>,
    },
    Kline {
        chart: Option<KlineChart>,
        indicators: Vec<KlineIndicator>,
        layout: data::chart::ViewConfig,
        kind: data::chart::KlineChartKind,
    },
    TimeAndSales(Option<TimeAndSales>),
    Ladder(Option<Ladder>),
    Comparison(Option<ComparisonChart>),
}

impl Content {
    fn new_heatmap(
        current_content: &Content,
        ticker_info: TickerInfo,
        settings: &Settings,
        tick_size: f32,
    ) -> Self {
        let (enabled_indicators, layout, prev_studies) = if let Content::Heatmap {
            chart,
            indicators,
            studies,
            layout,
        } = current_content
        {
            (
                indicators.clone(),
                chart
                    .as_ref()
                    .map(|c| c.chart_layout())
                    .unwrap_or(layout.clone()),
                chart
                    .as_ref()
                    .map_or(studies.clone(), |c| c.studies.clone()),
            )
        } else {
            (
                vec![HeatmapIndicator::Volume],
                ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::CenterLatest),
                },
                vec![],
            )
        };

        let basis = settings
            .selected_basis
            .unwrap_or_else(|| Basis::default_heatmap_time(Some(ticker_info)));
        let config = settings.visual_config.clone().and_then(|cfg| cfg.heatmap());

        let chart = HeatmapChart::new(
            layout.clone(),
            basis,
            tick_size,
            &enabled_indicators,
            ticker_info,
            config,
            prev_studies.clone(),
        );

        Content::Heatmap {
            chart: Some(chart),
            indicators: enabled_indicators,
            layout,
            studies: prev_studies,
        }
    }

    fn new_kline(
        content_kind: ContentKind,
        current_content: &Content,
        ticker_info: TickerInfo,
        settings: &Settings,
        tick_size: f32,
    ) -> Self {
        let (prev_indis, prev_layout, prev_kind_opt) = if let Content::Kline {
            chart,
            indicators,
            kind,
            layout,
        } = current_content
        {
            (
                Some(indicators.clone()),
                Some(chart.as_ref().map_or(layout.clone(), |c| c.chart_layout())),
                Some(chart.as_ref().map_or(kind.clone(), |c| c.kind().clone())),
            )
        } else {
            (None, None, None)
        };

        let (default_tf, determined_chart_kind) = match content_kind {
            ContentKind::FootprintChart => (
                Timeframe::M5,
                prev_kind_opt
                    .filter(|k| matches!(k, data::chart::KlineChartKind::Footprint { .. }))
                    .unwrap_or_else(|| data::chart::KlineChartKind::Footprint {
                        clusters: data::chart::kline::ClusterKind::default(),
                        scaling: data::chart::kline::ClusterScaling::default(),
                        studies: vec![],
                    }),
            ),
            ContentKind::CandlestickChart => (Timeframe::M15, data::chart::KlineChartKind::Candles),
            _ => unreachable!("invalid content kind for kline chart"),
        };

        let basis = settings.selected_basis.unwrap_or(Basis::Time(default_tf));

        let enabled_indicators = {
            let available = KlineIndicator::for_market(ticker_info.market_type());
            prev_indis.map_or_else(
                || vec![KlineIndicator::Volume],
                |indis| {
                    indis
                        .into_iter()
                        .filter(|i| available.contains(i))
                        .collect()
                },
            )
        };

        let splits = {
            let main_chart_split: f32 = 0.8;
            let mut splits_vec = vec![main_chart_split];

            if !enabled_indicators.is_empty() {
                let num_indicators = enabled_indicators.len();

                if num_indicators > 0 {
                    let indicator_total_height_ratio = 1.0 - main_chart_split;
                    let height_per_indicator_pane =
                        indicator_total_height_ratio / num_indicators as f32;

                    let mut current_split_pos = main_chart_split;
                    for _ in 0..(num_indicators - 1) {
                        current_split_pos += height_per_indicator_pane;
                        splits_vec.push(current_split_pos);
                    }
                }
            }
            splits_vec
        };

        let layout = prev_layout
            .filter(|l| l.splits.len() == splits.len())
            .unwrap_or(ViewConfig {
                splits,
                autoscale: Some(data::chart::Autoscale::FitToVisible),
            });

        let mut chart = KlineChart::new(
            layout.clone(),
            basis,
            tick_size,
            &[],
            vec![],
            &enabled_indicators,
            ticker_info,
            &determined_chart_kind,
        );

        let visual_cfg = settings
            .visual_config
            .clone()
            .and_then(|cfg| cfg.kline())
            .unwrap_or_default();
        chart.set_visual_config(visual_cfg);

        Content::Kline {
            chart: Some(chart),
            indicators: enabled_indicators,
            layout,
            kind: determined_chart_kind,
        }
    }

    fn placeholder(kind: ContentKind) -> Self {
        match kind {
            ContentKind::Starter => Content::Starter,
            ContentKind::CandlestickChart => Content::Kline {
                chart: None,
                indicators: vec![KlineIndicator::Volume],
                kind: data::chart::KlineChartKind::Candles,
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::FitToVisible),
                },
            },
            ContentKind::FootprintChart => Content::Kline {
                chart: None,
                indicators: vec![KlineIndicator::Volume],
                kind: data::chart::KlineChartKind::Footprint {
                    clusters: data::chart::kline::ClusterKind::default(),
                    scaling: data::chart::kline::ClusterScaling::default(),
                    studies: vec![],
                },
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::FitToVisible),
                },
            },
            ContentKind::HeatmapChart => Content::Heatmap {
                chart: None,
                indicators: vec![HeatmapIndicator::Volume],
                studies: vec![],
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::CenterLatest),
                },
            },
            ContentKind::CandlesHeatmapChart => Content::Heatmap {
                chart: None,
                indicators: vec![HeatmapIndicator::Volume],
                studies: vec![],
                layout: ViewConfig {
                    splits: vec![],
                    autoscale: Some(data::chart::Autoscale::CenterLatest),
                },
            },
            ContentKind::ComparisonChart => Content::Comparison(None),
            ContentKind::TimeAndSales => Content::TimeAndSales(None),
            ContentKind::Ladder => Content::Ladder(None),
        }
    }

    pub fn last_tick(&self) -> Option<Instant> {
        match self {
            Content::Heatmap { chart, .. } => Some(chart.as_ref()?.last_update()),
            Content::Kline { chart, .. } => Some(chart.as_ref()?.last_update()),
            Content::TimeAndSales(panel) => Some(panel.as_ref()?.last_update()),
            Content::Ladder(panel) => Some(panel.as_ref()?.last_update()),
            Content::Comparison(chart) => Some(chart.as_ref()?.last_update()),
            Content::Starter => None,
        }
    }

    pub fn chart_kind(&self) -> Option<data::chart::KlineChartKind> {
        match self {
            Content::Kline { chart, .. } => Some(chart.as_ref()?.kind().clone()),
            _ => None,
        }
    }

    pub fn toggle_indicator(&mut self, indicator: UiIndicator) {
        match (self, indicator) {
            (
                Content::Heatmap {
                    chart, indicators, ..
                },
                UiIndicator::Heatmap(ind),
            ) => {
                let Some(chart) = chart else {
                    return;
                };

                if indicators.contains(&ind) {
                    indicators.retain(|i| i != &ind);
                } else {
                    indicators.push(ind);
                }
                chart.toggle_indicator(ind);
            }
            (
                Content::Kline {
                    chart, indicators, ..
                },
                UiIndicator::Kline(ind),
            ) => {
                let Some(chart) = chart else {
                    return;
                };

                if indicators.contains(&ind) {
                    indicators.retain(|i| i != &ind);
                } else {
                    indicators.push(ind);
                }
                chart.toggle_indicator(ind);
            }
            _ => panic!("indicator toggle on {indicator:?} pane",),
        }
    }

    pub fn reorder_indicators(&mut self, event: &column_drag::DragEvent) {
        match self {
            Content::Heatmap { indicators, .. } => column_drag::reorder_vec(indicators, event),
            Content::Kline { indicators, .. } => column_drag::reorder_vec(indicators, event),
            Content::TimeAndSales(_)
            | Content::Ladder(_)
            | Content::Starter
            | Content::Comparison(_) => {
                panic!("indicator reorder on {} pane", self)
            }
        }
    }

    pub fn change_visual_config(&mut self, config: VisualConfig) {
        match (self, config) {
            (Content::Heatmap { chart: Some(c), .. }, VisualConfig::Heatmap(cfg)) => {
                c.set_visual_config(cfg);
            }
            (Content::Kline { chart: Some(c), .. }, VisualConfig::Kline(cfg)) => {
                c.set_visual_config(cfg);
            }
            (Content::TimeAndSales(Some(panel)), VisualConfig::TimeAndSales(cfg)) => {
                panel.config = cfg;
            }
            (Content::Ladder(Some(panel)), VisualConfig::Ladder(cfg)) => {
                panel.config = cfg;
            }
            (Content::Comparison(Some(chart)), VisualConfig::Comparison(cfg)) => {
                chart.config = cfg;
            }
            _ => {}
        }
    }

    pub fn studies(&self) -> Option<data::chart::Study> {
        match &self {
            Content::Heatmap { studies, .. } => Some(data::chart::Study::Heatmap(studies.clone())),
            Content::Kline { kind, .. } => {
                if let data::chart::KlineChartKind::Footprint { studies, .. } = kind {
                    Some(data::chart::Study::Footprint(studies.clone()))
                } else {
                    None
                }
            }
            Content::TimeAndSales(_)
            | Content::Ladder(_)
            | Content::Starter
            | Content::Comparison(_) => None,
        }
    }

    pub fn update_studies(&mut self, studies: data::chart::Study) {
        match (self, studies) {
            (
                Content::Heatmap {
                    chart,
                    studies: previous,
                    ..
                },
                data::chart::Study::Heatmap(studies),
            ) => {
                chart
                    .as_mut()
                    .expect("heatmap chart not initialized")
                    .studies = studies.clone();
                *previous = studies;
            }
            (Content::Kline { chart, kind, .. }, data::chart::Study::Footprint(studies)) => {
                chart
                    .as_mut()
                    .expect("kline chart not initialized")
                    .set_studies(studies.clone());
                if let data::chart::KlineChartKind::Footprint {
                    studies: k_studies, ..
                } = kind
                {
                    *k_studies = studies;
                }
            }
            _ => {}
        }
    }

    pub fn kind(&self) -> ContentKind {
        match self {
            Content::Heatmap { chart, .. } => {
                let show_candles = chart
                    .as_ref()
                    .map(|c| c.visual_config().show_candles)
                    .unwrap_or(false);
                if show_candles {
                    ContentKind::CandlesHeatmapChart
                } else {
                    ContentKind::HeatmapChart
                }
            }
            Content::Kline { kind, .. } => match kind {
                data::chart::KlineChartKind::Footprint { .. } => ContentKind::FootprintChart,
                data::chart::KlineChartKind::Candles => ContentKind::CandlestickChart,
                data::chart::KlineChartKind::CandlesStudied { .. } => ContentKind::CandlestickChart,
            },
            Content::TimeAndSales(_) => ContentKind::TimeAndSales,
            Content::Ladder(_) => ContentKind::Ladder,
            Content::Comparison(_) => ContentKind::ComparisonChart,
            Content::Starter => ContentKind::Starter,
        }
    }

    fn initialized(&self) -> bool {
        match self {
            Content::Heatmap { chart, .. } => chart.is_some(),
            Content::Kline { chart, .. } => chart.is_some(),
            Content::TimeAndSales(panel) => panel.is_some(),
            Content::Ladder(panel) => panel.is_some(),
            Content::Comparison(chart) => chart.is_some(),
            Content::Starter => true,
        }
    }
}

impl std::fmt::Display for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind())
    }
}

impl PartialEq for Content {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Content::Starter, Content::Starter)
                | (Content::Heatmap { .. }, Content::Heatmap { .. })
                | (Content::Kline { .. }, Content::Kline { .. })
                | (Content::TimeAndSales(_), Content::TimeAndSales(_))
                | (Content::Ladder(_), Content::Ladder(_))
        )
    }
}

fn link_group_modal<'a>(
    pane: pane_grid::Pane,
    selected_group: Option<LinkGroup>,
) -> Element<'a, Message> {
    let mut grid = column![].spacing(4);
    let rows = LinkGroup::ALL.chunks(3);

    for row_groups in rows {
        let mut button_row = row![].spacing(4);

        for &group in row_groups {
            let is_selected = selected_group == Some(group);
            let btn_content = text(group.to_string()).font(style::AZERET_MONO);

            let btn = if is_selected {
                button_with_tooltip(
                    btn_content.align_x(iced::Alignment::Center),
                    Message::SwitchLinkGroup(pane, None),
                    Some("Unlink"),
                    tooltip::Position::Bottom,
                    move |theme, status| style::button::menu_body(theme, status, true),
                )
            } else {
                button(btn_content.align_x(iced::Alignment::Center))
                    .on_press(Message::SwitchLinkGroup(pane, Some(group)))
                    .style(move |theme, status| style::button::menu_body(theme, status, false))
                    .into()
            };

            button_row = button_row.push(btn);
        }

        grid = grid.push(button_row);
    }

    container(grid)
        .max_width(240)
        .padding(16)
        .style(style::chart_modal)
        .into()
}

fn ticksize_modifier<'a>(
    id: pane_grid::Pane,
    base_ticksize: f32,
    multiplier: TickMultiplier,
    modifier: Option<modal::stream::Modifier>,
    kind: ModifierKind,
    exchange: Option<exchange::adapter::Exchange>,
) -> Element<'a, Message> {
    let modifier_modal = Modal::StreamModifier(
        modal::stream::Modifier::new(kind).with_ticksize_view(base_ticksize, multiplier, exchange),
    );

    let is_active = modifier.is_some_and(|m| {
        matches!(
            m.view_mode,
            modal::stream::ViewMode::TicksizeSelection { .. }
        )
    });

    button(text(multiplier.to_string()))
        .style(move |theme, status| style::button::modifier(theme, status, !is_active))
        .on_press(Message::PaneEvent(id, Event::ShowModal(modifier_modal)))
        .into()
}

fn go_to_latest_button<'a>(id: pane_grid::Pane) -> Element<'a, Message> {
    tooltip(
        button(icon_text(Icon::Return, 12))
            .on_press(Message::PaneEvent(id, Event::GoToLatest))
            .style(|theme, status| style::button::transparent(theme, status, false)),
        Some("Go to latest candle"),
        crate::TooltipPosition::Top,
    )
    .into()
}

fn basis_modifier<'a>(
    id: pane_grid::Pane,
    selected_basis: Basis,
    modifier: Option<modal::stream::Modifier>,
    kind: ModifierKind,
) -> Element<'a, Message> {
    let modifier_modal = Modal::StreamModifier(
        modal::stream::Modifier::new(kind).with_view_mode(modal::stream::ViewMode::BasisSelection),
    );

    let is_active =
        modifier.is_some_and(|m| m.view_mode == modal::stream::ViewMode::BasisSelection);

    button(text(selected_basis.to_string()))
        .style(move |theme, status| style::button::modifier(theme, status, !is_active))
        .on_press(Message::PaneEvent(id, Event::ShowModal(modifier_modal)))
        .into()
}

fn by_basis_default<T>(
    basis: Option<Basis>,
    default_tf: Timeframe,
    on_time: impl FnOnce(Timeframe) -> T,
    on_tick: impl FnOnce() -> T,
) -> T {
    match basis.unwrap_or(Basis::Time(default_tf)) {
        Basis::Time(tf) => on_time(tf),
        Basis::Tick(_) => on_tick(),
    }
}

fn resolve_heatmap_candle_tf(
    cfg: data::chart::heatmap::Config,
    basis: Option<Basis>,
) -> Option<Timeframe> {
    if !cfg.show_candles {
        return None;
    }

    if let Some(tf) = cfg.candle_timeframe {
        return Some(tf);
    }

    let basis_tf = match basis {
        Some(Basis::Time(tf)) => tf,
        _ => Timeframe::M5,
    };

    Some(data::chart::heatmap::default_candle_timeframe(basis_tf))
}
