//! Iced view tree. Compact two-card layout with conditional rendering:
//! Direct mode hides the proxy detail rows, "Authentication off" hides the
//! login/password rows. Keeps the window short so it fits on small displays.

use iced::widget::{
    button, checkbox, column, container, pick_list, row, text, text_input, Space,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use redrover_config::UdpStrategy;

use crate::app::{App, Message, ProxyKindChoice, Status};

const ACCENT: Color = Color::from_rgb(0.40, 0.55, 0.95);
const OK_COLOR: Color = Color::from_rgb(0.30, 0.78, 0.42);
const ERR_COLOR: Color = Color::from_rgb(0.93, 0.36, 0.36);
const MUTED: Color = Color::from_rgb(0.62, 0.65, 0.72);

const LABEL_W: f32 = 64.0;
const FIELD_H: f32 = 28.0;

pub fn view(app: &App) -> Element<'_, Message> {
    container(
        column![
            title_bar(),
            proxy_card(app),
            voice_card(app),
            actions(app),
        ]
        .spacing(10),
    )
    .padding(14)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// --- Title bar -------------------------------------------------------------

fn title_bar<'a>() -> Element<'a, Message> {
    row![
        text("Redrover").size(20),
        Space::with_width(Length::Fill),
        text(concat!("v", env!("CARGO_PKG_VERSION")))
            .size(11)
            .color(MUTED),
    ]
    .align_y(Alignment::Center)
    .into()
}

// --- Proxy card ------------------------------------------------------------

fn proxy_card(app: &App) -> Element<'_, Message> {
    let mut col = column![section_header("Proxy"), proxy_mode_picker(app)].spacing(8);

    let direct = matches!(app.kind, ProxyKindChoice::Direct);
    if !direct {
        col = col
            .push(labeled("Host", text_field(&app.host, "proxy.example.com", Message::HostChanged, false, Length::Fill)))
            .push(labeled("Port", text_field(&app.port, "1080", Message::PortChanged, false, Length::Fixed(90.0))))
            .push(checkbox("Authentication", app.auth).on_toggle(Message::AuthToggled).size(15));
        if app.auth {
            col = col
                .push(labeled("Login", text_field(&app.login, "user", Message::LoginChanged, false, Length::Fill)))
                .push(labeled("Pass", text_field(&app.password, "•••••••", Message::PasswordChanged, true, Length::Fill)));
        }
    } else {
        col = col.push(
            text("UDP-only mode: no proxy, only voice-channel mangling.")
                .size(12)
                .color(MUTED),
        );
    }

    card(col.into())
}

fn proxy_mode_picker(app: &App) -> Element<'_, Message> {
    let mut r = row![].spacing(6);
    for &k in ProxyKindChoice::ALL {
        let label = k.label();
        let is_active = k == app.kind;
        let btn = button(text(label).size(13).center())
            .on_press(Message::ProxyKindSelected(k))
            .padding(Padding {
                top: 5.0,
                bottom: 5.0,
                left: 12.0,
                right: 12.0,
            })
            .style(if is_active {
                button::primary
            } else {
                button::secondary
            });
        r = r.push(btn);
    }
    r.into()
}

// --- Voice / UDP card ------------------------------------------------------

fn voice_card(app: &App) -> Element<'_, Message> {
    let strategy_pick = pick_list(
        UdpStrategy::ALL,
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

    let col = column![
        section_header("Voice"),
        labeled("Strategy", strategy_pick.into()),
        labeled(
            "Delay",
            row![
                text_field(&app.udp_delay_ms, "50", Message::UdpDelayChanged, false, Length::Fixed(80.0)),
                text("ms").size(12).color(MUTED),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
        ),
        checkbox("Force TCP fallback", app.udp_force_tcp_fallback)
            .on_toggle(Message::UdpForceTcpToggled)
            .size(15),
        checkbox("SOCKS5 UDP associate", app.socks5_udp_associate)
            .on_toggle(Message::Socks5UdpAssociateToggled)
            .size(15),
        checkbox("Debug console", app.log_console)
            .on_toggle(Message::LogConsoleToggled)
            .size(15),
    ]
    .spacing(8);

    card(col.into())
}

// --- Actions + status ------------------------------------------------------

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

    column![
        row![install_btn, uninstall_btn].spacing(8),
        status_row(app),
    ]
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

// --- Building blocks -------------------------------------------------------

fn section_header<'a>(title: &'a str) -> Element<'a, Message> {
    text(title)
        .size(11)
        .color(ACCENT)
        .into()
}

fn labeled<'a>(label: &'a str, control: Element<'a, Message>) -> Element<'a, Message> {
    row![
        text(label).size(13).width(Length::Fixed(LABEL_W)).color(MUTED),
        control,
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn text_field<'a>(
    value: &'a str,
    placeholder: &'a str,
    on_change: impl Fn(String) -> Message + 'a,
    secure: bool,
    width: Length,
) -> Element<'a, Message> {
    let mut input = text_input(placeholder, value)
        .on_input(on_change)
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
