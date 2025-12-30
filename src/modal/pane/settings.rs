use crate::chart::comparison::ComparisonChart;
use crate::screen::dashboard::pane::{Event, Message, SettingsColorsSection};
use crate::screen::dashboard::panel::timeandsales;
use crate::split_column;
use crate::widget::{classic_slider_row, labeled_slider};
use crate::{style, tooltip, widget::scrollable_content};

use data::chart::heatmap::HeatmapStudy;
use data::chart::kline::FootprintStudy;
use data::chart::{
    KlineChartKind,
    heatmap::{self, CoalesceKind},
    kline::{CandleStudy, ClusterKind},
};
use data::layout::pane::VisualConfig;
use data::panel::ladder;
use data::panel::timeandsales::{StackedBar, StackedBarRatio};
use data::util::format_with_commas;

use exchange::Timeframe;
use iced::widget::{checkbox, space};
use iced::{
    Alignment, Element, Length,
    widget::{
        button, column, container, pane_grid, pick_list, radio, row, slider, text,
        tooltip::Position as TooltipPosition,
    },
};
use std::time::Duration;

fn cfg_view_container<'a, T>(max_width: u32, content: T) -> Element<'a, Message>
where
    T: Into<Element<'a, Message>>,
{
    container(scrollable_content(content))
        .width(Length::Shrink)
        .padding(28)
        .max_width(max_width)
        .style(style::chart_modal)
        .into()
}

fn rgb_to_color(rgb: [u8; 3]) -> iced::Color {
    iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2])
}

pub fn heatmap_cfg_view<'a>(
    cfg: heatmap::Config,
    pane: pane_grid::Pane,
    study_config: &'a study::Configurator<HeatmapStudy>,
    studies: &'a [HeatmapStudy],
    basis: data::chart::Basis,
) -> Element<'a, Message> {
    let default_candle_tf = match basis {
        data::chart::Basis::Time(tf) => Some(heatmap::default_candle_timeframe(tf)),
        data::chart::Basis::Tick(_) => None,
    };
    let candles_toggle = {
        let enabled = cfg.show_candles;
        checkbox(enabled)
            .label("Show candles overlay (beta)")
            .on_toggle(move |v| {
                let candle_timeframe = if v {
                    cfg.candle_timeframe.or(default_candle_tf)
                } else {
                    cfg.candle_timeframe
                };
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::Heatmap(heatmap::Config {
                        show_candles: v,
                        candle_timeframe,
                        ..cfg
                    }),
                    false,
                )
            })
    };

    let trade_size_slider = {
        let filter = cfg.trade_size_filter;
        labeled_slider(
            "Trade",
            0.0..=50000.0,
            filter,
            move |value| {
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::Heatmap(heatmap::Config {
                        trade_size_filter: value,
                        ..cfg
                    }),
                    false,
                )
            },
            |value| format!(">${}", format_with_commas(*value)),
            Some(500.0),
        )
    };

    let order_size_slider = {
        let filter = cfg.order_size_filter;
        labeled_slider(
            "Order",
            0.0..=500_000.0,
            filter,
            move |value| {
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::Heatmap(heatmap::Config {
                        order_size_filter: value,
                        ..cfg
                    }),
                    false,
                )
            },
            |value| format!(">${}", format_with_commas(*value)),
            Some(5000.0),
        )
    };

    let circle_scaling_slider = cfg.trade_size_scale.map(|radius_scale| {
        classic_slider_row(
            text("Circle radius scaling"),
            slider(10..=200, radius_scale, move |value| {
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::Heatmap(heatmap::Config {
                        trade_size_scale: Some(value),
                        ..cfg
                    }),
                    false,
                )
            })
            .step(10)
            .into(),
            Some(text(format!("{}%", radius_scale)).size(13)),
        )
    });

    let coalescer_cfg: Option<Element<_>> = if let Some(coalescing) = cfg.coalescing {
        let threshold_pct = coalescing.threshold();

        let coalescer_kinds = {
            let average = radio(
                "Average",
                CoalesceKind::Average(threshold_pct),
                Some(coalescing),
                move |value| {
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::Heatmap(heatmap::Config {
                            coalescing: Some(value),
                            ..cfg
                        }),
                        false,
                    )
                },
            )
            .spacing(4);

            let first = radio(
                "First",
                CoalesceKind::First(threshold_pct),
                Some(coalescing),
                move |value| {
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::Heatmap(heatmap::Config {
                            coalescing: Some(value),
                            ..cfg
                        }),
                        false,
                    )
                },
            )
            .spacing(4);

            let max = radio(
                "Max",
                CoalesceKind::Max(threshold_pct),
                Some(coalescing),
                move |value| {
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::Heatmap(heatmap::Config {
                            coalescing: Some(value),
                            ..cfg
                        }),
                        false,
                    )
                },
            )
            .spacing(4);

            row![
                text("Merge method: "),
                row![average, first, max].spacing(12)
            ]
            .spacing(12)
        };

        let threshold_slider = classic_slider_row(
            text("Size similarity"),
            slider(0.05..=0.8, threshold_pct, move |value| {
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::Heatmap(heatmap::Config {
                        coalescing: Some(coalescing.with_threshold(value)),
                        ..cfg
                    }),
                    false,
                )
            })
            .step(0.05)
            .into(),
            Some(text(format!("{:.0}%", threshold_pct * 100.0)).size(13)),
        );

        Some(
            container(column![coalescer_kinds, threshold_slider].spacing(8))
                .style(style::modal_container)
                .padding(8)
                .into(),
        )
    } else {
        None
    };

    let size_filters_column = column![
        text("Size filters").size(14),
        column![trade_size_slider, order_size_slider].spacing(8),
    ]
    .spacing(8);

    let noise_filters_column = {
        let merge_checkbox = checkbox(cfg.coalescing.is_some())
            .label("Merge orders if sizes are similar")
            .on_toggle(move |value| {
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::Heatmap(heatmap::Config {
                        coalescing: if value {
                            Some(CoalesceKind::Average(0.15))
                        } else {
                            None
                        },
                        ..cfg
                    }),
                    false,
                )
            });

        let mut col = column![text("Noise filters").size(14), merge_checkbox].spacing(8);
        if let Some(c) = coalescer_cfg {
            col = col.push(c);
        }
        col
    };

    let trade_viz_column = {
        let dyn_checkbox = checkbox(cfg.trade_size_scale.is_some())
            .label("Dynamic circle radius")
            .on_toggle(move |value| {
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::Heatmap(heatmap::Config {
                        trade_size_scale: if value { Some(100) } else { None },
                        ..cfg
                    }),
                    false,
                )
            });

        let mut col = column![text("Trade visualization").size(14), dyn_checkbox].spacing(8);
        if let Some(slider) = circle_scaling_slider {
            col = col.push(slider);
        }
        col
    };

    let study_cfg = study_config.view(studies, basis).map(move |msg| {
        Message::PaneEvent(
            pane,
            Event::StudyConfigurator(study::StudyMessage::Heatmap(msg)),
        )
    });

    let candle_tf_picker: Option<Element<'a, Message>> = if cfg.show_candles {
        match basis {
            data::chart::Basis::Time(tf) => {
                let default_tf = heatmap::default_candle_timeframe(tf);
                let selected_tf = cfg.candle_timeframe.unwrap_or(default_tf);
                let picker: Element<'a, Message> =
                    pick_list(Timeframe::KLINE, Some(selected_tf), move |new_tf| {
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Heatmap(heatmap::Config {
                                candle_timeframe: Some(new_tf),
                                ..cfg
                            }),
                            false,
                        )
                    })
                    .into();
                Some(column![text("Candle timeframe").size(12), picker].spacing(6).into())
            }
            data::chart::Basis::Tick(_) => None,
        }
    } else {
        None
    };

    let sync_toggle: Option<Element<'a, Message>> = if cfg.show_candles {
        let enabled = cfg.sync_heatmap_to_candles;
        let fallback_tf = default_candle_tf.unwrap_or(Timeframe::M5);
        Some(
            checkbox(enabled)
                .label("Sync heatmap aggregation to candle timeframe")
                .on_toggle(move |v| {
                    let candle_timeframe = if v {
                        cfg.candle_timeframe.or(Some(fallback_tf))
                    } else {
                        cfg.candle_timeframe
                    };
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::Heatmap(heatmap::Config {
                            sync_heatmap_to_candles: v,
                            candle_timeframe,
                            ..cfg
                        }),
                        false,
                    )
                })
                .into(),
        )
    } else {
        None
    };

    let mut overlay_column = column![text("Overlay").size(14), candles_toggle].spacing(8);
    if let Some(picker) = candle_tf_picker {
        overlay_column = overlay_column.push(picker);
    }
    if let Some(toggle) = sync_toggle {
        overlay_column = overlay_column.push(toggle);
    }

    let content = split_column![
        overlay_column,
        size_filters_column,
        noise_filters_column,
        trade_viz_column,
        column![text("Studies").size(14), study_cfg].spacing(8),
        row![
            space::horizontal(),
            sync_all_button(pane, VisualConfig::Heatmap(cfg))
        ]
        ; spacing = 12, align_x = Alignment::Start
    ];

    cfg_view_container(360, content)
}

pub fn timesales_cfg_view<'a>(
    cfg: timeandsales::Config,
    pane: pane_grid::Pane,
) -> Element<'a, Message> {
    let trade_size_column = {
        let filter = cfg.trade_size_filter;
        let slider = labeled_slider(
            "Trade",
            0.0..=50000.0,
            filter,
            move |value| {
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::TimeAndSales(timeandsales::Config {
                        trade_size_filter: value,
                        ..cfg
                    }),
                    false,
                )
            },
            |value| format!(">${}", format_with_commas(*value)),
            Some(500.0),
        );

        column![text("Size filter").size(14), slider].spacing(8)
    };

    let retention_minutes = (cfg.trade_retention.as_secs_f32() / 60.0).max(1.0);
    let retention_slider = {
        let slider_ui = slider(1.0..=60.0, retention_minutes, move |new_minutes| {
            let mins = new_minutes.round().max(1.0) as u64;
            Message::VisualConfigChanged(
                pane,
                VisualConfig::TimeAndSales(timeandsales::Config {
                    trade_retention: Duration::from_secs(mins * 60),
                    ..cfg
                }),
                false,
            )
        })
        .step(1.0);

        classic_slider_row(
            text("Keep trades for"),
            slider_ui.into(),
            Some(text(format!("≈ {} min", retention_minutes.round() as u64)).size(13)),
        )
    };

    let history_column = column![
        row![
            text("History").size(14),
            tooltip(
                button("i").style(style::button::info),
                Some("Affects the stacked bar, colors and how much you can scroll down"),
                TooltipPosition::Top,
            )
        ]
        .spacing(4)
        .align_y(Alignment::Center),
        retention_slider
    ]
    .spacing(8);

    let stacked_bar: Element<_> = {
        let is_shown = cfg.stacked_bar.is_some();

        let enable_checkbox = checkbox(is_shown).label("Show stacked bar").on_toggle({
            move |value| {
                let current_ratio = cfg.stacked_bar.map(|h| h.ratio()).unwrap_or_default();
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::TimeAndSales(timeandsales::Config {
                        stacked_bar: if value {
                            Some(StackedBar::Compact(current_ratio))
                        } else {
                            None
                        },
                        ..cfg
                    }),
                    false,
                )
            }
        });

        let controls: Option<Element<_>> = cfg.stacked_bar.map(|hist| {
            let ratio = hist.ratio();
            let is_compact = matches!(hist, StackedBar::Compact(_));

            let compact = radio("Compact", true, Some(is_compact), {
                move |_v| {
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::TimeAndSales(timeandsales::Config {
                            stacked_bar: Some(StackedBar::Compact(ratio)),
                            ..cfg
                        }),
                        false,
                    )
                }
            })
            .spacing(4);

            let full = radio("Full", false, Some(is_compact), {
                move |_v| {
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::TimeAndSales(timeandsales::Config {
                            stacked_bar: Some(StackedBar::Full(ratio)),
                            ..cfg
                        }),
                        false,
                    )
                }
            })
            .spacing(4);

            let metric_picklist = pick_list(StackedBarRatio::ALL, Some(ratio), move |new_ratio| {
                let new_hist = Some(match cfg.stacked_bar {
                    Some(StackedBar::Full(_)) => StackedBar::Full(new_ratio),
                    _ => StackedBar::Compact(new_ratio),
                });
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::TimeAndSales(timeandsales::Config {
                        stacked_bar: new_hist,
                        ..cfg
                    }),
                    false,
                )
            });

            column![
                iced::widget::rule::horizontal(1),
                text("Mode").size(12),
                row![compact, full].spacing(12),
                text("Metric").size(12),
                metric_picklist,
            ]
            .spacing(8)
            .into()
        });

        let mut inner = column![enable_checkbox]
            .width(Length::Fill)
            .padding(4)
            .spacing(8);

        if let Some(ctrls) = controls {
            inner = inner.push(ctrls);
        }

        container(inner)
            .style(style::modal_container)
            .padding(8)
            .into()
    };

    let content = split_column![
        trade_size_column,
        history_column,
        stacked_bar,
        row![space::horizontal(), sync_all_button(pane, VisualConfig::TimeAndSales(cfg))],
        ; spacing = 12, align_x = Alignment::Start
    ];

    cfg_view_container(320, content)
}

pub fn comparison_cfg_view<'a>(
    pane: pane_grid::Pane,
    chart: &'a ComparisonChart,
) -> Element<'a, Message> {
    let series = &chart.series;
    let series_editor = &chart.series_editor;

    let content = column![series_editor.view(series).map(move |msg| {
        Message::PaneEvent(
            pane,
            Event::ComparisonChartInteraction(crate::chart::comparison::Message::Editor(msg)),
        )
    })];

    cfg_view_container(320, content)
}

pub fn kline_cfg_view<'a>(
    study_config: &'a study::Configurator<FootprintStudy>,
    candle_study_config: &'a study::Configurator<CandleStudy>,
    cfg: data::chart::kline::Config,
    kind: &'a KlineChartKind,
    pane: pane_grid::Pane,
    basis: data::chart::Basis,
    colors_expanded: &'a std::collections::HashSet<SettingsColorsSection>,
) -> Element<'a, Message> {
    use crate::widget::color_picker::color_picker;

    let rgb_to_color = |rgb: [u8; 3]| iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]);

    let content = match kind {
        KlineChartKind::Candles | KlineChartKind::CandlesStudied { .. } => {
            let colors_toggle = |section: SettingsColorsSection| {
                let expanded = colors_expanded.contains(&section);
                let label = if expanded { "Colors ▾" } else { "Colors ▸" };
                button(text(label).size(12))
                    .on_press(Message::PaneEvent(
                        pane,
                        Event::ToggleSettingsColors(section),
                    ))
                    .style(move |theme, status| style::button::transparent(theme, status, expanded))
                    .padding([2, 6])
            };

            let rsi_period = cfg.rsi_period.max(2) as f32;
            let rsi_slider = column![
                text(format!("RSI period: {}", cfg.rsi_period.max(2))),
                slider(2.0..=200.0, rsi_period, move |new_value| {
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::Kline(data::chart::kline::Config {
                            rsi_period: new_value.round().max(2.0) as u16,
                            ..cfg
                        }),
                        false,
                    )
                })
                .step(1.0)
            ]
            .spacing(4);

            let rsi_color_picker: Element<_> = {
                let current_color = rgb_to_color(cfg.rsi_color_rgb);
                let current_hsva = data::config::theme::to_hsva(current_color);
                let picker = color_picker(current_hsva, move |hsva| {
                    let color = data::config::theme::from_hsva(hsva);
                    let [r, g, b, _a] = color.into_rgba8();
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::Kline(data::chart::kline::Config {
                            rsi_color_rgb: [r, g, b],
                            ..cfg
                        }),
                        false,
                    )
                });
                column![text("RSI color"), picker].spacing(6).into()
            };

            let atr_period = cfg.atr_period.max(2) as f32;
            let atr_slider = column![
                text(format!("ATR period: {}", cfg.atr_period.max(2))),
                slider(2.0..=200.0, atr_period, move |new_value| {
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::Kline(data::chart::kline::Config {
                            atr_period: new_value.round().max(2.0) as u16,
                            ..cfg
                        }),
                        false,
                    )
                })
                .step(1.0)
            ]
            .spacing(4);

            let atr_color_picker: Element<_> = {
                let current_color = rgb_to_color(cfg.atr_color_rgb);
                let current_hsva = data::config::theme::to_hsva(current_color);
                let picker = color_picker(current_hsva, move |hsva| {
                    let color = data::config::theme::from_hsva(hsva);
                    let [r, g, b, _a] = color.into_rgba8();
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::Kline(data::chart::kline::Config {
                            atr_color_rgb: [r, g, b],
                            ..cfg
                        }),
                        false,
                    )
                });
                column![text("ATR color"), picker].spacing(6).into()
            };

            let macd_sliders = {
                let fast = cfg.macd_fast.max(2) as f32;
                let slow = cfg.macd_slow.max(3) as f32;
                let signal = cfg.macd_signal.max(2) as f32;

                column![
                    text(format!("MACD fast: {}", cfg.macd_fast.max(2))),
                    slider(2.0..=60.0, fast, move |new_value| {
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                macd_fast: new_value.round().max(2.0) as u16,
                                ..cfg
                            }),
                            false,
                        )
                    })
                    .step(1.0),
                    text(format!("MACD slow: {}", cfg.macd_slow.max(3))),
                    slider(3.0..=120.0, slow, move |new_value| {
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                macd_slow: new_value.round().max(3.0) as u16,
                                ..cfg
                            }),
                            false,
                        )
                    })
                    .step(1.0),
                    text(format!("MACD signal: {}", cfg.macd_signal.max(2))),
                    slider(2.0..=60.0, signal, move |new_value| {
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                macd_signal: new_value.round().max(2.0) as u16,
                                ..cfg
                            }),
                            false,
                        )
                    })
                    .step(1.0),
                ]
                .spacing(4)
            };

            let macd_colors: Element<_> = {
                let macd_cur = rgb_to_color(cfg.macd_color_rgb);
                let sig_cur = rgb_to_color(cfg.macd_signal_color_rgb);
                let macd_hsva = data::config::theme::to_hsva(macd_cur);
                let sig_hsva = data::config::theme::to_hsva(sig_cur);

                let macd_picker = color_picker(macd_hsva, move |hsva| {
                    let color = data::config::theme::from_hsva(hsva);
                    let [r, g, b, _a] = color.into_rgba8();
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::Kline(data::chart::kline::Config {
                            macd_color_rgb: [r, g, b],
                            ..cfg
                        }),
                        false,
                    )
                });

                let signal_picker = color_picker(sig_hsva, move |hsva| {
                    let color = data::config::theme::from_hsva(hsva);
                    let [r, g, b, _a] = color.into_rgba8();
                    Message::VisualConfigChanged(
                        pane,
                        VisualConfig::Kline(data::chart::kline::Config {
                            macd_signal_color_rgb: [r, g, b],
                            ..cfg
                        }),
                        false,
                    )
                });

                column![
                    text("MACD colors"),
                    column![text("MACD line"), macd_picker].spacing(6),
                    column![text("Signal line"), signal_picker].spacing(6),
                ]
                .spacing(8)
                .into()
            };

            let stoch_rsi_cfg: (Element<_>, Element<_>, Element<_>, Element<_>, Element<_>, Element<_>) = {
                let rsi_p = cfg.stoch_rsi_rsi_period.max(2) as f32;
                let stoch_p = cfg.stoch_rsi_period.max(2) as f32;
                let k_sm = cfg.stoch_rsi_k_smooth.max(1) as f32;
                let d_sm = cfg.stoch_rsi_d_smooth.max(1) as f32;

                let rsi_slider = column![
                    text(format!("Stoch RSI - RSI period: {}", cfg.stoch_rsi_rsi_period.max(2))),
                    slider(2.0..=200.0, rsi_p, move |new_value| {
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                stoch_rsi_rsi_period: new_value.round().max(2.0) as u16,
                                ..cfg
                            }),
                            false,
                        )
                    })
                    .step(1.0)
                ]
                .spacing(4);

                let stoch_slider = column![
                    text(format!("Stoch period: {}", cfg.stoch_rsi_period.max(2))),
                    slider(2.0..=200.0, stoch_p, move |new_value| {
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                stoch_rsi_period: new_value.round().max(2.0) as u16,
                                ..cfg
                            }),
                            false,
                        )
                    })
                    .step(1.0)
                ]
                .spacing(4);

                let k_slider = column![
                    text(format!("K smooth: {}", cfg.stoch_rsi_k_smooth.max(1))),
                    slider(1.0..=50.0, k_sm, move |new_value| {
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                stoch_rsi_k_smooth: new_value.round().max(1.0) as u16,
                                ..cfg
                            }),
                            false,
                        )
                    })
                    .step(1.0)
                ]
                .spacing(4);

                let d_slider = column![
                    text(format!("D smooth: {}", cfg.stoch_rsi_d_smooth.max(1))),
                    slider(1.0..=50.0, d_sm, move |new_value| {
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                stoch_rsi_d_smooth: new_value.round().max(1.0) as u16,
                                ..cfg
                            }),
                            false,
                        )
                    })
                    .step(1.0)
                ]
                .spacing(4);

                let k_color: Element<_> = {
                    let hsva = data::config::theme::to_hsva(rgb_to_color(cfg.stoch_rsi_k_color_rgb));
                    let picker = color_picker(hsva, move |hsva| {
                        let color = data::config::theme::from_hsva(hsva);
                        let [r, g, b, _a] = color.into_rgba8();
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                stoch_rsi_k_color_rgb: [r, g, b],
                                ..cfg
                            }),
                            false,
                        )
                    });
                    column![text("K color"), picker].spacing(6).into()
                };

                let d_color: Element<_> = {
                    let hsva = data::config::theme::to_hsva(rgb_to_color(cfg.stoch_rsi_d_color_rgb));
                    let picker = color_picker(hsva, move |hsva| {
                        let color = data::config::theme::from_hsva(hsva);
                        let [r, g, b, _a] = color.into_rgba8();
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                stoch_rsi_d_color_rgb: [r, g, b],
                                ..cfg
                            }),
                            false,
                        )
                    });
                    column![text("D color"), picker].spacing(6).into()
                };

                (
                    rsi_slider.into(),
                    stoch_slider.into(),
                    k_slider.into(),
                    d_slider.into(),
                    k_color,
                    d_color,
                )
            };

            let dmi_cfg: (Element<_>, Element<_>, Element<_>, Element<_>) = {
                let p = cfg.dmi_period.max(2) as f32;
                let period_slider = column![
                    text(format!("DMI period: {}", cfg.dmi_period.max(2))),
                    slider(2.0..=200.0, p, move |new_value| {
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                dmi_period: new_value.round().max(2.0) as u16,
                                ..cfg
                            }),
                            false,
                        )
                    })
                    .step(1.0)
                ]
                .spacing(4);

                let plus_picker: Element<_> = {
                    let hsva = data::config::theme::to_hsva(rgb_to_color(cfg.dmi_plus_color_rgb));
                    let picker = color_picker(hsva, move |hsva| {
                        let color = data::config::theme::from_hsva(hsva);
                        let [r, g, b, _a] = color.into_rgba8();
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                dmi_plus_color_rgb: [r, g, b],
                                ..cfg
                            }),
                            false,
                        )
                    });
                    column![text("+DI color"), picker].spacing(6).into()
                };

                let minus_picker: Element<_> = {
                    let hsva = data::config::theme::to_hsva(rgb_to_color(cfg.dmi_minus_color_rgb));
                    let picker = color_picker(hsva, move |hsva| {
                        let color = data::config::theme::from_hsva(hsva);
                        let [r, g, b, _a] = color.into_rgba8();
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                dmi_minus_color_rgb: [r, g, b],
                                ..cfg
                            }),
                            false,
                        )
                    });
                    column![text("-DI color"), picker].spacing(6).into()
                };

                let adx_picker: Element<_> = {
                    let hsva = data::config::theme::to_hsva(rgb_to_color(cfg.adx_color_rgb));
                    let picker = color_picker(hsva, move |hsva| {
                        let color = data::config::theme::from_hsva(hsva);
                        let [r, g, b, _a] = color.into_rgba8();
                        Message::VisualConfigChanged(
                            pane,
                            VisualConfig::Kline(data::chart::kline::Config {
                                adx_color_rgb: [r, g, b],
                                ..cfg
                            }),
                            false,
                        )
                    });
                    column![text("ADX color"), picker].spacing(6).into()
                };

                (period_slider.into(), plus_picker, minus_picker, adx_picker)
            };

            let candle_studies: &[CandleStudy] = match kind {
                KlineChartKind::CandlesStudied { studies } => studies,
                _ => &[],
            };

            let candle_study_cfg = candle_study_config.view(candle_studies, basis).map(move |msg| {
                Message::PaneEvent(
                    pane,
                    Event::StudyConfigurator(study::StudyMessage::Candle(msg)),
                )
            });

            split_column![
                column![
                    text("Oscillators").size(14),
                    column![
                        {
                        let rsi_card: Element<_> = container(column![
                            text("RSI").size(13),
                            rsi_slider,
                            colors_toggle(SettingsColorsSection::Rsi),
                            if colors_expanded.contains(&SettingsColorsSection::Rsi) {
                                rsi_color_picker
                            } else {
                                column![].into()
                            },
                        ]
                        .spacing(8))
                        .style(style::modal_container)
                        .padding(8)
                        .into();
                        rsi_card
                        },
                        {
                        let macd_card: Element<_> = container(column![
                            text("MACD").size(13),
                            macd_sliders,
                            colors_toggle(SettingsColorsSection::Macd),
                            if colors_expanded.contains(&SettingsColorsSection::Macd) {
                                macd_colors
                            } else {
                                column![].into()
                            },
                        ]
                        .spacing(8))
                        .style(style::modal_container)
                        .padding(8)
                        .into();
                        macd_card
                        },
                        {
                        let stoch_card: Element<_> = container(column![
                            text("Stoch RSI").size(13),
                            stoch_rsi_cfg.0,
                            stoch_rsi_cfg.1,
                            stoch_rsi_cfg.2,
                            stoch_rsi_cfg.3,
                            colors_toggle(SettingsColorsSection::StochRsi),
                            if colors_expanded.contains(&SettingsColorsSection::StochRsi) {
                                Element::from(column![stoch_rsi_cfg.4, stoch_rsi_cfg.5].spacing(10))
                            } else {
                                Element::from(column![])
                            },
                        ]
                        .spacing(8))
                        .style(style::modal_container)
                        .padding(8)
                        .into();
                        stoch_card
                        },
                        {
                        let atr_card: Element<_> = container(column![
                            text("ATR").size(13),
                            atr_slider,
                            colors_toggle(SettingsColorsSection::Atr),
                            if colors_expanded.contains(&SettingsColorsSection::Atr) {
                                atr_color_picker
                            } else {
                                column![].into()
                            },
                        ]
                        .spacing(8))
                        .style(style::modal_container)
                        .padding(8)
                        .into();
                        atr_card
                        },
                        {
                        let dmi_card: Element<_> = container(column![
                            text("DMI / ADX").size(13),
                            dmi_cfg.0,
                            colors_toggle(SettingsColorsSection::DmiAdx),
                            if colors_expanded.contains(&SettingsColorsSection::DmiAdx) {
                                Element::from(column![dmi_cfg.1, dmi_cfg.2, dmi_cfg.3].spacing(10))
                            } else {
                                Element::from(column![])
                            },
                        ]
                        .spacing(8))
                        .style(style::modal_container)
                        .padding(8)
                        .into();
                        dmi_card
                        },
                    ]
                    .spacing(12)
                ]
                .spacing(8),
                column![text("Overlays").size(14), candle_study_cfg].spacing(8),
                row![space::horizontal(), sync_all_button(pane, VisualConfig::Kline(cfg))],
                ; spacing = 12, align_x = Alignment::Start
            ]
        }
        KlineChartKind::Footprint {
            clusters,
            scaling,
            studies,
        } => {
            let cluster_picklist =
                pick_list(ClusterKind::ALL, Some(clusters), move |new_cluster_kind| {
                    Message::PaneEvent(pane, Event::ClusterKindSelected(new_cluster_kind))
                });

            let scaling = {
                let picklist = pick_list(
                    data::chart::kline::ClusterScaling::ALL,
                    Some(scaling),
                    move |new_scaling| {
                        Message::PaneEvent(pane, Event::ClusterScalingSelected(new_scaling))
                    },
                );

                if let data::chart::kline::ClusterScaling::Hybrid { weight } = scaling {
                    let hybrid_slider = slider(0.0..=1.0, *weight, move |new_weight| {
                        Message::PaneEvent(
                            pane,
                            Event::ClusterScalingSelected(
                                data::chart::kline::ClusterScaling::Hybrid { weight: new_weight },
                            ),
                        )
                    })
                    .step(0.05);

                    column![
                        picklist,
                        hybrid_slider,
                        text("Blend visible-range and per-candle scaling"),
                    ]
                    .spacing(8)
                } else {
                    column![picklist].spacing(8)
                }
            };

            let study_cfg = study_config.view(studies, basis).map(move |msg| {
                Message::PaneEvent(
                    pane,
                    Event::StudyConfigurator(study::StudyMessage::Footprint(msg)),
                )
            });

            split_column![
                column![text("Cluster type").size(14), cluster_picklist].spacing(8),
                column![text("Cluster scaling").size(14), scaling].spacing(8),
                column![text("Studies").size(14), study_cfg].spacing(8),
                row![
                    space::horizontal(),
                    sync_all_button(pane, VisualConfig::Kline(cfg))
                ],
                ; spacing = 12, align_x = Alignment::Start
            ]
        }
    };

    cfg_view_container(360, content)
}

pub fn ladder_cfg_view<'a>(cfg: ladder::Config, pane: pane_grid::Pane) -> Element<'a, Message> {
    let display_options = {
        let spread = checkbox(cfg.show_spread)
            .label("Show Spread")
            .on_toggle(move |value| {
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::Ladder(ladder::Config {
                        show_spread: value,
                        ..cfg
                    }),
                    false,
                )
            });

        let chase_tracker = checkbox(cfg.show_chase_tracker)
            .label("Show Chase Tracker")
            .on_toggle(move |value| {
                Message::VisualConfigChanged(
                    pane,
                    VisualConfig::Ladder(ladder::Config {
                        show_chase_tracker: value,
                        ..cfg
                    }),
                    false,
                )
            });

        column![
            text("Display Options").size(14),
            column![
                spread,
                row![
                    chase_tracker,
                    tooltip(
                        button("i").style(style::button::info),
                        Some("Highlights consecutive best-price moves and fades when momentum stalls.\nCalculated using raw ungrouped data."),
                        TooltipPosition::Top,
                    )
                ]
                .align_y(Alignment::Center)
                .spacing(4)
            ]
            .spacing(4)
        ]
        .spacing(8)
    };

    let retention_slider = {
        let retention_minutes = (cfg.trade_retention.as_secs_f32() / 60.0).max(1.0);

        let slider_ui = slider(1.0..=60.0, retention_minutes, move |new_minutes| {
            let mins = new_minutes.round().max(1.0) as u64;
            Message::VisualConfigChanged(
                pane,
                VisualConfig::Ladder(ladder::Config {
                    trade_retention: Duration::from_secs(mins * 60),
                    ..cfg
                }),
                false,
            )
        })
        .step(1.0);

        classic_slider_row(
            text("Keep trades for"),
            slider_ui.into(),
            Some(text(format!("≈ {} min", retention_minutes.round() as u64)).size(13)),
        )
    };

    let history_column = column![text("History").size(14), retention_slider].spacing(8);

    let content = split_column![
        display_options,
        history_column,
        row![
            space::horizontal(),
            sync_all_button(pane, VisualConfig::Ladder(cfg))
        ],
        ; spacing = 12, align_x = Alignment::Start
    ];

    cfg_view_container(320, content)
}

fn sync_all_button<'a>(pane: pane_grid::Pane, config: VisualConfig) -> Element<'a, Message> {
    tooltip(
        button("Sync all").on_press(Message::VisualConfigChanged(pane, config, true)),
        Some("Apply configuration to similar panes"),
        TooltipPosition::Top,
    )
}

pub mod study {
    use super::rgb_to_color;
    use crate::{
        split_column,
        style::{self, Icon, icon_text},
    };
    use data::chart::heatmap::{CLEANUP_THRESHOLD, HeatmapStudy, ProfileKind};
    use data::chart::kline::{CandleStudy, FootprintStudy, MovingAverageKind};
    use iced::{
        Element, padding,
        widget::{button, checkbox, column, container, radio, row, slider, space, text},
    };

    #[derive(Debug, Clone, Copy)]
    pub enum StudyMessage {
        Footprint(Message<FootprintStudy>),
        Heatmap(Message<HeatmapStudy>),
        Candle(Message<CandleStudy>),
    }

    pub trait Study: Sized + Copy + 'static {
        fn is_same_type(&self, other: &Self) -> bool;
        fn all() -> Vec<Self>;
        fn view_config<'a>(
            &self,
            basis: data::chart::Basis,
            on_change: impl Fn(Self) -> Message<Self> + Copy + 'a,
        ) -> Element<'a, Message<Self>>;
    }

    impl Study for FootprintStudy {
        fn is_same_type(&self, other: &Self) -> bool {
            std::mem::discriminant(self) == std::mem::discriminant(other)
        }

        fn all() -> Vec<Self> {
            FootprintStudy::ALL.to_vec()
        }

        fn view_config<'a>(
            &self,
            _basis: data::chart::Basis,
            on_change: impl Fn(Self) -> Message<Self> + Copy + 'a,
        ) -> Element<'a, Message<Self>> {
            match *self {
                FootprintStudy::NPoC { lookback } => {
                    let slider_ui = slider(10.0..=400.0, lookback as f32, move |new_value| {
                        on_change(FootprintStudy::NPoC {
                            lookback: new_value as usize,
                        })
                    })
                    .step(10.0);

                    column![text(format!("Lookback: {lookback} datapoints")), slider_ui]
                        .padding(8)
                        .spacing(4)
                        .into()
                }
                FootprintStudy::Imbalance {
                    threshold,
                    color_scale,
                    ignore_zeros,
                } => {
                    let qty_threshold = {
                        let info_text = text(format!("Ask:Bid threshold: {threshold}%"));

                        let threshold_slider =
                            slider(100.0..=800.0, threshold as f32, move |new_value| {
                                on_change(FootprintStudy::Imbalance {
                                    threshold: new_value as usize,
                                    color_scale,
                                    ignore_zeros,
                                })
                            })
                            .step(25.0);

                        column![info_text, threshold_slider,].padding(8).spacing(4)
                    };

                    let color_scaling = {
                        let color_scale_enabled = color_scale.is_some();
                        let color_scale_value = color_scale.unwrap_or(100);

                        let color_scale_checkbox = checkbox(color_scale_enabled)
                            .label("Dynamic color scaling")
                            .on_toggle(move |is_enabled| {
                                on_change(FootprintStudy::Imbalance {
                                    threshold,
                                    color_scale: if is_enabled {
                                        Some(color_scale_value)
                                    } else {
                                        None
                                    },
                                    ignore_zeros,
                                })
                            });

                        if color_scale_enabled {
                            let scaling_slider = column![
                                text(format!("Opaque color at: {color_scale_value}x")),
                                slider(50.0..=2000.0, color_scale_value as f32, move |new_value| {
                                    on_change(FootprintStudy::Imbalance {
                                        threshold,
                                        color_scale: Some(new_value as usize),
                                        ignore_zeros,
                                    })
                                })
                                .step(50.0)
                            ]
                            .spacing(2);

                            column![color_scale_checkbox, scaling_slider]
                                .padding(8)
                                .spacing(8)
                        } else {
                            column![color_scale_checkbox].padding(8)
                        }
                    };

                    let ignore_zeros_checkbox = {
                        let cbox = checkbox(ignore_zeros).label("Ignore zeros").on_toggle(
                            move |is_checked| {
                                on_change(FootprintStudy::Imbalance {
                                    threshold,
                                    color_scale,
                                    ignore_zeros: is_checked,
                                })
                            },
                        );

                        column![cbox].padding(8).spacing(4)
                    };

                    split_column![qty_threshold, color_scaling, ignore_zeros_checkbox]
                        .padding(4)
                        .into()
                }
            }
        }
    }

    impl Study for CandleStudy {
        fn is_same_type(&self, other: &Self) -> bool {
            // allow two MAs: fast vs slow (they are different variants)
            std::mem::discriminant(self) == std::mem::discriminant(other)
        }

        fn all() -> Vec<Self> {
            vec![
                CandleStudy::MovingAverageFast {
                    kind: MovingAverageKind::SMA,
                    period: 20,
                    source: data::chart::kline::CandleSource::Close,
                    color_rgb: [0x4C, 0xA3, 0xFF],
                },
                CandleStudy::MovingAverageSlow {
                    kind: MovingAverageKind::EMA,
                    period: 50,
                    source: data::chart::kline::CandleSource::Close,
                    color_rgb: [0xFF, 0xC1, 0x4C],
                },
                CandleStudy::BollingerBands {
                    period: 20,
                    source: data::chart::kline::CandleSource::Close,
                    stddev_x100: 200,
                    mid_color_rgb: [0xB0, 0xB0, 0xB0],
                    upper_color_rgb: [0x4C, 0xA3, 0xFF],
                    lower_color_rgb: [0xFF, 0xC1, 0x4C],
                },
                CandleStudy::VwapBands {
                    source: data::chart::kline::CandleSource::Close,
                    reset_daily_utc: true,
                    band_stddev_x100: 200,
                    vwap_color_rgb: [0xB0, 0xB0, 0xB0],
                    upper_color_rgb: [0x4C, 0xA3, 0xFF],
                    lower_color_rgb: [0xFF, 0xC1, 0x4C],
                },
                CandleStudy::Supertrend {
                    atr_period: 10,
                    multiplier_x100: 300,
                    up_color_rgb: [0x4C, 0xA3, 0xFF],
                    down_color_rgb: [0xFF, 0x59, 0x59],
                },
                CandleStudy::EmaRibbon {
                    min_period: 10,
                    max_period: 100,
                    step: 5,
                    start_color_rgb: [0x4C, 0xA3, 0xFF],
                    end_color_rgb: [0xFF, 0xC1, 0x4C],
                },
                CandleStudy::DonchianChannels {
                    period: 20,
                    upper_color_rgb: [0x4C, 0xA3, 0xFF],
                    mid_color_rgb: [0xB0, 0xB0, 0xB0],
                    lower_color_rgb: [0xFF, 0xC1, 0x4C],
                },
                CandleStudy::KeltnerChannels {
                    ema_period: 20,
                    atr_period: 14,
                    multiplier_x100: 150,
                    mid_color_rgb: [0xB0, 0xB0, 0xB0],
                    upper_color_rgb: [0x4C, 0xA3, 0xFF],
                    lower_color_rgb: [0xFF, 0xC1, 0x4C],
                },
                CandleStudy::Ichimoku {
                    tenkan_period: 9,
                    kijun_period: 26,
                    senkou_period: 52,
                    tenkan_color_rgb: [0x4C, 0xA3, 0xFF],
                    kijun_color_rgb: [0xFF, 0xC1, 0x4C],
                    span_a_color_rgb: [0x6E, 0xE7, 0xB7],
                    span_b_color_rgb: [0xFF, 0x9A, 0x9A],
                    lag_color_rgb: [0xB0, 0xB0, 0xB0],
                },
            ]
        }

        fn view_config<'a>(
            &self,
            _basis: data::chart::Basis,
            on_change: impl Fn(Self) -> Message<Self> + Copy + 'a,
        ) -> Element<'a, Message<Self>> {
            match *self {
                CandleStudy::MovingAverageFast {
                    kind,
                    period,
                    source,
                    color_rgb,
                } => ma_config("Fast MA", kind, period, source, color_rgb, on_change),
                CandleStudy::MovingAverageSlow {
                    kind,
                    period,
                    source,
                    color_rgb,
                } => ma_config("Slow MA", kind, period, source, color_rgb, on_change),
                CandleStudy::BollingerBands {
                    period,
                    source,
                    stddev_x100,
                    mid_color_rgb,
                    upper_color_rgb,
                    lower_color_rgb,
                } => bbands_config(
                    period,
                    source,
                    stddev_x100,
                    mid_color_rgb,
                    upper_color_rgb,
                    lower_color_rgb,
                    on_change,
                ),
                CandleStudy::VwapBands {
                    source,
                    reset_daily_utc,
                    band_stddev_x100,
                    vwap_color_rgb,
                    upper_color_rgb,
                    lower_color_rgb,
                } => vwap_config(
                    source,
                    reset_daily_utc,
                    band_stddev_x100,
                    vwap_color_rgb,
                    upper_color_rgb,
                    lower_color_rgb,
                    on_change,
                ),
                CandleStudy::Supertrend {
                    atr_period,
                    multiplier_x100,
                    up_color_rgb,
                    down_color_rgb,
                } => supertrend_config(
                    atr_period,
                    multiplier_x100,
                    up_color_rgb,
                    down_color_rgb,
                    on_change,
                ),
                CandleStudy::EmaRibbon {
                    min_period,
                    max_period,
                    step,
                    start_color_rgb,
                    end_color_rgb,
                } => ema_ribbon_config(
                    min_period,
                    max_period,
                    step,
                    start_color_rgb,
                    end_color_rgb,
                    on_change,
                ),
                CandleStudy::DonchianChannels {
                    period,
                    upper_color_rgb,
                    mid_color_rgb,
                    lower_color_rgb,
                } => donchian_config(
                    period,
                    upper_color_rgb,
                    mid_color_rgb,
                    lower_color_rgb,
                    on_change,
                ),
                CandleStudy::KeltnerChannels {
                    ema_period,
                    atr_period,
                    multiplier_x100,
                    mid_color_rgb,
                    upper_color_rgb,
                    lower_color_rgb,
                } => keltner_config(
                    ema_period,
                    atr_period,
                    multiplier_x100,
                    mid_color_rgb,
                    upper_color_rgb,
                    lower_color_rgb,
                    on_change,
                ),
                CandleStudy::Ichimoku {
                    tenkan_period,
                    kijun_period,
                    senkou_period,
                    tenkan_color_rgb,
                    kijun_color_rgb,
                    span_a_color_rgb,
                    span_b_color_rgb,
                    lag_color_rgb,
                } => ichimoku_config(
                    tenkan_period,
                    kijun_period,
                    senkou_period,
                    tenkan_color_rgb,
                    kijun_color_rgb,
                    span_a_color_rgb,
                    span_b_color_rgb,
                    lag_color_rgb,
                    on_change,
                ),
            }
        }
    }

    fn bbands_config<'a>(
        period: u16,
        source: data::chart::kline::CandleSource,
        stddev_x100: u16,
        mid_color_rgb: [u8; 3],
        upper_color_rgb: [u8; 3],
        lower_color_rgb: [u8; 3],
        on_change: impl Fn(CandleStudy) -> Message<CandleStudy> + Copy + 'a,
    ) -> Element<'a, Message<CandleStudy>> {
        use crate::widget::color_picker::color_picker;

        let rgb_to_color = |rgb: [u8; 3]| iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]);

        let sd = stddev_x100.max(10).min(500) as f32 / 100.0;

        let period_slider = slider(2.0..=400.0, period as f32, move |new_value| {
            let new_period = new_value.round().max(2.0) as u16;
            on_change(CandleStudy::BollingerBands {
                period: new_period,
                source,
                stddev_x100,
                mid_color_rgb,
                upper_color_rgb,
                lower_color_rgb,
            })
        })
        .step(1.0);

        let stddev_slider = slider(0.5..=5.0, sd, move |new_value| {
            let new_sd = (new_value * 100.0).round().max(50.0).min(500.0) as u16;
            on_change(CandleStudy::BollingerBands {
                period,
                source,
                stddev_x100: new_sd,
                mid_color_rgb,
                upper_color_rgb,
                lower_color_rgb,
            })
        })
        .step(0.05);

        let mid_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(mid_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _a] = color.into_rgba8();
                on_change(CandleStudy::BollingerBands {
                    period,
                    source,
                    stddev_x100,
                    mid_color_rgb: [r, g, b],
                    upper_color_rgb,
                    lower_color_rgb,
                })
            })
        };

        let upper_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(upper_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _a] = color.into_rgba8();
                on_change(CandleStudy::BollingerBands {
                    period,
                    source,
                    stddev_x100,
                    mid_color_rgb,
                    upper_color_rgb: [r, g, b],
                    lower_color_rgb,
                })
            })
        };

        let lower_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(lower_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _a] = color.into_rgba8();
                on_change(CandleStudy::BollingerBands {
                    period,
                    source,
                    stddev_x100,
                    mid_color_rgb,
                    upper_color_rgb,
                    lower_color_rgb: [r, g, b],
                })
            })
        };

        column![
            text("Bollinger Bands"),
            text(format!("Period: {period}")),
            period_slider,
            text(format!("Stddev: {sd:.2}")),
            stddev_slider,
            text("Mid color"),
            mid_picker,
            text("Upper color"),
            upper_picker,
            text("Lower color"),
            lower_picker,
        ]
        .padding(8)
        .spacing(6)
        .into()
    }

    fn vwap_config<'a>(
        source: data::chart::kline::CandleSource,
        reset_daily_utc: bool,
        band_stddev_x100: u16,
        vwap_color_rgb: [u8; 3],
        upper_color_rgb: [u8; 3],
        lower_color_rgb: [u8; 3],
        on_change: impl Fn(CandleStudy) -> Message<CandleStudy> + Copy + 'a,
    ) -> Element<'a, Message<CandleStudy>> {
        use crate::widget::color_picker::color_picker;

        let sd = band_stddev_x100.max(10).min(500) as f32 / 100.0;

        let reset_toggle = checkbox(reset_daily_utc)
            .label("Reset daily (UTC)")
            .on_toggle(move |v| {
                on_change(CandleStudy::VwapBands {
                    source,
                    reset_daily_utc: v,
                    band_stddev_x100,
                    vwap_color_rgb,
                    upper_color_rgb,
                    lower_color_rgb,
                })
            });

        let stddev_slider = slider(0.5..=5.0, sd, move |new_value| {
            let new_sd = (new_value * 100.0).round().max(50.0).min(500.0) as u16;
            on_change(CandleStudy::VwapBands {
                source,
                reset_daily_utc,
                band_stddev_x100: new_sd,
                vwap_color_rgb,
                upper_color_rgb,
                lower_color_rgb,
            })
        })
        .step(0.05);

        let vwap_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(vwap_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _a] = color.into_rgba8();
                on_change(CandleStudy::VwapBands {
                    source,
                    reset_daily_utc,
                    band_stddev_x100,
                    vwap_color_rgb: [r, g, b],
                    upper_color_rgb,
                    lower_color_rgb,
                })
            })
        };

        let upper_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(upper_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _a] = color.into_rgba8();
                on_change(CandleStudy::VwapBands {
                    source,
                    reset_daily_utc,
                    band_stddev_x100,
                    vwap_color_rgb,
                    upper_color_rgb: [r, g, b],
                    lower_color_rgb,
                })
            })
        };

        let lower_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(lower_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _a] = color.into_rgba8();
                on_change(CandleStudy::VwapBands {
                    source,
                    reset_daily_utc,
                    band_stddev_x100,
                    vwap_color_rgb,
                    upper_color_rgb,
                    lower_color_rgb: [r, g, b],
                })
            })
        };

        column![
            text("VWAP"),
            reset_toggle,
            text(format!("Bands stddev: {sd:.2}")),
            stddev_slider,
            text("VWAP color"),
            vwap_picker,
            text("Upper band color"),
            upper_picker,
            text("Lower band color"),
            lower_picker,
        ]
        .padding(8)
        .spacing(6)
        .into()
    }

    fn supertrend_config<'a>(
        atr_period: u16,
        multiplier_x100: u16,
        up_color_rgb: [u8; 3],
        down_color_rgb: [u8; 3],
        on_change: impl Fn(CandleStudy) -> Message<CandleStudy> + Copy + 'a,
    ) -> Element<'a, Message<CandleStudy>> {
        use crate::widget::color_picker::color_picker;
        let rgb_to_color = |rgb: [u8; 3]| iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]);

        let atr_slider = slider(2.0..=200.0, atr_period.max(2) as f32, move |new_value| {
            on_change(CandleStudy::Supertrend {
                atr_period: new_value.round().max(2.0) as u16,
                multiplier_x100,
                up_color_rgb,
                down_color_rgb,
            })
        })
        .step(1.0);

        let mult = multiplier_x100.max(50).min(1000) as f32 / 100.0;
        let mult_slider = slider(0.5..=10.0, mult, move |new_value| {
            let m = (new_value * 100.0).round().max(50.0).min(1000.0) as u16;
            on_change(CandleStudy::Supertrend {
                atr_period,
                multiplier_x100: m,
                up_color_rgb,
                down_color_rgb,
            })
        })
        .step(0.05);

        let up_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(up_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _a] = color.into_rgba8();
                on_change(CandleStudy::Supertrend {
                    atr_period,
                    multiplier_x100,
                    up_color_rgb: [r, g, b],
                    down_color_rgb,
                })
            })
        };

        let down_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(down_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _a] = color.into_rgba8();
                on_change(CandleStudy::Supertrend {
                    atr_period,
                    multiplier_x100,
                    up_color_rgb,
                    down_color_rgb: [r, g, b],
                })
            })
        };

        column![
            text("Supertrend"),
            text(format!("ATR period: {atr_period}")),
            atr_slider,
            text(format!("Multiplier: {mult:.2}")),
            mult_slider,
            text("Up color"),
            up_picker,
            text("Down color"),
            down_picker,
        ]
        .padding(8)
        .spacing(6)
        .into()
    }

    fn ema_ribbon_config<'a>(
        min_period: u16,
        max_period: u16,
        step: u16,
        start_color_rgb: [u8; 3],
        end_color_rgb: [u8; 3],
        on_change: impl Fn(CandleStudy) -> Message<CandleStudy> + Copy + 'a,
    ) -> Element<'a, Message<CandleStudy>> {
        use crate::widget::color_picker::color_picker;
        let rgb_to_color = |rgb: [u8; 3]| iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]);

        let min_p = min_period.max(2) as f32;
        let max_p = max_period.max(min_period.max(2)) as f32;
        let step_v = step.max(1) as f32;

        let min_slider = slider(2.0..=400.0, min_p, move |new_value| {
            let v = new_value.round().max(2.0) as u16;
            on_change(CandleStudy::EmaRibbon {
                min_period: v,
                max_period: max_period.max(v),
                step,
                start_color_rgb,
                end_color_rgb,
            })
        })
        .step(1.0);

        let max_slider = slider(2.0..=400.0, max_p, move |new_value| {
            let v = new_value.round().max(2.0) as u16;
            on_change(CandleStudy::EmaRibbon {
                min_period,
                max_period: v.max(min_period),
                step,
                start_color_rgb,
                end_color_rgb,
            })
        })
        .step(1.0);

        let step_slider = slider(1.0..=50.0, step_v, move |new_value| {
            let v = new_value.round().max(1.0) as u16;
            on_change(CandleStudy::EmaRibbon {
                min_period,
                max_period,
                step: v,
                start_color_rgb,
                end_color_rgb,
            })
        })
        .step(1.0);

        let start_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(start_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _a] = color.into_rgba8();
                on_change(CandleStudy::EmaRibbon {
                    min_period,
                    max_period,
                    step,
                    start_color_rgb: [r, g, b],
                    end_color_rgb,
                })
            })
        };

        let end_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(end_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _a] = color.into_rgba8();
                on_change(CandleStudy::EmaRibbon {
                    min_period,
                    max_period,
                    step,
                    start_color_rgb,
                    end_color_rgb: [r, g, b],
                })
            })
        };

        column![
            text("EMA Ribbon"),
            text(format!("Min period: {min_period}")),
            min_slider,
            text(format!("Max period: {max_period}")),
            max_slider,
            text(format!("Step: {step}")),
            step_slider,
            text("Start color"),
            start_picker,
            text("End color"),
            end_picker,
        ]
        .padding(8)
        .spacing(6)
        .into()
    }

    fn donchian_config<'a>(
        period: u16,
        upper_color_rgb: [u8; 3],
        mid_color_rgb: [u8; 3],
        lower_color_rgb: [u8; 3],
        on_change: impl Fn(CandleStudy) -> Message<CandleStudy> + Copy + 'a,
    ) -> Element<'a, Message<CandleStudy>> {
        use crate::widget::color_picker::color_picker;
        let rgb_to_color = |rgb: [u8; 3]| iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]);

        let period_slider = slider(2.0..=400.0, period.max(2) as f32, move |new_value| {
            let p = new_value.round().max(2.0) as u16;
            on_change(CandleStudy::DonchianChannels {
                period: p,
                upper_color_rgb,
                mid_color_rgb,
                lower_color_rgb,
            })
        })
        .step(1.0);

        let mid_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(mid_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _] = color.into_rgba8();
                on_change(CandleStudy::DonchianChannels {
                    period,
                    upper_color_rgb,
                    mid_color_rgb: [r, g, b],
                    lower_color_rgb,
                })
            })
        };
        let upper_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(upper_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _] = color.into_rgba8();
                on_change(CandleStudy::DonchianChannels {
                    period,
                    upper_color_rgb: [r, g, b],
                    mid_color_rgb,
                    lower_color_rgb,
                })
            })
        };
        let lower_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(lower_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _] = color.into_rgba8();
                on_change(CandleStudy::DonchianChannels {
                    period,
                    upper_color_rgb,
                    mid_color_rgb,
                    lower_color_rgb: [r, g, b],
                })
            })
        };

        column![
            text("Donchian Channels"),
            text(format!("Period: {period}")),
            period_slider,
            text("Upper color"),
            upper_picker,
            text("Mid color"),
            mid_picker,
            text("Lower color"),
            lower_picker,
        ]
        .padding(8)
        .spacing(6)
        .into()
    }

    fn keltner_config<'a>(
        ema_period: u16,
        atr_period: u16,
        multiplier_x100: u16,
        mid_color_rgb: [u8; 3],
        upper_color_rgb: [u8; 3],
        lower_color_rgb: [u8; 3],
        on_change: impl Fn(CandleStudy) -> Message<CandleStudy> + Copy + 'a,
    ) -> Element<'a, Message<CandleStudy>> {
        use crate::widget::color_picker::color_picker;
        let rgb_to_color = |rgb: [u8; 3]| iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]);

        let ema_slider = slider(2.0..=200.0, ema_period.max(2) as f32, move |new_value| {
            let p = new_value.round().max(2.0) as u16;
            on_change(CandleStudy::KeltnerChannels {
                ema_period: p,
                atr_period,
                multiplier_x100,
                mid_color_rgb,
                upper_color_rgb,
                lower_color_rgb,
            })
        })
        .step(1.0);

        let atr_slider = slider(2.0..=200.0, atr_period.max(2) as f32, move |new_value| {
            let p = new_value.round().max(2.0) as u16;
            on_change(CandleStudy::KeltnerChannels {
                ema_period,
                atr_period: p,
                multiplier_x100,
                mid_color_rgb,
                upper_color_rgb,
                lower_color_rgb,
            })
        })
        .step(1.0);

        let mult = multiplier_x100.max(50).min(1000) as f32 / 100.0;
        let mult_slider = slider(0.5..=10.0, mult, move |new_value| {
            let m = (new_value * 100.0).round().max(50.0).min(1000.0) as u16;
            on_change(CandleStudy::KeltnerChannels {
                ema_period,
                atr_period,
                multiplier_x100: m,
                mid_color_rgb,
                upper_color_rgb,
                lower_color_rgb,
            })
        })
        .step(0.05);

        let mid_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(mid_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _] = color.into_rgba8();
                on_change(CandleStudy::KeltnerChannels {
                    ema_period,
                    atr_period,
                    multiplier_x100,
                    mid_color_rgb: [r, g, b],
                    upper_color_rgb,
                    lower_color_rgb,
                })
            })
        };
        let upper_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(upper_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _] = color.into_rgba8();
                on_change(CandleStudy::KeltnerChannels {
                    ema_period,
                    atr_period,
                    multiplier_x100,
                    mid_color_rgb,
                    upper_color_rgb: [r, g, b],
                    lower_color_rgb,
                })
            })
        };
        let lower_picker = {
            let hsva = data::config::theme::to_hsva(rgb_to_color(lower_color_rgb));
            color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _] = color.into_rgba8();
                on_change(CandleStudy::KeltnerChannels {
                    ema_period,
                    atr_period,
                    multiplier_x100,
                    mid_color_rgb,
                    upper_color_rgb,
                    lower_color_rgb: [r, g, b],
                })
            })
        };

        column![
            text("Keltner Channels"),
            text(format!("EMA period: {ema_period}")),
            ema_slider,
            text(format!("ATR period: {atr_period}")),
            atr_slider,
            text(format!("Multiplier: {mult:.2}")),
            mult_slider,
            text("Mid color"),
            mid_picker,
            text("Upper color"),
            upper_picker,
            text("Lower color"),
            lower_picker,
        ]
        .padding(8)
        .spacing(6)
        .into()
    }

    fn ichimoku_config<'a>(
        tenkan_period: u16,
        kijun_period: u16,
        senkou_period: u16,
        tenkan_color_rgb: [u8; 3],
        kijun_color_rgb: [u8; 3],
        span_a_color_rgb: [u8; 3],
        span_b_color_rgb: [u8; 3],
        lag_color_rgb: [u8; 3],
        on_change: impl Fn(CandleStudy) -> Message<CandleStudy> + Copy + 'a,
    ) -> Element<'a, Message<CandleStudy>> {
        use crate::widget::color_picker::color_picker;

        let tenkan_slider = slider(2.0..=200.0, tenkan_period.max(2) as f32, move |v| {
            on_change(CandleStudy::Ichimoku {
                tenkan_period: v.round().max(2.0) as u16,
                kijun_period,
                senkou_period,
                tenkan_color_rgb,
                kijun_color_rgb,
                span_a_color_rgb,
                span_b_color_rgb,
                lag_color_rgb,
            })
        })
        .step(1.0);

        let kijun_slider = slider(2.0..=300.0, kijun_period.max(2) as f32, move |v| {
            on_change(CandleStudy::Ichimoku {
                tenkan_period,
                kijun_period: v.round().max(2.0) as u16,
                senkou_period,
                tenkan_color_rgb,
                kijun_color_rgb,
                span_a_color_rgb,
                span_b_color_rgb,
                lag_color_rgb,
            })
        })
        .step(1.0);

        let senkou_slider = slider(2.0..=500.0, senkou_period.max(2) as f32, move |v| {
            on_change(CandleStudy::Ichimoku {
                tenkan_period,
                kijun_period,
                senkou_period: v.round().max(2.0) as u16,
                tenkan_color_rgb,
                kijun_color_rgb,
                span_a_color_rgb,
                span_b_color_rgb,
                lag_color_rgb,
            })
        })
        .step(1.0);

        fn pick_color<'a, M, F>(
            label: &'static str,
            rgb: [u8; 3],
            on_change: M,
            f: F,
        ) -> Element<'a, Message<CandleStudy>>
        where
            M: Fn(CandleStudy) -> Message<CandleStudy> + Copy + 'a,
            F: Fn([u8; 3]) -> CandleStudy + Copy + 'a,
        {
            let hsva = data::config::theme::to_hsva(iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]));
            column![text(label), color_picker(hsva, move |hsva| {
                let color = data::config::theme::from_hsva(hsva);
                let [r, g, b, _] = color.into_rgba8();
                on_change(f([r, g, b]))
            })]
            .spacing(6)
            .into()
        }

        let tenkan_pick = pick_color("Tenkan color", tenkan_color_rgb, on_change, move |c| {
            CandleStudy::Ichimoku {
            tenkan_period,
            kijun_period,
            senkou_period,
            tenkan_color_rgb: c,
            kijun_color_rgb,
            span_a_color_rgb,
            span_b_color_rgb,
            lag_color_rgb,
            }
        });
        let kijun_pick = pick_color("Kijun color", kijun_color_rgb, on_change, move |c| {
            CandleStudy::Ichimoku {
            tenkan_period,
            kijun_period,
            senkou_period,
            tenkan_color_rgb,
            kijun_color_rgb: c,
            span_a_color_rgb,
            span_b_color_rgb,
            lag_color_rgb,
            }
        });
        let span_a_pick = pick_color("Span A color", span_a_color_rgb, on_change, move |c| {
            CandleStudy::Ichimoku {
            tenkan_period,
            kijun_period,
            senkou_period,
            tenkan_color_rgb,
            kijun_color_rgb,
            span_a_color_rgb: c,
            span_b_color_rgb,
            lag_color_rgb,
            }
        });
        let span_b_pick = pick_color("Span B color", span_b_color_rgb, on_change, move |c| {
            CandleStudy::Ichimoku {
            tenkan_period,
            kijun_period,
            senkou_period,
            tenkan_color_rgb,
            kijun_color_rgb,
            span_a_color_rgb,
            span_b_color_rgb: c,
            lag_color_rgb,
            }
        });
        let lag_pick = pick_color("Lagging color", lag_color_rgb, on_change, move |c| {
            CandleStudy::Ichimoku {
            tenkan_period,
            kijun_period,
            senkou_period,
            tenkan_color_rgb,
            kijun_color_rgb,
            span_a_color_rgb,
            span_b_color_rgb,
            lag_color_rgb: c,
            }
        });

        column![
            text("Ichimoku"),
            text(format!("Tenkan: {tenkan_period}")),
            tenkan_slider,
            text(format!("Kijun: {kijun_period}")),
            kijun_slider,
            text(format!("Senkou: {senkou_period}")),
            senkou_slider,
            tenkan_pick,
            kijun_pick,
            span_a_pick,
            span_b_pick,
            lag_pick,
        ]
        .padding(8)
        .spacing(8)
        .into()
    }

    fn ma_config<'a>(
        title: &'static str,
        kind: MovingAverageKind,
        period: u16,
        source: data::chart::kline::CandleSource,
        color_rgb: [u8; 3],
        on_change: impl Fn(CandleStudy) -> Message<CandleStudy> + Copy + 'a,
    ) -> Element<'a, Message<CandleStudy>> {
        use crate::widget::color_picker::color_picker;

        let rgb_to_color = |rgb: [u8; 3]| iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]);

        let current_color = rgb_to_color(color_rgb);
        let current_hsva = data::config::theme::to_hsva(current_color);

        let sma = radio("SMA", MovingAverageKind::SMA, Some(kind), move |new_kind| {
            on_change(match title {
                "Fast MA" => CandleStudy::MovingAverageFast {
                    kind: new_kind,
                    period,
                    source,
                    color_rgb,
                },
                _ => CandleStudy::MovingAverageSlow {
                    kind: new_kind,
                    period,
                    source,
                    color_rgb,
                },
            })
        })
        .spacing(6);

        let ema = radio("EMA", MovingAverageKind::EMA, Some(kind), move |new_kind| {
            on_change(match title {
                "Fast MA" => CandleStudy::MovingAverageFast {
                    kind: new_kind,
                    period,
                    source,
                    color_rgb,
                },
                _ => CandleStudy::MovingAverageSlow {
                    kind: new_kind,
                    period,
                    source,
                    color_rgb,
                },
            })
        })
        .spacing(6);

        let period_slider = slider(2.0..=400.0, period as f32, move |new_value| {
            let new_period = new_value.round().max(2.0) as u16;
            on_change(match title {
                "Fast MA" => CandleStudy::MovingAverageFast {
                    kind,
                    period: new_period,
                    source,
                    color_rgb,
                },
                _ => CandleStudy::MovingAverageSlow {
                    kind,
                    period: new_period,
                    source,
                    color_rgb,
                },
            })
        })
        .step(1.0);

        let picker = color_picker(current_hsva, move |hsva| {
            let color = data::config::theme::from_hsva(hsva);
            let [r, g, b, _a] = color.into_rgba8();
            let new_rgb = [r, g, b];
            on_change(match title {
                "Fast MA" => CandleStudy::MovingAverageFast {
                    kind,
                    period,
                    source,
                    color_rgb: new_rgb,
                },
                _ => CandleStudy::MovingAverageSlow {
                    kind,
                    period,
                    source,
                    color_rgb: new_rgb,
                },
            })
        });

        column![
            text(title),
            row![sma, ema].spacing(16),
            text("Color"),
            picker,
            text(format!("Period: {period}")),
            period_slider,
        ]
        .padding(8)
        .spacing(6)
        .into()
    }

    impl Study for HeatmapStudy {
        fn is_same_type(&self, other: &Self) -> bool {
            std::mem::discriminant(self) == std::mem::discriminant(other)
        }

        fn all() -> Vec<Self> {
            HeatmapStudy::ALL.to_vec()
        }

        fn view_config<'a>(
            &self,
            basis: data::chart::Basis,
            on_change: impl Fn(Self) -> Message<Self> + Copy + 'a,
        ) -> Element<'a, Message<Self>> {
            let interval_ms = match basis {
                data::chart::Basis::Time(interval) => interval.to_milliseconds(),
                data::chart::Basis::Tick(_) => {
                    return iced::widget::center(text(
                        "Heatmap studies are not supported for tick-based charts",
                    ))
                    .into();
                }
            };

            match self {
                HeatmapStudy::VolumeProfile(kind) => match kind {
                    ProfileKind::FixedWindow(datapoint_count) => {
                        let duration_secs = (*datapoint_count as u64 * interval_ms) / 1000;
                        let min_range = CLEANUP_THRESHOLD / 20;

                        let duration_text = if duration_secs < 60 {
                            format!("{} seconds", duration_secs)
                        } else {
                            let minutes = duration_secs / 60;
                            let seconds = duration_secs % 60;
                            if seconds == 0 {
                                format!("{} minutes", minutes)
                            } else {
                                format!("{}m {}s", minutes, seconds)
                            }
                        };

                        let slider = slider(
                            min_range as f32..=CLEANUP_THRESHOLD as f32,
                            *datapoint_count as f32,
                            move |new_datapoint_count| {
                                on_change(HeatmapStudy::VolumeProfile(ProfileKind::FixedWindow(
                                    new_datapoint_count as usize,
                                )))
                            },
                        )
                        .step(40.0);

                        let switch_kind = button(text("Switch to visible range")).on_press(
                            on_change(HeatmapStudy::VolumeProfile(ProfileKind::VisibleRange)),
                        );

                        column![
                            row![space::horizontal(), switch_kind,],
                            text(format!(
                                "Window: {} datapoints ({})",
                                datapoint_count, duration_text
                            )),
                            slider,
                        ]
                        .padding(8)
                        .spacing(4)
                        .into()
                    }
                    ProfileKind::VisibleRange => {
                        let switch_kind = button(text("Switch to fixed window")).on_press(
                            on_change(HeatmapStudy::VolumeProfile(ProfileKind::FixedWindow(
                                CLEANUP_THRESHOLD / 5_usize,
                            ))),
                        );

                        column![row![space::horizontal(), switch_kind,],]
                            .padding(8)
                            .spacing(4)
                            .into()
                    }
                },
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub enum Message<S: Study> {
        CardToggled(S),
        StudyToggled(S, bool),
        StudyValueChanged(S),
    }

    pub enum Action<S: Study> {
        ToggleStudy(S, bool),
        ConfigureStudy(S),
    }

    pub struct Configurator<S: Study> {
        expanded_card: Option<S>,
    }

    impl<S: Study> Default for Configurator<S> {
        fn default() -> Self {
            Self {
                expanded_card: None,
            }
        }
    }

    impl<S: Study + ToString> Configurator<S> {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn update(&mut self, message: Message<S>) -> Option<Action<S>> {
            match message {
                Message::CardToggled(study) => {
                    let should_collapse = self
                        .expanded_card
                        .as_ref()
                        .is_some_and(|expanded| expanded.is_same_type(&study));

                    if should_collapse {
                        self.expanded_card = None;
                    } else {
                        self.expanded_card = Some(study);
                    }
                }
                Message::StudyToggled(study, is_checked) => {
                    return Some(Action::ToggleStudy(study, is_checked));
                }
                Message::StudyValueChanged(study) => {
                    return Some(Action::ConfigureStudy(study));
                }
            }

            None
        }

        pub fn view<'a>(
            &self,
            active_studies: &'a [S],
            basis: data::chart::Basis,
        ) -> Element<'a, Message<S>> {
            let mut content = column![].spacing(4);

            for available_study in S::all() {
                content =
                    content.push(self.create_study_row(available_study, active_studies, basis));
            }

            content.into()
        }

        fn create_study_row<'a>(
            &self,
            study: S,
            active_studies: &'a [S],
            basis: data::chart::Basis,
        ) -> Element<'a, Message<S>> {
            let (is_selected, study_config) = {
                let mut is_selected = false;
                let mut study_config = None;

                for s in active_studies {
                    if s.is_same_type(&study) {
                        is_selected = true;
                        study_config = Some(*s);
                        break;
                    }
                }
                (is_selected, study_config)
            };

            let checkbox = checkbox(is_selected)
                .label(study_config.map_or(study.to_string(), |s| s.to_string()))
                .on_toggle(move |checked| Message::StudyToggled(study, checked));

            let mut checkbox_row = row![checkbox, space::horizontal()]
                .height(36)
                .align_y(iced::Alignment::Center)
                .padding(padding::left(8).right(4))
                .spacing(4);

            let is_expanded = self
                .expanded_card
                .as_ref()
                .is_some_and(|expanded| expanded.is_same_type(&study));

            if is_selected {
                checkbox_row = checkbox_row.push(
                    button(icon_text(Icon::Cog, 12))
                        .on_press(Message::CardToggled(study))
                        .style(move |theme, status| {
                            style::button::transparent(theme, status, is_expanded)
                        }),
                );
            }

            let mut column = column![checkbox_row];

            if is_expanded && let Some(config) = study_config {
                column = column.push(config.view_config(basis, Message::StudyValueChanged));
            }

            container(column).style(style::modal_container).into()
        }
    }
}
