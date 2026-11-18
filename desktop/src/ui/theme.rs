//! "Graphite glass" design tokens from `frontend/src/index.css`, as iced styles.
//! The web version layers translucent glass over a blurred landscape; natively we
//! use the same colours over a static gradient (no live blur, which keeps idle and
//! scrolling cost negligible).

use iced::border::Radius;
use iced::gradient::Linear;
use iced::widget::{button, container, scrollable, text_editor, text_input};
use iced::{Background, Border, Color, Degrees, Font, Gradient, Shadow, Theme, Vector, font};

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

pub const fn white(alpha: f32) -> Color {
    Color::from_rgba(1.0, 1.0, 1.0, alpha)
}

pub const fn black(alpha: f32) -> Color {
    Color::from_rgba(0.0, 0.0, 0.0, alpha)
}

// Text
pub const TEXT_1: Color = rgb(0xf2, 0xf0, 0xec);
pub const TEXT_2: Color = rgb(0xbd, 0xba, 0xb4);
pub const TEXT_3: Color = rgb(0x90, 0x8d, 0x87);
// Accent & status
pub const ACCENT: Color = rgb(0xb6, 0xcc, 0xff);
pub const ACCENT_SOFT: Color = rgba(182, 204, 255, 0.12);
pub const SUCCESS: Color = rgb(0x86, 0xd7, 0xaa);
pub const SUCCESS_SOFT: Color = rgba(134, 215, 170, 0.13);
pub const WARNING: Color = rgb(0xf0, 0xc4, 0x7a);
pub const DANGER: Color = rgb(0xff, 0x95, 0x92);
pub const DANGER_STRONG: Color = rgb(0xe5, 0x53, 0x4b);
pub const DANGER_SOFT: Color = rgba(255, 120, 115, 0.12);
// Primary action: porcelain on graphite
pub const PRIMARY: Color = rgb(0xf2, 0xef, 0xea);
pub const PRIMARY_HOVER: Color = rgb(0xff, 0xff, 0xff);
pub const PRIMARY_TEXT: Color = rgb(0x15, 0x15, 0x17);
// Surfaces
pub const SURFACE_1: Color = white(0.035);
pub const SURFACE_2: Color = white(0.06);
pub const SURFACE_3: Color = white(0.09);
pub const SURFACE_4: Color = white(0.13);
pub const SUNKEN: Color = black(0.22);
pub const LINE: Color = white(0.07);
pub const LINE_STRONG: Color = white(0.12);
pub const LINE_FOCUS: Color = rgba(186, 208, 255, 0.55);
// Shell: the glass window colour composited over the backdrop.
pub const SHELL_BOTTOM: Color = rgb(0x17, 0x18, 0x1a);
pub const FLOAT: Color = rgb(0x24, 0x24, 0x28);
pub const FLOAT_SOLID: Color = rgb(0x1f, 0x1f, 0x22);
// Bubbles
pub const BUBBLE_IN: Color = white(0.075);
pub const BUBBLE_IN_LINE: Color = white(0.06);
pub const BUBBLE_OUT: Color = rgb(0xe9, 0xe5, 0xdf);
pub const BUBBLE_OUT_TEXT: Color = rgb(0x18, 0x18, 0x1a);
pub const BUBBLE_OUT_TEXT_2: Color = rgba(22, 22, 24, 0.58);

// Type scale (px)
pub const TEXT_XS: f32 = 11.5;
pub const TEXT_SM: f32 = 13.0;
pub const TEXT_MD: f32 = 14.5;
pub const TEXT_LG: f32 = 16.0;
pub const TEXT_XL: f32 = 20.0;
pub const TEXT_2XL: f32 = 26.0;

// Radii
pub const RADIUS_SM: f32 = 10.0;
pub const RADIUS_MD: f32 = 14.0;
pub const RADIUS_XL: f32 = 24.0;

pub const RAIL_WIDTH: f32 = 68.0;

#[cfg(windows)]
pub const FONT_FAMILY: &str = "Segoe UI";
#[cfg(not(windows))]
pub const FONT_FAMILY: &str = "DejaVu Sans";

pub const REGULAR: Font = Font::with_name(FONT_FAMILY);
pub const MEDIUM: Font = Font { weight: font::Weight::Medium, ..REGULAR };
pub const SEMIBOLD: Font = Font { weight: font::Weight::Semibold, ..REGULAR };
pub const BOLD: Font = Font { weight: font::Weight::Bold, ..REGULAR };

pub fn app_theme() -> Theme {
    Theme::custom(
        "Graphite glass",
        iced::theme::Palette {
            background: SHELL_BOTTOM,
            text: TEXT_1,
            primary: ACCENT,
            success: SUCCESS,
            warning: WARNING,
            danger: DANGER,
        },
    )
}

fn linear(angle: f32, top: Color, bottom: Color) -> Background {
    Background::Gradient(Gradient::Linear(Linear::new(Degrees(angle)).add_stop(0.0, top).add_stop(1.0, bottom)))
}

pub fn shadow(color: Color, y: f32, blur: f32) -> Shadow {
    Shadow { color, offset: Vector::new(0.0, y), blur_radius: blur }
}

// ----------------------------------------------------------------------------
// Containers
// ----------------------------------------------------------------------------

/// The whole window: web `--canvas` (the backdrop image and glass window are drawn on top).
pub fn shell(_: &Theme) -> container::Style {
    container::Style {
        background: Some(rgb(0x0d, 0x0d, 0x0f).into()),
        text_color: Some(TEXT_1),
        ..container::Style::default()
    }
}

/// `corner`: the window radius, so the rail's tint follows the rounded left edge.
pub fn rail(corner: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(black(0.14).into()),
        border: Border { color: LINE, width: 0.0, radius: Radius { top_left: corner, bottom_left: corner, ..Radius::default() } },
        ..container::Style::default()
    }
}

pub fn sidebar(_: &Theme) -> container::Style {
    container::Style { background: Some(black(0.06).into()), ..container::Style::default() }
}

pub fn chat_header(corner: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(white(0.015).into()),
        border: Border { radius: Radius { top_right: corner, ..Radius::default() }, ..Border::default() },
        ..container::Style::default()
    }
}

pub fn divider(_: &Theme) -> container::Style {
    container::Style { background: Some(LINE.into()), ..container::Style::default() }
}

pub fn sunken_field(focused: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(SUNKEN.into()),
        border: Border { color: if focused { LINE_FOCUS } else { LINE }, width: 1.0, radius: 12.0.into() },
        ..container::Style::default()
    }
}

pub fn card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE_1.into()),
        border: Border { color: LINE, width: 1.0, radius: RADIUS_MD.into() },
        ..container::Style::default()
    }
}

/// Web `.auth-card`: rgba(24,24,27,.62) blurred over the backdrop centre, pre-composited
/// (iced draws shadows beneath translucent fills, which would darken it).
pub fn auth_card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(rgb(0x27, 0x26, 0x27).into()),
        text_color: Some(TEXT_1),
        border: Border { color: white(0.1), width: 1.0, radius: RADIUS_XL.into() },
        shadow: shadow(black(0.75), 40.0, 120.0),
        ..container::Style::default()
    }
}

/// Floating surfaces: dialogs, menus, toasts.
pub fn float(_: &Theme) -> container::Style {
    container::Style {
        background: Some(FLOAT_SOLID.into()),
        text_color: Some(TEXT_1),
        border: Border { color: LINE_STRONG, width: 1.0, radius: RADIUS_XL.into() },
        shadow: shadow(black(0.6), 24.0, 60.0),
        ..container::Style::default()
    }
}

pub fn menu(_: &Theme) -> container::Style {
    container::Style {
        background: Some(FLOAT.into()),
        text_color: Some(TEXT_1),
        border: Border { color: LINE_STRONG, width: 1.0, radius: RADIUS_MD.into() },
        shadow: shadow(black(0.55), 10.0, 28.0),
        ..container::Style::default()
    }
}

pub fn backdrop(_: &Theme) -> container::Style {
    container::Style { background: Some(rgba(6, 6, 8, 0.62).into()), ..container::Style::default() }
}

pub fn bubble(own: bool, top_join: bool, bottom_join: bool, glow: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let big = 18.0;
        let small = 6.0;
        // Runs of messages from one sender get tighter corners on the joined side.
        let radius = if own {
            Radius { top_left: big, top_right: if top_join { small } else { big }, bottom_right: small, bottom_left: big }
        } else {
            Radius { top_left: if top_join { small } else { big }, top_right: big, bottom_right: big, bottom_left: small }
        };
        let radius = if bottom_join {
            if own { Radius { bottom_right: small, ..radius } } else { Radius { bottom_left: small, ..radius } }
        } else {
            radius
        };
        let (background, border, text) = if own {
            (BUBBLE_OUT, Color::TRANSPARENT, BUBBLE_OUT_TEXT)
        } else {
            (BUBBLE_IN, BUBBLE_IN_LINE, TEXT_1)
        };
        container::Style {
            background: Some(background.into()),
            text_color: Some(text),
            border: Border {
                color: if glow > 0.0 { mix(border, ACCENT, glow) } else { border },
                width: 1.0 + glow,
                radius,
            },
            shadow: if glow > 0.0 {
                Shadow { color: Color { a: 0.45 * glow, ..ACCENT }, offset: Vector::new(0.0, 0.0), blur_radius: 18.0 * glow }
            } else {
                // Web own: 0 1px 1px .18 + 0 10px 26px -18px .7. Incoming bubbles are translucent,
                // and iced paints shadows beneath the fill, so they get none.
                if own { shadow(black(0.22), 2.0, 6.0) } else { Shadow::default() }
            },
            ..container::Style::default()
        }
    }
}

pub fn date_chip(_: &Theme) -> container::Style {
    container::Style {
        background: Some(rgba(18, 18, 20, 0.55).into()),
        text_color: Some(TEXT_2),
        border: Border { color: LINE, width: 1.0, radius: 999.0.into() },
        ..container::Style::default()
    }
}

pub fn badge(_: &Theme) -> container::Style {
    container::Style {
        background: Some(PRIMARY.into()),
        text_color: Some(PRIMARY_TEXT),
        border: Border { radius: 999.0.into(), ..Border::default() },
        ..container::Style::default()
    }
}

pub fn pill(color: Color, background: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(background.into()),
        text_color: Some(color),
        border: Border { radius: 999.0.into(), ..Border::default() },
        ..container::Style::default()
    }
}

pub fn avatar(hue: f32, group: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| {
        let (s1, l1, s2, l2, ts, tl) = if group { (0.14, 0.40, 0.12, 0.22, 0.22, 0.88) } else { (0.22, 0.46, 0.18, 0.26, 0.30, 0.92) };
        container::Style {
            background: Some(linear(160.0, hsla(hue, s1, l1, 0.95), hsla(hue, s2, l2, 0.95))),
            text_color: Some(hsla(hue, ts, tl, 1.0)),
            border: Border { color: white(0.08), width: 1.0, radius: 999.0.into() },
            ..container::Style::default()
        }
    }
}

pub fn presence_dot(_: &Theme) -> container::Style {
    container::Style {
        background: Some(SUCCESS.into()),
        border: Border { color: rgb(0x1b, 0x1b, 0x1e), width: 2.5, radius: 999.0.into() },
        ..container::Style::default()
    }
}

/// Web `.message-form`. Its --shadow-md would be painted beneath the translucent fill by
/// iced (turning the field nearly black), so the native version has none.
pub fn composer(focused: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(white(if focused { 0.075 } else { 0.06 }).into()),
        border: Border { color: if focused { rgba(182, 204, 255, 0.34) } else { LINE_STRONG }, width: 1.0, radius: 20.0.into() },
        ..container::Style::default()
    }
}

pub fn composer_preview(_: &Theme) -> container::Style {
    container::Style {
        background: Some(white(0.045).into()),
        border: Border { color: LINE, width: 1.0, radius: RADIUS_MD.into() },
        ..container::Style::default()
    }
}

pub fn reply_quote(own: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(if own { black(0.06) } else { white(0.05) }.into()),
        border: Border { color: if own { black(0.25) } else { ACCENT }, width: 0.0, radius: 8.0.into() },
        ..container::Style::default()
    }
}

pub fn quote_bar(own: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(if own { rgba(59, 99, 196, 0.85) } else { ACCENT }.into()),
        border: Border { radius: 2.0.into(), ..Border::default() },
        ..container::Style::default()
    }
}

pub fn toast(error: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(if error { rgb(0x3a, 0x22, 0x23) } else { FLOAT }.into()),
        text_color: Some(if error { rgb(0xff, 0xd7, 0xd5) } else { TEXT_1 }),
        border: Border { color: if error { rgba(255, 149, 146, 0.35) } else { LINE_STRONG }, width: 1.0, radius: RADIUS_MD.into() },
        shadow: shadow(black(0.5), 10.0, 28.0),
        ..container::Style::default()
    }
}

pub fn pinned_bar(_: &Theme) -> container::Style {
    container::Style {
        background: Some(black(0.08).into()),
        ..container::Style::default()
    }
}

pub fn image_viewer(_: &Theme) -> container::Style {
    container::Style { background: Some(rgba(6, 6, 8, 0.94).into()), text_color: Some(TEXT_1), ..container::Style::default() }
}

pub fn update_banner(_: &Theme) -> container::Style {
    container::Style {
        background: Some(ACCENT_SOFT.into()),
        text_color: Some(TEXT_1),
        border: Border { color: rgba(182, 204, 255, 0.28), width: 1.0, radius: RADIUS_MD.into() },
        ..container::Style::default()
    }
}

// ----------------------------------------------------------------------------
// Buttons
// ----------------------------------------------------------------------------

fn base_button(background: Option<Color>, text: Color, radius: f32) -> button::Style {
    button::Style {
        background: background.map(Background::Color),
        text_color: text,
        border: Border { radius: radius.into(), ..Border::default() },
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Porcelain primary action.
pub fn primary_button(_: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => PRIMARY_HOVER,
        button::Status::Disabled => white(0.22),
        button::Status::Active => PRIMARY,
    };
    let text = if status == button::Status::Disabled { white(0.45) } else { PRIMARY_TEXT };
    button::Style { shadow: shadow(black(0.28), 1.0, 2.0), ..base_button(Some(background), text, 12.0) }
}

pub fn secondary_button(_: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => SURFACE_3,
        button::Status::Pressed => SURFACE_4,
        _ => SURFACE_2,
    };
    let text = if status == button::Status::Disabled { TEXT_3 } else { TEXT_1 };
    button::Style { border: Border { color: LINE_STRONG, width: 1.0, radius: 12.0.into() }, ..base_button(Some(background), text, 12.0) }
}

pub fn danger_button(_: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => rgba(229, 83, 75, 0.9),
        button::Status::Disabled => rgba(229, 83, 75, 0.35),
        button::Status::Active => DANGER_STRONG,
    };
    base_button(Some(background), rgb(0xff, 0xff, 0xff), 12.0)
}

pub fn danger_text_button(_: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Some(DANGER_SOFT),
        _ => None,
    };
    base_button(background, DANGER, 10.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    /// Accent action (new group).
    Primary,
    Normal,
    /// Destructive (log out): turns red on hover.
    Danger,
}

impl Tile {
    /// Glyph colour for the tile at rest / hovered.
    pub fn glyph(self, hovered: bool) -> Color {
        match (self, hovered) {
            (Tile::Primary, false) => ACCENT,
            (Tile::Primary, true) => rgb(0xd6, 0xe2, 0xff),
            (Tile::Danger, true) => DANGER,
            (_, false) => TEXT_2,
            (_, true) => TEXT_1,
        }
    }
}

/// Rail buttons: soft glass tiles with a hairline border and a gentle lift on hover.
pub fn rail_tile(tile: Tile, hovered: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = hovered || matches!(status, button::Status::Hovered);
        let pressed = matches!(status, button::Status::Pressed);
        let (background, border) = match (tile, hovered || pressed) {
            (Tile::Primary, false) => (rgba(182, 204, 255, 0.10), rgba(182, 204, 255, 0.22)),
            (Tile::Primary, true) => (rgba(182, 204, 255, 0.20), rgba(182, 204, 255, 0.42)),
            (Tile::Danger, true) => (DANGER_SOFT, rgba(255, 149, 146, 0.30)),
            (_, false) => (white(0.035), white(0.06)),
            (_, true) => (white(0.10), white(0.16)),
        };
        let lift = if hovered && !pressed { shadow(black(0.35), 4.0, 12.0) } else { Shadow::default() };
        button::Style {
            background: Some(if pressed { mix(background, white(0.2), 0.3) } else { background }.into()),
            text_color: tile.glyph(hovered),
            border: Border { color: border, width: 1.0, radius: RADIUS_MD.into() },
            shadow: lift,
            snap: true,
        }
    }
}

/// Transparent icon button (rail, header, composer).
pub fn icon_button(_: &Theme, status: button::Status) -> button::Style {
    let (background, text) = match status {
        button::Status::Hovered => (Some(SURFACE_2), TEXT_1),
        button::Status::Pressed => (Some(SURFACE_3), TEXT_1),
        button::Status::Disabled => (None, white(0.25)),
        button::Status::Active => (None, TEXT_3),
    };
    base_button(background, text, 12.0)
}

pub fn send_button(_: &Theme, status: button::Status) -> button::Style {
    let (background, text) = match status {
        button::Status::Hovered | button::Status::Pressed => (PRIMARY_HOVER, PRIMARY_TEXT),
        button::Status::Disabled => (SURFACE_2, TEXT_3),
        button::Status::Active => (PRIMARY, PRIMARY_TEXT),
    };
    let shadow = if status == button::Status::Disabled { Shadow::default() } else { shadow(black(0.28), 1.0, 2.0) };
    button::Style { shadow, ..base_button(Some(background), text, 14.0) }
}

/// Web `.call-button` / `.pinned-list-toggle`: a quiet bordered tile.
pub fn outline_button(radius: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (background, text) = match status {
            button::Status::Hovered | button::Status::Pressed => (SURFACE_3, TEXT_1),
            button::Status::Disabled => (white(0.014), white(0.36)),
            button::Status::Active => (SURFACE_1, TEXT_2),
        };
        button::Style { border: Border { color: LINE, width: 1.0, radius: radius.into() }, ..base_button(Some(background), text, radius) }
    }
}

pub fn chat_item(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = if active {
            Some(white(0.085))
        } else {
            match status {
                button::Status::Hovered => Some(SURFACE_2),
                button::Status::Pressed => Some(SURFACE_3),
                _ => None,
            }
        };
        button::Style {
            border: Border { color: if active { white(0.06) } else { Color::TRANSPARENT }, width: 1.0, radius: RADIUS_MD.into() },
            ..base_button(background, TEXT_1, RADIUS_MD)
        }
    }
}

pub fn list_button(_: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Some(SURFACE_2),
        button::Status::Pressed => Some(SURFACE_3),
        _ => None,
    };
    base_button(background, TEXT_1, RADIUS_SM)
}

pub fn menu_item(danger: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match status {
            button::Status::Hovered | button::Status::Pressed => Some(if danger { DANGER_SOFT } else { SURFACE_2 }),
            _ => None,
        };
        base_button(background, if danger { DANGER } else { TEXT_1 }, 9.0)
    }
}

pub fn tab_button(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = if active {
            Some(SURFACE_3)
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => Some(SURFACE_2),
                _ => None,
            }
        };
        base_button(background, if active { TEXT_1 } else { TEXT_2 }, 12.0)
    }
}

pub fn link_button(_: &Theme, status: button::Status) -> button::Style {
    let text = match status {
        button::Status::Hovered | button::Status::Pressed => TEXT_1,
        button::Status::Disabled => TEXT_3,
        button::Status::Active => ACCENT,
    };
    base_button(None, text, 6.0)
}

/// Invisible button wrapping arbitrary content (images, reply quotes, headers).
pub fn bare_button(_: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Some(white(0.03)),
        _ => None,
    };
    base_button(background, TEXT_1, 10.0)
}

pub fn floating_round_button(_: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => SURFACE_4,
        _ => FLOAT,
    };
    button::Style {
        border: Border { color: LINE_STRONG, width: 1.0, radius: 999.0.into() },
        shadow: shadow(black(0.5), 8.0, 20.0),
        ..base_button(Some(background), TEXT_1, 999.0)
    }
}

// ----------------------------------------------------------------------------
// Inputs
// ----------------------------------------------------------------------------

/// Text field with its own sunken background (forms).
pub fn field(_: &Theme, status: text_input::Status) -> text_input::Style {
    let border = match status {
        text_input::Status::Focused { .. } => LINE_FOCUS,
        text_input::Status::Hovered => LINE_STRONG,
        _ => LINE,
    };
    text_input::Style {
        background: SUNKEN.into(),
        border: Border { color: border, width: 1.0, radius: 12.0.into() },
        icon: TEXT_3,
        placeholder: TEXT_3,
        value: TEXT_1,
        selection: rgba(182, 204, 255, 0.32),
    }
}

/// Borderless text field placed inside a styled container (search, composer).
pub fn bare_field(_: &Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Color::TRANSPARENT.into(),
        border: Border::default(),
        icon: TEXT_3,
        placeholder: TEXT_3,
        value: TEXT_1,
        selection: rgba(182, 204, 255, 0.32),
    }
}

pub fn editor(_: &Theme, status: text_editor::Status) -> text_editor::Style {
    let border = match status {
        text_editor::Status::Focused { .. } => LINE_FOCUS,
        text_editor::Status::Hovered => LINE_STRONG,
        _ => LINE,
    };
    text_editor::Style {
        background: SUNKEN.into(),
        border: Border { color: border, width: 1.0, radius: 12.0.into() },
        placeholder: TEXT_3,
        value: TEXT_1,
        selection: rgba(182, 204, 255, 0.32),
    }
}

pub fn scroll(_: &Theme, status: scrollable::Status) -> scrollable::Style {
    let hovered = matches!(status, scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. });
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: (if hovered { white(0.22) } else { white(0.1) }).into(),
            border: Border { radius: 999.0.into(), ..Border::default() },
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: FLOAT.into(),
            border: Border { color: LINE_STRONG, width: 1.0, radius: 999.0.into() },
            shadow: Shadow::default(),
            icon: TEXT_2,
        },
    }
}

/// Linear blend between two colours.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

// ----------------------------------------------------------------------------
// Colour helpers
// ----------------------------------------------------------------------------

/// CSS-style HSL(A) → [`Color`]. `hue` in degrees, the rest in 0..=1.
pub fn hsla(hue: f32, saturation: f32, lightness: f32, alpha: f32) -> Color {
    let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h = (hue.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = lightness - c / 2.0;
    Color::from_rgba(r + m, g + m, b + m, alpha)
}

/// Stable hue per id, identical to the web's `avatarStyle` (Java-style string hash).
pub fn hue_for(seed: &str) -> f32 {
    let mut hash: i32 = 0;
    for unit in seed.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(unit as i32);
    }
    (hash.unsigned_abs() % 360) as f32
}

/// Group author names use the avatar hue at readable lightness.
pub fn author_color(seed: &str) -> Color {
    hsla(hue_for(seed), 0.45, 0.74, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_matches_web_hash() {
        // JS: let h=0; for (c of "abc") h=(h*31+c.charCodeAt(0))|0; Math.abs(h)%360 → 96354 % 360
        assert_eq!(hue_for("abc"), (96354 % 360) as f32);
        assert_eq!(hue_for(""), 0.0);
    }

    #[test]
    fn hsl_converts_primaries() {
        let red = hsla(0.0, 1.0, 0.5, 1.0);
        assert!((red.r - 1.0).abs() < 1e-4 && red.g.abs() < 1e-4 && red.b.abs() < 1e-4);
        let grey = hsla(200.0, 0.0, 0.5, 0.5);
        assert!((grey.r - 0.5).abs() < 1e-4 && (grey.a - 0.5).abs() < 1e-4);
    }
}
