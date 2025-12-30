use crate::screen::dashboard::pane::{self, Event, Message};
use crate::style::{self, Icon, icon_text};
use crate::widget::button_with_tooltip;

use data::rules::{CompareDirection, CrossDirection, EvaluationMode, RuleAction, RuleCondition};
use iced::{
    Alignment, Element, Length,
    widget::{button, checkbox, column, container, pick_list, row, scrollable, space, text, text_input},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionKind {
    PriceCrossLevel,
    CandleCloseCrossLevel,
    VolumeIs,
    MovingAverageCross,
    RsiCrossLevel,
    MacdCrossSignal,
    VwapCross,
    SupertrendFlip,
    SupertrendLineCross,
    DonchianBreakout,
    KeltnerBreakout,
    DmiCross,
    AdxIs,
}

impl std::fmt::Display for ConditionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConditionKind::PriceCrossLevel => write!(f, "Price crosses level"),
            ConditionKind::CandleCloseCrossLevel => write!(f, "Candle close vs level"),
            ConditionKind::VolumeIs => write!(f, "Candle volume vs value"),
            ConditionKind::MovingAverageCross => write!(f, "MA cross (Fast vs Slow)"),
            ConditionKind::RsiCrossLevel => write!(f, "RSI crosses level"),
            ConditionKind::MacdCrossSignal => write!(f, "MACD crosses Signal"),
            ConditionKind::VwapCross => write!(f, "Price crosses VWAP"),
            ConditionKind::SupertrendFlip => write!(f, "Supertrend flips"),
            ConditionKind::SupertrendLineCross => write!(f, "Price crosses Supertrend line"),
            ConditionKind::DonchianBreakout => write!(f, "Donchian breakout"),
            ConditionKind::KeltnerBreakout => write!(f, "Keltner breakout"),
            ConditionKind::DmiCross => write!(f, "DMI cross (+DI vs -DI)"),
            ConditionKind::AdxIs => write!(f, "ADX vs value"),
        }
    }
}

pub fn view<'a>(pane_id: iced::widget::pane_grid::Pane, state: &'a pane::State) -> Element<'a, Message> {
    let tooltip_pos = iced::widget::tooltip::Position::Top;
    let mut col = column![text("Rules").size(14)].spacing(12);

    let add_btn = button_with_tooltip(
        icon_text(Icon::Clone, 12),
        Message::PaneEvent(pane_id, Event::AddRule),
        Some("Add rule"),
        tooltip_pos,
        style::button::info,
    );

    col = col.push(row![add_btn, space::horizontal()].align_y(Alignment::Center));

    if state.rules.is_empty() {
        col = col.push(text("No rules yet").size(13));
    } else {
        for rule in &state.rules {
            let is_expanded = state
                .rules_expanded
                .is_some_and(|id| id == rule.id);

            let header = {
                let enabled = checkbox(rule.enabled)
                    .label(rule.name.clone())
                    .on_toggle(move |checked| Message::PaneEvent(pane_id, Event::ToggleRule(rule.id, checked)));

                let toggle = button(icon_text(Icon::Cog, 12))
                    .on_press(Message::PaneEvent(pane_id, Event::ToggleRuleCard(rule.id)))
                    .style(move |theme, status| style::button::transparent(theme, status, is_expanded));

                let delete = button(icon_text(Icon::TrashBin, 12))
                    .on_press(Message::PaneEvent(pane_id, Event::DeleteRule(rule.id)))
                    .style(move |theme, status| style::button::cancel(theme, status, false));

                row![enabled, space::horizontal(), toggle, delete]
                    .align_y(Alignment::Center)
                    .spacing(8)
                    .padding(8)
            };

            let mut card = column![header].spacing(8);

            if is_expanded {
                let name_input = text_input("Rule name", &rule.name)
                    .on_input(move |v| Message::PaneEvent(pane_id, Event::UpdateRuleName(rule.id, v)));

                let evaluation = pick_list(
                    [EvaluationMode::OnTick, EvaluationMode::OnCandleClose, EvaluationMode::Both],
                    Some(rule.evaluation),
                    move |m| Message::PaneEvent(pane_id, Event::UpdateRuleEvaluation(rule.id, m)),
                );

                let kind = match &rule.condition {
                    RuleCondition::PriceCrossLevel { .. } => ConditionKind::PriceCrossLevel,
                    RuleCondition::CandleCloseCrossLevel { .. } => ConditionKind::CandleCloseCrossLevel,
                    RuleCondition::VolumeIs { .. } => ConditionKind::VolumeIs,
                    RuleCondition::MovingAverageCross { .. } => ConditionKind::MovingAverageCross,
                    RuleCondition::RsiCrossLevel { .. } => ConditionKind::RsiCrossLevel,
                    RuleCondition::MacdCrossSignal { .. } => ConditionKind::MacdCrossSignal,
                    RuleCondition::VwapCross { .. } => ConditionKind::VwapCross,
                    RuleCondition::SupertrendFlip { .. } => ConditionKind::SupertrendFlip,
                    RuleCondition::SupertrendLineCross { .. } => ConditionKind::SupertrendLineCross,
                    RuleCondition::DonchianBreakout { .. } => ConditionKind::DonchianBreakout,
                    RuleCondition::KeltnerBreakout { .. } => ConditionKind::KeltnerBreakout,
                    RuleCondition::DmiCross { .. } => ConditionKind::DmiCross,
                    RuleCondition::AdxIs { .. } => ConditionKind::AdxIs,
                };

                let condition_kind = pick_list(
                    [
                        ConditionKind::PriceCrossLevel,
                        ConditionKind::CandleCloseCrossLevel,
                        ConditionKind::VolumeIs,
                        ConditionKind::MovingAverageCross,
                        ConditionKind::RsiCrossLevel,
                        ConditionKind::MacdCrossSignal,
                        ConditionKind::VwapCross,
                        ConditionKind::SupertrendFlip,
                        ConditionKind::SupertrendLineCross,
                        ConditionKind::DonchianBreakout,
                        ConditionKind::KeltnerBreakout,
                        ConditionKind::DmiCross,
                        ConditionKind::AdxIs,
                    ],
                    Some(kind),
                    move |k| Message::PaneEvent(pane_id, Event::UpdateRuleConditionKind(rule.id, k)),
                );

                let level_value = match &rule.condition {
                    RuleCondition::PriceCrossLevel { level, .. }
                    | RuleCondition::CandleCloseCrossLevel { level, .. }
                    | RuleCondition::RsiCrossLevel { level, .. } => level.to_string(),
                    RuleCondition::VolumeIs { value, .. } => value.to_string(),
                    RuleCondition::AdxIs { value, .. } => value.to_string(),
                    _ => "".to_string(),
                };

                let level_label = match kind {
                    ConditionKind::VolumeIs => "Value",
                    ConditionKind::RsiCrossLevel => "RSI level",
                    ConditionKind::MovingAverageCross | ConditionKind::MacdCrossSignal => "",
                    ConditionKind::VwapCross
                    | ConditionKind::SupertrendFlip
                    | ConditionKind::SupertrendLineCross
                    | ConditionKind::DonchianBreakout
                    | ConditionKind::KeltnerBreakout
                    | ConditionKind::DmiCross => "",
                    ConditionKind::AdxIs => "ADX value",
                    _ => "Level",
                };

                let level_input: Element<_> = if !level_label.is_empty() {
                    text_input(level_label, &level_value)
                        .on_input(move |v| {
                            Message::PaneEvent(pane_id, Event::UpdateRuleLevel(rule.id, v))
                        })
                        .into()
                } else {
                    row![].into()
                };

                let cross_dir = match &rule.condition {
                    RuleCondition::PriceCrossLevel { direction, .. }
                    | RuleCondition::CandleCloseCrossLevel { direction, .. }
                    | RuleCondition::MovingAverageCross { direction, .. }
                    | RuleCondition::RsiCrossLevel { direction, .. }
                    | RuleCondition::MacdCrossSignal { direction, .. }
                    | RuleCondition::VwapCross { direction }
                    | RuleCondition::SupertrendFlip { direction }
                    | RuleCondition::SupertrendLineCross { direction }
                    | RuleCondition::DonchianBreakout { direction }
                    | RuleCondition::KeltnerBreakout { direction }
                    | RuleCondition::DmiCross { direction } => Some(*direction),
                    _ => None,
                };

                let compare_dir = match &rule.condition {
                    RuleCondition::VolumeIs { direction, .. } | RuleCondition::AdxIs { direction, .. } => {
                        Some(*direction)
                    }
                    _ => None,
                };

                let cross_pick: Element<_> = if let Some(cur) = cross_dir {
                    pick_list(
                        [CrossDirection::CrossUp, CrossDirection::CrossDown],
                        Some(cur),
                        move |d| Message::PaneEvent(pane_id, Event::UpdateRuleCrossDirection(rule.id, d)),
                    )
                    .into()
                } else {
                    row![].into()
                };

                let compare_pick: Element<_> = if let Some(cur) = compare_dir {
                    pick_list(
                        [CompareDirection::Above, CompareDirection::Below],
                        Some(cur),
                        move |d| Message::PaneEvent(pane_id, Event::UpdateRuleCompareDirection(rule.id, d)),
                    )
                    .into()
                } else {
                    row![].into()
                };

                let toast_enabled = rule.actions.iter().any(|a| matches!(a, RuleAction::Toast { .. }));
                let toast_msg = rule
                    .actions
                    .iter()
                    .find_map(|a| match a {
                        RuleAction::Toast { message } => Some(message.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "Rule triggered".to_string());

                let toast_toggle = checkbox(toast_enabled)
                    .label("Toast")
                    .on_toggle(move |on| Message::PaneEvent(pane_id, Event::ToggleRuleActionToast(rule.id, on)));

                let toast_input = text_input("Toast message", &toast_msg)
                    .on_input(move |v| Message::PaneEvent(pane_id, Event::UpdateRuleToastMessage(rule.id, v)));

                let sound_enabled = rule.actions.iter().any(|a| matches!(a, RuleAction::Sound { enabled: true }));
                let sound_toggle = checkbox(sound_enabled)
                    .label("Sound")
                    .on_toggle(move |on| Message::PaneEvent(pane_id, Event::ToggleRuleActionSound(rule.id, on)));

                let telegram_enabled =
                    rule.actions.iter().any(|a| matches!(a, RuleAction::Telegram { enabled: true }));
                let telegram_toggle = checkbox(telegram_enabled)
                    .label("Telegram")
                    .on_toggle(move |on| {
                        Message::PaneEvent(pane_id, Event::ToggleRuleActionTelegram(rule.id, on))
                    });

                let push_enabled =
                    rule.actions.iter().any(|a| matches!(a, RuleAction::Push { enabled: true }));
                let push_toggle = checkbox(push_enabled)
                    .label("Push (ntfy)")
                    .on_toggle(move |on| {
                        Message::PaneEvent(pane_id, Event::ToggleRuleActionPush(rule.id, on))
                    });

                let paper_enabled = rule.actions.iter().any(|a| matches!(a, RuleAction::PaperTrade { .. }));
                let paper_toggle = checkbox(paper_enabled)
                    .label("PaperTrade (%)")
                    .on_toggle(move |on| Message::PaneEvent(pane_id, Event::ToggleRuleActionPaperTrade(rule.id, on)));

                let paper_pct = rule
                    .actions
                    .iter()
                    .find_map(|a| match a {
                        RuleAction::PaperTrade { percent_of_balance, .. } => Some(percent_of_balance.to_string()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "25".to_string());

                let paper_input = text_input("%", &paper_pct)
                    .width(Length::Fixed(80.0))
                    .on_input(move |v| Message::PaneEvent(pane_id, Event::UpdateRulePaperPercent(rule.id, v)));

                card = card.push(
                    column![
                        column![text("Name").size(12), name_input].spacing(4),
                        column![text("Evaluation").size(12), evaluation].spacing(4),
                        column![
                            text("Condition").size(12),
                            condition_kind,
                            cross_pick,
                            compare_pick,
                            level_input
                        ]
                        .spacing(6),
                        column![
                            text("Actions").size(12),
                            column![
                                toast_toggle,
                                toast_input,
                                sound_toggle,
                                telegram_toggle,
                                push_toggle,
                                row![paper_toggle, paper_input].spacing(8)
                            ]
                                .spacing(6)
                        ]
                        .spacing(6),
                    ]
                    .spacing(10)
                    .padding(8),
                );
            }

            col = col.push(container(card).style(style::modal_container));
        }
    }

    let body = scrollable(col).height(Length::Fill);
    let footer = row![space::horizontal(), text("Drag bottom-right corner ↘ to resize").size(11)]
        .align_y(Alignment::Center);

    container(column![body, footer].spacing(8))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(16)
        .style(style::chart_modal)
        .into()
}


