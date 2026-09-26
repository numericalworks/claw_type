//! The colour palette.
//!
//! Colours are stored as plain RGB triples and converted where they are used.
//! Semantic roles are resolved to colours here, which keeps the Markdown parser
//! free of any appearance concerns.

use crate::markdown::Role;

/// An 8-bit-per-channel colour.
pub type Rgb = (u8, u8, u8);

/// The editor's palette.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Window background.
    pub bg: Rgb,
    /// Default body text.
    pub fg: Rgb,
    /// De-emphasised text (focus mode, hints).
    pub dim: Rgb,
    /// The single accent used for interactive hints.
    pub accent: Rgb,
    /// Heading colours, `h1`..=`h6`.
    pub heading: [Rgb; 6],
    /// Inline and fenced code.
    pub code: Rgb,
    /// Block quotes.
    pub quote: Rgb,
    /// List markers and numbers.
    pub list: Rgb,
    /// Link text.
    pub link: Rgb,
    /// Horizontal rules.
    pub rule: Rgb,
    /// Markdown punctuation.
    pub marker: Rgb,
    /// Status bar.
    pub bar_fg: Rgb,
    pub bar_bg: Rgb,
    /// Panels drawn on top of the editor.
    pub overlay_bg: Rgb,
    pub border: Rgb,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: (0x17, 0x1a, 0x20),
            fg: (0xd7, 0xdc, 0xe5),
            dim: (0x4b, 0x53, 0x62),
            accent: (0x62, 0xa9, 0xf5),
            heading: [
                (0xe6, 0x7a, 0x83),
                (0xd8, 0x9a, 0x60),
                (0xe2, 0xc0, 0x74),
                (0x9b, 0xc8, 0x7a),
                (0x6a, 0xb2, 0xe6),
                (0xc0, 0x82, 0xd8),
            ],
            code: (0x9b, 0xc8, 0x7a),
            quote: (0x8a, 0x92, 0xa0),
            list: (0x62, 0xa9, 0xf5),
            link: (0x59, 0xba, 0xc4),
            rule: (0x3a, 0x40, 0x4b),
            marker: (0x50, 0x58, 0x66),
            bar_fg: (0x6f, 0x77, 0x86),
            bar_bg: (0x11, 0x13, 0x18),
            overlay_bg: (0x1e, 0x22, 0x2a),
            border: (0x3a, 0x40, 0x4b),
        }
    }
}

impl Theme {
    /// A light counterpart, used by the browser preview's dark/light toggle.
    pub fn light() -> Self {
        Self {
            bg: (0xfb, 0xfb, 0xfa),
            fg: (0x21, 0x25, 0x2b),
            dim: (0x87, 0x8f, 0x9a),
            accent: (0x0b, 0x6b, 0xd6),
            heading: [
                (0xb0, 0x2a, 0x34),
                (0x9c, 0x52, 0x12),
                (0x7d, 0x61, 0x14),
                (0x2c, 0x72, 0x2f),
                (0x18, 0x5a, 0x99),
                (0x6c, 0x34, 0x93),
            ],
            code: (0x2c, 0x72, 0x2f),
            quote: (0x55, 0x5d, 0x69),
            list: (0x0b, 0x6b, 0xd6),
            link: (0x0b, 0x6b, 0xd6),
            rule: (0xd8, 0xdc, 0xe2),
            marker: (0x9d, 0xa5, 0xb0),
            bar_fg: (0x6f, 0x77, 0x86),
            bar_bg: (0xf0, 0xf1, 0xf3),
            overlay_bg: (0xff, 0xff, 0xff),
            border: (0xd8, 0xdc, 0xe2),
        }
    }

    /// The colour a semantic role should be drawn in.
    pub fn role_color(&self, role: Role) -> Rgb {
        match role {
            Role::Text => self.fg,
            Role::Marker => self.marker,
            Role::Heading(level) => self.heading_color(level),
            Role::HeadingMarker(level) => self.heading_color(level),
            Role::Code => self.code,
            Role::Fence => self.marker,
            Role::Quote => self.quote,
            Role::List => self.list,
            Role::Link => self.link,
            Role::Url => self.dim,
            Role::Rule => self.rule,
        }
    }

    fn heading_color(&self, level: u8) -> Rgb {
        let index = usize::from(level.saturating_sub(1)).min(self.heading.len() - 1);
        self.heading[index]
    }
}
