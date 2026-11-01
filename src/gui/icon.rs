//! The app's icons: Lucide's (`assets/icons`, ISC license), compiled into
//! the executable so a portable copy needs nothing beside it.
//!
//! They are drawn in one color, the text's unless told otherwise, so they
//! follow the light and dark themes like the words next to them.

use iced::widget::{Svg, row, svg, text};
use iced::{Alignment, Color, Element, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Add,
    Back,
    Copy,
    Download,
    Failed,
    File,
    Link,
    Live,
    Log,
    Menu,
    New,
    Ok,
    OpenFolder,
    Queued,
    Remove,
    Retry,
    Stop,
    Trash,
    Warning,
    Working,
}

impl Icon {
    #[cfg(test)]
    pub const ALL: [Icon; 20] = [
        Icon::Add,
        Icon::Back,
        Icon::Copy,
        Icon::Download,
        Icon::Failed,
        Icon::File,
        Icon::Link,
        Icon::Live,
        Icon::Log,
        Icon::Menu,
        Icon::New,
        Icon::Ok,
        Icon::OpenFolder,
        Icon::Queued,
        Icon::Remove,
        Icon::Retry,
        Icon::Stop,
        Icon::Trash,
        Icon::Warning,
        Icon::Working,
    ];

    fn svg(self) -> &'static [u8] {
        match self {
            Icon::Add => include_bytes!("../../assets/icons/plus.svg"),
            Icon::Back => include_bytes!("../../assets/icons/chevron-left.svg"),
            Icon::Copy => include_bytes!("../../assets/icons/copy.svg"),
            Icon::Download => include_bytes!("../../assets/icons/download.svg"),
            Icon::Failed => include_bytes!("../../assets/icons/circle-x.svg"),
            Icon::File => include_bytes!("../../assets/icons/file.svg"),
            Icon::Link => include_bytes!("../../assets/icons/external-link.svg"),
            Icon::Live => include_bytes!("../../assets/icons/radio.svg"),
            Icon::Log => include_bytes!("../../assets/icons/list.svg"),
            Icon::Menu => include_bytes!("../../assets/icons/menu.svg"),
            Icon::New => include_bytes!("../../assets/icons/sparkles.svg"),
            Icon::Ok => include_bytes!("../../assets/icons/circle-check.svg"),
            Icon::OpenFolder => include_bytes!("../../assets/icons/folder-open.svg"),
            Icon::Queued => include_bytes!("../../assets/icons/clock.svg"),
            Icon::Remove => include_bytes!("../../assets/icons/x.svg"),
            Icon::Retry => include_bytes!("../../assets/icons/rotate-ccw.svg"),
            Icon::Stop => include_bytes!("../../assets/icons/square.svg"),
            Icon::Trash => include_bytes!("../../assets/icons/trash-2.svg"),
            Icon::Warning => include_bytes!("../../assets/icons/triangle-alert.svg"),
            Icon::Working => include_bytes!("../../assets/icons/loader-circle.svg"),
        }
    }
}

/// `icon`, `size` pixels square, in the text's color.
pub fn icon<'a>(icon: Icon, size: f32) -> Svg<'a> {
    tinted(icon, size, |theme| theme.palette().text)
}

/// `icon` in the color `color` picks for the theme: the accent, a
/// status color, the muted gray, or a button's text color.
pub fn tinted<'a>(icon: Icon, size: f32, color: impl Fn(&Theme) -> Color + 'a) -> Svg<'a> {
    svg(svg::Handle::from_memory(icon.svg()))
        .width(size)
        .height(size)
        .style(move |theme, _| svg::Style {
            color: Some(color(theme)),
        })
}

/// What a button with an icon is drawn on, which decides the icon's color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum On {
    /// An accent or destructive button: white, like its text.
    Color,
    /// Any other button: the text color.
    Plain,
}

/// A button's content: `icon` and `label` side by side, the icon a little
/// larger than the text and in the same color, faded like it when the
/// button is `enabled` no more.
pub fn labeled<'a, Message: 'a>(
    icon: Icon,
    label: String,
    size: f32,
    on: On,
    enabled: bool,
) -> Element<'a, Message> {
    let fade = if enabled { 1.0 } else { 0.5 };
    let color = move |theme: &Theme| {
        let base = match on {
            On::Color => Color::WHITE,
            On::Plain => theme.palette().text,
        };
        Color {
            a: base.a * fade,
            ..base
        }
    };
    row![tinted(icon, size + 2.0, color), text(label).size(size)]
        .spacing(size / 2.0)
        .align_y(Alignment::Center)
        .into()
}

/// A button that is only an icon, for an action on one item. `label`
/// says what it does in a tooltip, since an icon alone can be unclear.
pub fn button<'a, Message: Clone + 'a>(
    icon: Icon,
    label: String,
    on_press: Message,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        iced::widget::button(self::icon(icon, 18.0))
            .padding(super::style::SPACE_S)
            .style(super::style::flat(false))
            .on_press(on_press),
        iced::widget::container(text(label).size(super::style::CAPTION))
            .padding([super::style::SPACE_XS, super::style::SPACE_S])
            .style(super::style::popover),
        iced::widget::tooltip::Position::Bottom,
    )
    .gap(super::style::SPACE_XS)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon is one of Lucide's outline icons, drawn with the current
    /// color (which the tint replaces) and nothing that could reach out of
    /// the file.
    #[test]
    fn every_icon_is_a_plain_lucide_svg() {
        for icon in Icon::ALL {
            let text = std::str::from_utf8(icon.svg()).unwrap();
            assert!(text.contains("@license lucide-static"), "{icon:?}");
            assert!(text.contains("stroke=\"currentColor\""), "{icon:?}");
            for forbidden in ["<script", "href", "<image", "<foreignObject"] {
                assert!(!text.contains(forbidden), "{icon:?} has {forbidden}");
            }
        }
    }

    #[test]
    fn no_two_icons_share_a_file() {
        for (i, a) in Icon::ALL.iter().enumerate() {
            for b in &Icon::ALL[i + 1..] {
                assert_ne!(a.svg(), b.svg(), "{a:?} and {b:?}");
            }
        }
    }
}
