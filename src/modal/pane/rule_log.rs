use crate::screen::dashboard::pane::{self, Event, Message};
use crate::style::{self, Icon, icon_text};
use crate::widget::button_with_tooltip;

use iced::{
    Alignment, Element, Length,
    widget::{column, container, row, scrollable, space, text},
};

pub fn view<'a>(
    pane_id: iced::widget::pane_grid::Pane,
    state: &'a pane::State,
) -> Element<'a, Message> {
    let tooltip_pos = iced::widget::tooltip::Position::Top;

    let clear_btn = button_with_tooltip(
        icon_text(Icon::TrashBin, 12),
        Message::PaneEvent(pane_id, Event::ClearRuleLog),
        Some("Clear rule log"),
        tooltip_pos,
        move |theme, status| style::button::cancel(theme, status, false),
    );

    let header = row![
        text("Rule log").size(14),
        space::horizontal(),
        clear_btn
    ]
    .align_y(Alignment::Center);

    let body: Element<_> = if state.rule_log.is_empty() {
        text("No rule triggers yet").size(13).into()
    } else {
        // newest first
        let entries = state
            .rule_log
            .iter()
            .rev()
            .take(250)
            .fold(column![].spacing(8), |col, e| {
                col.push(
                    column![
                        text(format!("{}  {}", e.time_hms, e.rule_name)).size(12),
                        text(e.message.as_str()).size(12),
                    ]
                    .spacing(2),
                )
            });

        scrollable(entries).height(Length::Fill).into()
    };

    let footer = row![space::horizontal(), text("Drag bottom-right corner ↘ to resize").size(11)]
        .align_y(Alignment::Center);

    let content = column![header, body, footer].spacing(12);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(16)
        .style(style::chart_modal)
        .into()
}


