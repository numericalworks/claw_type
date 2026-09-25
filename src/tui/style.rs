//! Mapping the shared palette onto `ratatui` styles.
//!
//! Exposed as an extension trait so the drawing code can keep calling
//! `theme.bar_style()`, `theme.cursor_style()`, and so on.

use ratatui::style::{Color, Modifier, Style};

use crate::markdown::{MStyle, Role};
use crate::palette::{Rgb, Theme};

/// Convert a palette colour into a terminal colour, preserving true colour.
pub fn color(rgb: Rgb) -> Color {
    Color::Rgb(rgb.0, rgb.1, rgb.2)
}

/// Styling helpers layered on top of the shared [`Theme`].
pub trait ThemeExt {
    /// The concrete style for a semantic span, optionally dimmed.
    fn style_of(&self, span: MStyle, dim: bool) -> Style;
    fn text(&self) -> Style;
    fn dim_style(&self) -> Style;
    fn accent_style(&self) -> Style;
    fn bar_style(&self) -> Style;
    fn overlay_style(&self) -> Style;
    fn border_style(&self) -> Style;
    fn cursor_style(&self) -> Style;
    fn marker_color(&self) -> Color;
    fn bg_color(&self) -> Color;
}

impl ThemeExt for Theme {
    fn style_of(&self, span: MStyle, dim: bool) -> Style {
        if dim {
            return self.dim_style();
        }
        let mut style = Style::default().fg(color(self.role_color(span.role)));
        if matches!(span.role, Role::HeadingMarker(_)) {
            style = style.add_modifier(Modifier::DIM);
        }
        if span.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if span.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if span.strike {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if span.underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        style
    }

    fn text(&self) -> Style {
        Style::default().fg(color(self.fg))
    }

    fn dim_style(&self) -> Style {
        Style::default().fg(color(self.dim))
    }

    fn accent_style(&self) -> Style {
        Style::default()
            .fg(color(self.accent))
            .add_modifier(Modifier::BOLD)
    }

    fn bar_style(&self) -> Style {
        Style::default().fg(color(self.bar_fg)).bg(color(self.bar_bg))
    }

    fn overlay_style(&self) -> Style {
        Style::default().fg(color(self.fg)).bg(color(self.overlay_bg))
    }

    fn border_style(&self) -> Style {
        Style::default().fg(color(self.border))
    }

    fn cursor_style(&self) -> Style {
        Style::default()
            .fg(color(self.cursor_fg))
            .bg(color(self.cursor_bg))
    }

    fn marker_color(&self) -> Color {
        color(self.marker)
    }

    fn bg_color(&self) -> Color {
        color(self.bg)
    }
}
