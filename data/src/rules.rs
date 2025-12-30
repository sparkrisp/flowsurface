use serde::{Deserialize, Serialize};
use std::fmt;

/// How often to evaluate a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum EvaluationMode {
    /// Evaluate using live/last-trade updates (fast path).
    OnTick,
    /// Evaluate only when a candle closes (time-based basis).
    OnCandleClose,
    /// Evaluate on both.
    Both,
}

impl Default for EvaluationMode {
    fn default() -> Self {
        Self::OnTick
    }
}

impl fmt::Display for EvaluationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvaluationMode::OnTick => write!(f, "On tick"),
            EvaluationMode::OnCandleClose => write!(f, "On candle close"),
            EvaluationMode::Both => write!(f, "Both"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum CrossDirection {
    CrossUp,
    CrossDown,
}

impl fmt::Display for CrossDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrossDirection::CrossUp => write!(f, "Cross up"),
            CrossDirection::CrossDown => write!(f, "Cross down"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum CompareDirection {
    Above,
    Below,
}

impl fmt::Display for CompareDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompareDirection::Above => write!(f, "Above"),
            CompareDirection::Below => write!(f, "Below"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Side {
    Buy,
    Sell,
}

/// A condition that can be evaluated on a candlestick chart pane.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum RuleCondition {
    /// Fast MA crosses Slow MA (uses the pane's configured MA overlays).
    MovingAverageCross { direction: CrossDirection },
    /// Last trade price crosses a fixed horizontal level.
    PriceCrossLevel { level: f32, direction: CrossDirection },
    /// Candle close crosses a fixed horizontal level (evaluated on close).
    CandleCloseCrossLevel { level: f32, direction: CrossDirection },
    /// Candle volume compared to a fixed threshold.
    VolumeIs { value: f32, direction: CompareDirection },
    /// RSI crosses a fixed level (uses the pane's RSI period config).
    RsiCrossLevel { level: f32, direction: CrossDirection },
    /// MACD crosses Signal (uses the pane's MACD config).
    MacdCrossSignal { direction: CrossDirection },
    /// Price crosses the pane's configured VWAP line (from VWAP Bands overlay).
    VwapCross { direction: CrossDirection },
    /// Supertrend flips direction (uses pane's Supertrend overlay).
    SupertrendFlip { direction: CrossDirection },
    /// Price crosses the Supertrend line (uses pane's Supertrend overlay).
    SupertrendLineCross { direction: CrossDirection },
    /// Donchian breakout: close crosses the upper/lower channel (uses pane's Donchian overlay).
    DonchianBreakout { direction: CrossDirection },
    /// Keltner breakout: close crosses the upper/lower channel (uses pane's Keltner overlay).
    KeltnerBreakout { direction: CrossDirection },
    /// DMI direction cross: +DI crosses -DI.
    DmiCross { direction: CrossDirection },
    /// ADX compared to a fixed threshold (Above/Below).
    AdxIs { value: f32, direction: CompareDirection },
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum RuleAction {
    Toast { message: String },
    Sound { enabled: bool },
    Telegram { enabled: bool },
    /// Push notification (mobile) via ntfy. Uses env vars in the app:
    /// - FLOWSURFACE_NTFY_URL (default: https://ntfy.sh)
    /// - FLOWSURFACE_NTFY_TOPIC (required)
    Push { enabled: bool },
    PaperTrade {
        side: Side,
        /// Percent of the current paper balance to allocate, e.g. 25.0 == 25%
        percent_of_balance: f32,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RuleSpec {
    pub id: uuid::Uuid,
    pub name: String,
    pub enabled: bool,
    pub evaluation: EvaluationMode,
    pub condition: RuleCondition,
    pub actions: Vec<RuleAction>,
    /// Minimum time between triggers (ms). 0 disables cooldown.
    pub cooldown_ms: u64,
}

impl Default for RuleSpec {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            name: "New rule".to_string(),
            enabled: true,
            evaluation: EvaluationMode::default(),
            condition: RuleCondition::MovingAverageCross {
                direction: CrossDirection::CrossUp,
            },
            actions: vec![RuleAction::Toast {
                message: "Rule triggered".to_string(),
            }],
            cooldown_ms: 1_000,
        }
    }
}


