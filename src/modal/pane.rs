use iced::{
    Alignment, Element, Length, padding,
    widget::{container, mouse_area, opaque, space},
    Color,
};

pub mod indicators;
pub mod mini_tickers_list;
pub mod rule_log;
pub mod rules;
pub mod settings;
pub mod stream;

#[derive(Debug, Clone, PartialEq)]
pub enum Modal {
    StreamModifier(super::stream::Modifier),
    MiniTickersList(mini_tickers_list::MiniPanel),
    Settings,
    Indicators,
    Rules,
    RuleLog,
    ContextMenu(iced::Point),
    LinkGroup,
    Controls,
}

pub fn stack_context_menu<'a, Message>(
    base: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    on_blur: Message,
    pos: iced::Point,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    iced::widget::stack![
        base.into(),
        // Background click-catcher (close on click outside the menu)
        mouse_area(container(space::Space::new()).width(Length::Fill).height(Length::Fill)).on_press(on_blur.clone()),
        // Actual menu content (opaque so it receives clicks and does not close)
        container(opaque(content))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(padding::Padding {
                left: pos.x,
                right: 0.0,
                top: pos.y,
                bottom: 0.0,
            })
            .align_x(Alignment::Start)
            .align_y(Alignment::Start)
            .style(|_theme| container::Style {
                shadow: iced::Shadow {
                    color: Color::TRANSPARENT,
                    ..Default::default()
                },
                ..Default::default()
            })
    ]
    .into()
}

pub fn stack_modal<'a, Message>(
    base: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    on_blur: Message,
    padding: padding::Padding,
    align_x: Alignment,
    align_y: Alignment,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    iced::widget::stack![
        base.into(),
        // Background click-catcher (close on click outside the modal)
        mouse_area(container(space::Space::new()).width(Length::Fill).height(Length::Fill)).on_press(on_blur.clone()),
        // Modal content (opaque so clicks inside don't close the modal)
        container(opaque(content))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(padding)
            .align_x(align_x)
            .align_y(align_y)
            .style(|_theme| container::Style {
                shadow: iced::Shadow {
                    color: Color::TRANSPARENT,
                    ..Default::default()
                },
                ..Default::default()
            })
    ]
    .into()
}
