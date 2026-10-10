//! Colors and widget styles shared by every screen.

use iced::overlay::menu;
use iced::theme::{Mode, Palette};
use iced::widget::{button, container, pick_list, progress_bar, rule, text, text_input, toggler};
use iced::{Border, Color, Font, Shadow, Theme, Vector, border, font};

use crate::config::ThemePref;
use crate::provision::Level;

/// The theme for the user's preference and, for [`ThemePref::System`], the
/// system's mode. An unknown mode (`Mode::None`) gets the dark theme.
pub fn theme(pref: ThemePref, system: Mode) -> Theme {
    let dark = match pref {
        ThemePref::Dark => true,
        ThemePref::Light => false,
        ThemePref::System => !matches!(system, Mode::Light),
    };
    if dark { dark_theme() } else { light_theme() }
}

// Both palettes are libadwaita's named colors.
fn dark_theme() -> Theme {
    Theme::custom(
        "MimirDLP Dark",
        Palette {
            background: Color::from_rgb8(0x22, 0x22, 0x26),
            text: Color::from_rgb8(0xff, 0xff, 0xff),
            primary: Color::from_rgb8(0x35, 0x84, 0xe4),
            success: Color::from_rgb8(0x8f, 0xf0, 0xa4),
            warning: Color::from_rgb8(0xf8, 0xe4, 0x5c),
            danger: Color::from_rgb8(0xff, 0x7b, 0x63),
        },
    )
}

fn light_theme() -> Theme {
    Theme::custom(
        "MimirDLP Light",
        Palette {
            background: Color::from_rgb8(0xfa, 0xfa, 0xfb),
            text: Color::from_rgb8(0x1e, 0x1e, 0x1e),
            primary: Color::from_rgb8(0x35, 0x84, 0xe4),
            success: Color::from_rgb8(0x1b, 0x85, 0x53),
            warning: Color::from_rgb8(0x9c, 0x6e, 0x03),
            danger: Color::from_rgb8(0xc0, 0x1c, 0x28),
        },
    )
}

/// Adwaita's raised surface (cards, the header bar, popovers) and the
/// hairline drawn around and between things on it.
fn surface(theme: &Theme) -> (Color, Color) {
    if theme.extended_palette().is_dark {
        (
            Color::from_rgb8(0x2e, 0x2e, 0x32),
            Color::from_rgba8(0xff, 0xff, 0xff, 0.08),
        )
    } else {
        (Color::WHITE, Color::from_rgba8(0x00, 0x00, 0x06, 0.10))
    }
}

/// The line between rows and menu sections.
pub fn separator(theme: &Theme) -> rule::Style {
    rule::Style {
        color: surface(theme).1,
        ..rule::default(theme)
    }
}

pub const BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..Font::DEFAULT
};

const SHADOW: Shadow = Shadow {
    color: Color::from_rgba8(0, 0, 0, 0.25),
    offset: Vector::new(0.0, 2.0),
    blur_radius: 12.0,
};

/// Adwaita's card: a lighter surface than the window (white in light mode),
/// 12 px corners and a hairline outline instead of a shadow.
pub fn card(theme: &Theme) -> container::Style {
    let (background, outline) = surface(theme);
    container::Style {
        background: Some(background.into()),
        border: Border {
            color: outline,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// The header bar runs edge to edge, so it has no corners.
pub fn header_bar(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(surface(theme).0.into()),
        ..container::Style::default()
    }
}

/// The menu floats over the screen, so it gets a shadow too.
pub fn popover(theme: &Theme) -> container::Style {
    container::Style {
        shadow: SHADOW,
        ..card(theme)
    }
}

/// A header bar or menu button: no background until hovered, like
/// Adwaita's flat buttons. `active` keeps the hover wash (an open menu).
pub fn flat(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let text = theme.palette().text;
        let wash = match status {
            button::Status::Pressed => 0.20,
            button::Status::Hovered => 0.08,
            _ if active => 0.12,
            _ => 0.0,
        };
        let mut text_color = text;
        if status == button::Status::Disabled {
            text_color.a = 0.4;
        }
        button::Style {
            background: Some(Color { a: wash, ..text }.into()),
            text_color,
            border: border::rounded(8),
            ..button::Style::default()
        }
    }
}

/// The main call to action: Adwaita's "suggested action", always white text
/// on the accent (iced would pick dark text on this blue in dark mode).
pub fn pill(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::primary(theme, status);
    style.text_color = Color::WHITE;
    if status == button::Status::Disabled {
        style.text_color.a = 0.5;
    }
    rounded(style)
}

/// Everything else a user can press: Adwaita's translucent flat button,
/// a wash of the text color that darkens on hover and press.
pub fn pill_secondary(theme: &Theme, status: button::Status) -> button::Style {
    let text = theme.palette().text;
    let wash = match status {
        button::Status::Active => 0.10,
        button::Status::Hovered => 0.15,
        button::Status::Pressed => 0.30,
        button::Status::Disabled => 0.05,
    };
    let mut text_color = text;
    if status == button::Status::Disabled {
        text_color.a = 0.5;
    }
    rounded(button::Style {
        background: Some(Color { a: wash, ..text }.into()),
        text_color,
        ..button::Style::default()
    })
}

/// Adwaita's "destructive action". Its own red, since the palette's danger
/// color is tuned for error text and is too pale for a dark-mode button.
pub fn pill_danger(theme: &Theme, status: button::Status) -> button::Style {
    let red = if theme.extended_palette().is_dark {
        Color::from_rgb8(0xc0, 0x1c, 0x28)
    } else {
        Color::from_rgb8(0xe0, 0x1b, 0x24)
    };
    let (background, text) = match status {
        button::Status::Active => (red, 1.0),
        button::Status::Hovered => (Color { a: 0.85, ..red }, 1.0),
        button::Status::Pressed => (Color { a: 0.7, ..red }, 1.0),
        button::Status::Disabled => (Color { a: 0.5, ..red }, 0.5),
    };
    rounded(button::Style {
        background: Some(background.into()),
        text_color: Color {
            a: text,
            ..Color::WHITE
        },
        ..button::Style::default()
    })
}

/// One choice of a row of choices (the theme in the menu): filled when it
/// is the current one.
pub fn segment(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if active {
            pill(theme, status)
        } else {
            rounded(button::text(theme, status))
        }
    }
}

fn rounded(mut style: button::Style) -> button::Style {
    style.border = border::rounded(999);
    style
}

/// Adwaita's dropdown: a flat button with a light wash, no outline.
pub fn dropdown(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let text = theme.palette().text;
    let wash = match status {
        pick_list::Status::Active => 0.08,
        pick_list::Status::Hovered => 0.12,
        pick_list::Status::Opened { .. } => 0.18,
    };
    pick_list::Style {
        text_color: text,
        placeholder_color: muted(theme),
        handle_color: text,
        background: Color { a: wash, ..text }.into(),
        border: border::rounded(8),
    }
}

/// The list a dropdown opens: a popover, the current choice washed rather
/// than filled with the accent.
pub fn dropdown_menu(theme: &Theme) -> menu::Style {
    let (background, outline) = surface(theme);
    let text = theme.palette().text;
    menu::Style {
        background: background.into(),
        border: Border {
            color: outline,
            width: 1.0,
            radius: 8.0.into(),
        },
        text_color: text,
        selected_text_color: text,
        selected_background: Color { a: 0.10, ..text }.into(),
        shadow: SHADOW,
    }
}

/// Adwaita's entry: a washed field with no outline until it has focus,
/// then a ring in the accent.
pub fn entry(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.palette();
    let text = palette.text;
    let wash = match status {
        text_input::Status::Hovered => 0.10,
        text_input::Status::Disabled => 0.04,
        _ => 0.08,
    };
    let ring = match status {
        text_input::Status::Focused { .. } => Border {
            color: palette.primary,
            width: 2.0,
            radius: 8.0.into(),
        },
        _ => border::rounded(8),
    };
    text_input::Style {
        background: Color { a: wash, ..text }.into(),
        border: ring,
        icon: muted(theme),
        placeholder: Color { a: 0.45, ..text },
        value: if status == text_input::Status::Disabled {
            muted(theme)
        } else {
            text
        },
        selection: Color {
            a: 0.35,
            ..palette.primary
        },
    }
}

/// Adwaita's switch: the accent when on, a grey wash when off, and a white
/// knob either way.
pub fn switch(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let text = theme.palette().text;
    let (on, hovered, disabled) = match status {
        toggler::Status::Active { is_toggled } => (is_toggled, false, false),
        toggler::Status::Hovered { is_toggled } => (is_toggled, true, false),
        toggler::Status::Disabled { is_toggled } => (is_toggled, false, true),
    };
    let mut track = if on {
        theme.palette().primary
    } else {
        Color {
            a: if hovered { 0.30 } else { 0.22 },
            ..text
        }
    };
    let mut knob = Color::WHITE;
    if disabled {
        track.a *= 0.5;
        knob.a = 0.6;
    }
    toggler::Style {
        background: track.into(),
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: knob.into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: 0.1,
    }
}

/// A card that is itself a button (the "Advanced options" row).
pub fn expander(theme: &Theme, status: button::Status) -> button::Style {
    let (surface, outline) = surface(theme);
    let text = theme.palette().text;
    let wash = match status {
        button::Status::Hovered => 0.04,
        button::Status::Pressed => 0.08,
        _ => 0.0,
    };
    button::Style {
        background: Some(mix(surface, Color { a: wash, ..text }).into()),
        text_color: text,
        border: Border {
            color: outline,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..button::Style::default()
    }
}

/// `wash` laid over `base`, so a translucent hover stays opaque.
fn mix(base: Color, wash: Color) -> Color {
    let a = wash.a;
    Color::from_rgb(
        base.r * (1.0 - a) + wash.r * a,
        base.g * (1.0 - a) + wash.g * a,
        base.b * (1.0 - a) + wash.b * a,
    )
}

pub fn progress(theme: &Theme) -> progress_bar::Style {
    let text = theme.palette().text;
    progress_bar::Style {
        background: Color { a: 0.12, ..text }.into(),
        bar: theme.palette().primary.into(),
        border: Border::default().rounded(999),
    }
}

/// Secondary text: hints, sizes, empty states.
pub fn muted(theme: &Theme) -> Color {
    let mut color = theme.palette().text;
    color.a = 0.55;
    color
}

/// [`muted`] as a text style.
pub fn muted_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(muted(theme)),
    }
}

/// The color for each log tag.
pub fn level(theme: &Theme, level: Level) -> Color {
    let palette = theme.palette();
    match level {
        Level::Info => palette.primary,
        Level::Install | Level::Download | Level::Ok | Level::Success => palette.success,
        Level::Verify | Level::Warn => palette.warning,
        Level::Error => palette.danger,
    }
}
