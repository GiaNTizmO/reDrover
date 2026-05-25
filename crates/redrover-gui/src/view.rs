//! Iced view tree for the reDrover installer.
//! Settings are grouped by function and annotated with compact hover help.

use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, tooltip,
    Space,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use redrover_config::UdpStrategy;

use crate::app::{App, LogLevelChoice, Message, ProxyKindChoice, Status};

const ACCENT: Color = Color::from_rgb(0.40, 0.55, 0.95);
const OK_COLOR: Color = Color::from_rgb(0.30, 0.78, 0.42);
const ERR_COLOR: Color = Color::from_rgb(0.93, 0.36, 0.36);
const MUTED: Color = Color::from_rgb(0.62, 0.65, 0.72);

const LABEL_W: f32 = 122.0;
const FIELD_H: f32 = 30.0;

pub fn view(app: &App) -> Element<'_, Message> {
    let content = column![
        title_bar(),
        proxy_card(app),
        voice_card(app),
        logging_card(app),
        actions(app),
    ]
    .spacing(10);

    container(scrollable(content).spacing(8))
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn title_bar<'a>() -> Element<'a, Message> {
    row![
        text("reDrover").size(21),
        Space::with_width(Length::Fill),
        text(concat!("v", env!("CARGO_PKG_VERSION")))
            .size(11)
            .color(MUTED),
    ]
    .align_y(Alignment::Center)
    .into()
}

fn proxy_card(app: &App) -> Element<'_, Message> {
    let mut col = column![section_header("Proxy"), proxy_mode_picker(app)].spacing(8);
    let direct = matches!(app.kind, ProxyKindChoice::Direct);

    if direct {
        col = col.push(note(
            "Direct mode keeps voice strategies and logging available without routing TCP traffic.",
        ));
    } else {
        col = col
            .push(labeled_help(
                "Host",
                "Proxy hostname or IP address used by Discord TCP connections.",
                text_field(
                    &app.host,
                    "proxy.example.com",
                    Message::HostChanged,
                    false,
                    Length::Fill,
                    true,
                ),
            ))
            .push(labeled_help(
                "Port",
                "Listening TCP port of the selected HTTP or SOCKS5 proxy.",
                text_field(
                    &app.port,
                    "1080",
                    Message::PortChanged,
                    false,
                    Length::Fixed(100.0),
                    true,
                ),
            ))
            .push(setting_checkbox(
                "Authentication",
                "Add proxy credentials. HTTP uses Basic auth; SOCKS5 credentials are used for UDP ASSOCIATE.",
                app.auth,
                Some(Message::AuthToggled),
            ));

        if app.auth {
            col = col
                .push(labeled_help(
                    "Login",
                    "Username sent to the proxy when authentication is enabled.",
                    text_field(
                        &app.login,
                        "user",
                        Message::LoginChanged,
                        false,
                        Length::Fill,
                        true,
                    ),
                ))
                .push(labeled_help(
                    "Password",
                    "Password sent to the proxy; it is stored in drover.ini.",
                    text_field(
                        &app.password,
                        "password",
                        Message::PasswordChanged,
                        true,
                        Length::Fill,
                        true,
                    ),
                ));
        }
    }

    card(col.into())
}

fn proxy_mode_picker(app: &App) -> Element<'_, Message> {
    let mut choices = row![].spacing(6);
    for &kind in ProxyKindChoice::ALL {
        let active = kind == app.kind;
        let btn = button(text(kind.label()).size(13).center())
            .on_press(Message::ProxyKindSelected(kind))
            .padding(Padding {
                top: 5.0,
                bottom: 5.0,
                left: 12.0,
                right: 12.0,
            })
            .style(if active {
                button::primary
            } else {
                button::secondary
            });
        choices = choices.push(btn);
    }

    labeled_help(
        "Mode",
        "HTTP and SOCKS5 route Discord TCP through a proxy. Direct leaves TCP untouched.",
        choices.into(),
    )
}

fn voice_card(app: &App) -> Element<'_, Message> {
    let socks5_mode = matches!(app.kind, ProxyKindChoice::Socks5);
    let advanced_strategy = matches!(app.udp_strategy, UdpStrategy::Split | UdpStrategy::Custom);

    let strategy_pick = pick_list(
        UdpStrategy::GUI_CHOICES,
        Some(app.udp_strategy),
        Message::UdpStrategySelected,
    )
    .text_size(13)
    .padding(Padding {
        top: 4.0,
        bottom: 4.0,
        left: 8.0,
        right: 8.0,
    });

    let mut col = column![
        section_header("Voice"),
        labeled_help(
            "Strategy",
            "How the first voice UDP packet is altered. Empty curated presets are intentionally not offered.",
            strategy_pick.into(),
        ),
    ]
    .spacing(8);

    if advanced_strategy {
        col = col
            .push(labeled_help(
                "Prefix",
                "Hex packets sent before the first Discord voice packet, separated by commas.",
                text_field(
                    &app.udp_prefix_packets,
                    "00, 01",
                    Message::UdpPrefixPacketsChanged,
                    false,
                    Length::Fill,
                    true,
                ),
            ))
            .push(labeled_help(
                "Delay",
                "Pause after prefix packets before the original or split payload is sent.",
                row![
                    text_field(
                        &app.udp_delay_ms,
                        "50",
                        Message::UdpDelayChanged,
                        false,
                        Length::Fixed(80.0),
                        true,
                    ),
                    text("ms").size(12).color(MUTED),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into(),
            ));
    }

    if advanced_strategy {
        col = col.push(labeled_help(
            "Parts",
            "Number of pieces for the first packet. Split requires 2 to 74; use 0 in Custom to disable splitting.",
            text_field(
                &app.udp_split_first,
                "2",
                Message::UdpSplitFirstChanged,
                false,
                Length::Fixed(80.0),
                true,
            ),
        ));
    }

    col = col
        .push(setting_checkbox(
            "Force TCP fallback",
            "Drop the first UDP attempt so Discord can fall back to TCP voice. Works in any mode; proxy modes can then route that TCP.",
            app.udp_force_tcp_fallback,
            Some(Message::UdpForceTcpToggled),
        ))
        .push(setting_checkbox(
            "SOCKS5 UDP associate",
            "Relay voice UDP using SOCKS5 UDP ASSOCIATE. Available only when Proxy mode is SOCKS5.",
            app.socks5_udp_associate && socks5_mode,
            if socks5_mode {
                Some(Message::Socks5UdpAssociateToggled)
            } else {
                None
            },
        ));

    if !socks5_mode {
        col = col.push(note(
            "SOCKS5 UDP associate becomes available after selecting SOCKS5 proxy mode.",
        ));
    }

    card(col.into())
}

fn logging_card(app: &App) -> Element<'_, Message> {
    let level_pick = pick_list(
        LogLevelChoice::ALL,
        Some(app.log_level),
        Message::LogLevelSelected,
    )
    .text_size(13)
    .padding(Padding {
        top: 4.0,
        bottom: 4.0,
        left: 8.0,
        right: 8.0,
    });

    let col = column![
        section_header("Logging"),
        labeled_help(
            "Level",
            "Minimum severity written to enabled log destinations. Debug is useful for troubleshooting.",
            level_pick.into(),
        ),
        setting_checkbox(
            "Write log file",
            "Write log messages next to Discord.exe. The filename below is used only when this is enabled.",
            app.log_file_enabled,
            Some(Message::LogFileToggled),
        ),
        labeled_help(
            "File",
            "Relative log filename stored next to Discord.exe.",
            text_field(
                &app.log_file,
                "drover.log",
                Message::LogFileChanged,
                false,
                Length::Fill,
                app.log_file_enabled,
            ),
        ),
        setting_checkbox(
            "Debug console",
            "Open a live colored console window on Windows and mirror log output there.",
            app.log_console,
            Some(Message::LogConsoleToggled),
        ),
    ]
    .spacing(8);

    card(col.into())
}

fn actions(app: &App) -> Element<'_, Message> {
    let install_btn = button(text("Install").size(14).center())
        .on_press(Message::InstallPressed)
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 18.0,
            right: 18.0,
        })
        .style(button::primary)
        .width(Length::Fill);

    let uninstall_btn = button(text("Uninstall").size(14).center())
        .on_press(Message::UninstallPressed)
        .padding(Padding {
            top: 8.0,
            bottom: 8.0,
            left: 18.0,
            right: 18.0,
        })
        .style(button::danger)
        .width(Length::Fill);

    column![row![install_btn, uninstall_btn].spacing(8), status_row(app),]
        .spacing(6)
        .into()
}

fn status_row(app: &App) -> Element<'_, Message> {
    match &app.status {
        Status::Idle => text("Ready.").size(12).color(MUTED).into(),
        Status::Working(s) => text(s.clone()).size(12).color(ACCENT).into(),
        Status::Ok(s) => text(s.clone()).size(12).color(OK_COLOR).into(),
        Status::Err(s) => text(s.clone()).size(12).color(ERR_COLOR).into(),
    }
}

fn section_header<'a>(title: &'a str) -> Element<'a, Message> {
    text(title).size(11).color(ACCENT).into()
}

fn labeled_help<'a>(
    label: &'a str,
    help: &'a str,
    control: Element<'a, Message>,
) -> Element<'a, Message> {
    row![
        row![text(label).size(13).color(MUTED), help_hint(help)]
            .spacing(5)
            .align_y(Alignment::Center)
            .width(Length::Fixed(LABEL_W)),
        control,
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn setting_checkbox<'a>(
    label: &'a str,
    help: &'a str,
    checked: bool,
    on_toggle: Option<fn(bool) -> Message>,
) -> Element<'a, Message> {
    row![
        checkbox(label, checked).on_toggle_maybe(on_toggle).size(15),
        help_hint(help),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}

fn help_hint<'a>(help: &'a str) -> Element<'a, Message> {
    tooltip(
        container(text("?").size(11).color(ACCENT))
            .padding([1, 5])
            .style(help_badge_style),
        container(text(help).size(12))
            .padding(8)
            .max_width(300)
            .style(tooltip_style),
        tooltip::Position::Top,
    )
    .gap(5)
    .into()
}

fn note<'a>(value: &'a str) -> Element<'a, Message> {
    text(value).size(12).color(MUTED).into()
}

fn text_field<'a>(
    value: &'a str,
    placeholder: &'a str,
    on_change: fn(String) -> Message,
    secure: bool,
    width: Length,
    enabled: bool,
) -> Element<'a, Message> {
    let mut input = text_input(placeholder, value)
        .on_input_maybe(enabled.then_some(on_change))
        .padding(6)
        .size(13)
        .width(width);
    if secure {
        input = input.secure(true);
    }
    container(input)
        .height(Length::Fixed(FIELD_H))
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

fn card(content: Element<'_, Message>) -> Element<'_, Message> {
    container(content)
        .padding(12)
        .width(Length::Fill)
        .style(card_style)
        .into()
}

fn card_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: Some(palette.background.weak.text),
        ..Default::default()
    }
}

fn help_badge_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.strong.color)),
        border: Border {
            color: ACCENT,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..Default::default()
    }
}

fn tooltip_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.strong.color)),
        border: Border {
            color: ACCENT,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(palette.background.strong.text),
        ..Default::default()
    }
}
