use crate::screen::dashboard::pane::{self, IndicatorsSidebar, IndicatorsSource, Message};
use crate::modal::pane::settings::study;
use crate::style::{self, Icon, icon_text};
use crate::widget::{column_drag, dragger_row};

use data::chart::indicator::{Indicator, UiIndicator};
use iced::{
    Element, Length, padding,
    Alignment,
    widget::{button, column, container, pane_grid, row, scrollable, space, text, text_input},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum IndicatorCategory {
    Enabled,
    Volume,
    Derivatives,
    Oscillators,
    Trend,
    Volatility,
}

impl std::fmt::Display for IndicatorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndicatorCategory::Enabled => write!(f, "Enabled"),
            IndicatorCategory::Volume => write!(f, "Volume"),
            IndicatorCategory::Derivatives => write!(f, "Derivatives"),
            IndicatorCategory::Oscillators => write!(f, "Oscillators"),
            IndicatorCategory::Trend => write!(f, "Trend"),
            IndicatorCategory::Volatility => write!(f, "Volatility"),
        }
    }
}

pub(crate) trait Categorize {
    fn category(&self) -> IndicatorCategory;
}

impl Categorize for data::chart::indicator::KlineIndicator {
    fn category(&self) -> IndicatorCategory {
        use data::chart::indicator::KlineIndicator as K;
        match self {
            K::Volume => IndicatorCategory::Volume,
            K::OpenInterest => IndicatorCategory::Derivatives,
            K::Rsi | K::StochRsi => IndicatorCategory::Oscillators,
            K::Macd | K::DmiAdx => IndicatorCategory::Trend,
            K::Atr => IndicatorCategory::Volatility,
        }
    }
}

impl Categorize for data::chart::indicator::HeatmapIndicator {
    fn category(&self) -> IndicatorCategory {
        IndicatorCategory::Volume
    }
}

fn sidebar_button<'a>(
    pane: pane_grid::Pane,
    current: IndicatorsSidebar,
    item: IndicatorsSidebar,
    label: &'static str,
) -> Element<'a, Message> {
    let selected = current == item;
    button(
        row![text(label).size(13), space::horizontal()]
            .align_y(Alignment::Center)
            .padding(2),
    )
    .width(Length::Fill)
    .on_press(Message::PaneEvent(pane, pane::Event::IndicatorsSidebarSelected(item)))
    .style(move |theme, status| style::button::modifier(theme, status, selected))
    .into()
}

fn source_tabs<'a>(pane: pane_grid::Pane, current: IndicatorsSource) -> Element<'a, Message> {
    let built_in_selected = current == IndicatorsSource::BuiltIn;
    let community_selected = current == IndicatorsSource::Community;

    let built_in = button(text("Built‑in").size(13))
        .on_press(Message::PaneEvent(
            pane,
            pane::Event::IndicatorsSourceSelected(IndicatorsSource::BuiltIn),
        ))
        .style(move |theme, status| style::button::modifier(theme, status, built_in_selected));

    let community = button(text("Community").size(13))
        .on_press(Message::PaneEvent(
            pane,
            pane::Event::IndicatorsSourceSelected(IndicatorsSource::Community),
        ))
        .style(move |theme, status| style::button::modifier(theme, status, community_selected));

    row![built_in, community].spacing(8).into()
}

pub fn view<'a, I>(
    pane: pane_grid::Pane,
    state: &'a pane::State,
    selected: &[I],
    market_type: Option<exchange::adapter::MarketKind>,
) -> Element<'a, Message>
where
    I: Indicator + Copy + Into<UiIndicator> + Categorize,
{
    let query = state.indicators_query.as_str();
    let sidebar = state.indicators_sidebar;
    let source = state.indicators_source;

    let search = text_input("Find an indicator", &state.indicators_query)
        .on_input(move |v| Message::PaneEvent(pane, pane::Event::IndicatorsQueryChanged(v)))
        .padding(10);

    let tabs = source_tabs(pane, source);

    let sidebar_col = column![
        sidebar_button(pane, sidebar, IndicatorsSidebar::All, "All"),
        sidebar_button(pane, sidebar, IndicatorsSidebar::Enabled, "Enabled"),
        sidebar_button(pane, sidebar, IndicatorsSidebar::Volume, "Volume"),
        sidebar_button(pane, sidebar, IndicatorsSidebar::Oscillators, "Oscillators"),
        sidebar_button(pane, sidebar, IndicatorsSidebar::Trend, "Trend"),
        sidebar_button(pane, sidebar, IndicatorsSidebar::Volatility, "Volatility"),
        sidebar_button(pane, sidebar, IndicatorsSidebar::Derivatives, "Derivatives"),
        // overlays only make sense for kline panes
        if matches!(state.content, pane::Content::Kline { .. }) {
            sidebar_button(pane, sidebar, IndicatorsSidebar::Overlays, "Overlays (Candles)")
        } else {
            row![].into()
        },
    ]
    .spacing(6)
    .width(Length::Fixed(140.0));

    if source == IndicatorsSource::Community {
        let body = column![
            tabs,
            text("Community / marketplace / plugins: coming soon.").size(13),
            text("For now: use Built‑in indicators.").size(12),
        ]
        .spacing(10);

        let body = scrollable(body).height(Length::Fill);
        let footer = row![space::horizontal(), text("Drag bottom-right corner ↘ to resize").size(11)]
            .align_y(Alignment::Center);

        return container(column![body, footer].spacing(8))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(16)
            .style(style::chart_modal)
            .into();
    }

    let content_allows_dragging = matches!(state.content, pane::Content::Kline { .. });
    let content_row = if let Some(market) = market_type {
        content_row(pane, selected, market, content_allows_dragging, query, sidebar)
    } else {
        column![].spacing(4).into()
    };

    // Extra section for candlestick overlays (MAs) when on a Kline pane.
    let overlays: Element<_> = match &state.content {
        pane::Content::Kline { chart: Some(c), kind, .. } => {
            if sidebar != IndicatorsSidebar::All && sidebar != IndicatorsSidebar::Overlays {
                return column![].into();
            }
            let candle_studies: &[data::chart::kline::CandleStudy] = match kind {
                data::chart::KlineChartKind::CandlesStudied { studies } => studies,
                _ => &[],
            };

            let basis = c.basis();
            let overlay_cfg = c
                .candle_study_configurator()
                .view(candle_studies, basis)
                .map(move |msg| {
                    Message::PaneEvent(
                        pane,
                        pane::Event::StudyConfigurator(study::StudyMessage::Candle(msg)),
                    )
                });

            column![
                container(text("Overlays (Candles)").size(14)).padding(padding::top(10).bottom(6)),
                overlay_cfg
            ]
            .spacing(4)
            .into()
        }
        _ => column![].into(),
    };

    let inner = column![
        tabs,
        search,
        row![
            sidebar_col,
            space::horizontal().width(Length::Fixed(12.0)),
            column![content_row, overlays].spacing(10)
        ]
        .spacing(8)
    ]
        .spacing(12);

    let body = scrollable(inner).height(Length::Fill);
    let footer = row![space::horizontal(), text("Drag bottom-right corner ↘ to resize").size(11)]
        .align_y(Alignment::Center);

    container(column![body, footer].spacing(8))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(16)
        .style(style::chart_modal)
        .into()
}

fn build_indicator_row<'a, I>(
    pane: pane_grid::Pane,
    indicator: &I,
    is_selected: bool,
) -> Element<'a, Message>
where
    I: Indicator + Copy + Into<UiIndicator> + Categorize,
{
    let content = if is_selected {
        row![
            text(indicator.to_string()),
            space::horizontal(),
            container(icon_text(Icon::Checkmark, 12)),
        ]
        .width(Length::Fill)
    } else {
        row![text(indicator.to_string())].width(Length::Fill)
    };

    button(content)
        .on_press(Message::PaneEvent(
            pane,
            pane::Event::ToggleIndicator((*indicator).into()),
        ))
        .width(Length::Fill)
        .style(move |theme, status| style::button::modifier(theme, status, is_selected))
        .into()
}

fn selected_list<'a, I>(
    pane: pane_grid::Pane,
    selected: &[I],
    reorderable: bool,
) -> Element<'a, Message>
where
    I: Indicator + Copy + Into<UiIndicator> + Categorize,
{
    let elements: Vec<Element<_>> = selected
        .iter()
        .map(|indicator| {
            let base = build_indicator_row(pane, indicator, true);
            dragger_row(base, reorderable)
        })
        .collect();

    if reorderable {
        let mut draggable_column = column_drag::Column::new()
            .on_drag(move |event| Message::PaneEvent(pane, pane::Event::ReorderIndicator(event)))
            .spacing(4);
        for element in elements {
            draggable_column = draggable_column.push(element);
        }
        draggable_column.into()
    } else {
        iced::widget::Column::with_children(elements)
            .spacing(4)
            .into()
    }
}

fn available_list<'a, I>(pane: pane_grid::Pane, available: &[I]) -> Element<'a, Message>
where
    I: Indicator + Copy + Into<UiIndicator> + Categorize,
{
    let elements: Vec<Element<_>> = available
        .iter()
        .map(|indicator| {
            let base = build_indicator_row(pane, indicator, false);
            dragger_row(base, false)
        })
        .collect();

    iced::widget::Column::with_children(elements)
        .spacing(4)
        .into()
}

fn content_row<'a, I>(
    pane: pane_grid::Pane,
    selected: &[I],
    market: exchange::adapter::MarketKind,
    allows_drag: bool,
    query: &'a str,
    sidebar: IndicatorsSidebar,
) -> Element<'a, Message>
where
    I: Indicator + Copy + Into<UiIndicator> + Categorize,
{
    let reorderable = allows_drag && selected.len() >= 2;

    let q = query.trim().to_lowercase();
    let matches_query = |s: &str| {
        if q.is_empty() {
            true
        } else {
            s.to_lowercase().contains(&q)
        }
    };

    let selected_filtered: Vec<I> = selected
        .iter()
        .copied()
        .filter(|i| matches_query(&i.to_string()))
        .collect();

    let available: Vec<I> = I::for_market(market)
        .iter()
        .filter(|indicator| !selected.contains(indicator))
        .filter(|indicator| matches_query(&indicator.to_string()))
        .cloned()
        .collect();

    let mut groups: std::collections::BTreeMap<IndicatorCategory, Vec<I>> = std::collections::BTreeMap::new();
    for ind in &available {
        groups.entry(ind.category()).or_default().push(*ind);
    }

    let enabled_section: Element<_> = if selected_filtered.is_empty() {
        column![
            container(text(IndicatorCategory::Enabled.to_string()).size(14)).padding(padding::bottom(6)),
            text("No enabled indicators").size(12),
        ]
        .spacing(4)
        .into()
    } else {
        column![
            container(text(IndicatorCategory::Enabled.to_string()).size(14)).padding(padding::bottom(6)),
            selected_list(pane, &selected_filtered, reorderable),
        ]
        .spacing(4)
        .into()
    };

    let mut available_sections = column![].spacing(10);
    for (cat, inds) in groups {
        if inds.is_empty() {
            continue;
        }

        let show_cat = match sidebar {
            IndicatorsSidebar::All => true,
            IndicatorsSidebar::Enabled => false,
            IndicatorsSidebar::Volume => cat == IndicatorCategory::Volume,
            IndicatorsSidebar::Derivatives => cat == IndicatorCategory::Derivatives,
            IndicatorsSidebar::Oscillators => cat == IndicatorCategory::Oscillators,
            IndicatorsSidebar::Trend => cat == IndicatorCategory::Trend,
            IndicatorsSidebar::Volatility => cat == IndicatorCategory::Volatility,
            IndicatorsSidebar::Overlays => false,
        };
        if !show_cat {
            continue;
        }

        available_sections = available_sections.push(
            column![
                container(text(cat.to_string()).size(14)).padding(padding::top(6).bottom(6)),
                available_list(pane, &inds),
            ]
            .spacing(4),
        );
    }

    column![
        if matches!(sidebar, IndicatorsSidebar::All | IndicatorsSidebar::Enabled) { enabled_section } else { column![].into() },
        available_sections
    ]
    .spacing(8)
    .into()
}
