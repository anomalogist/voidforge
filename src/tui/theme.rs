use ratatui::style::Color;

use crate::settings::ThemeName;

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
}

impl Theme {
    pub fn new(name: ThemeName) -> Self {
        match name {
            ThemeName::Dark => Self {
                bg: Color::Rgb(17, 19, 24),
                surface: Color::Rgb(25, 28, 34),
                surface_alt: Color::Rgb(32, 36, 43),
                border: Color::Rgb(66, 71, 80),
                text: Color::Rgb(230, 232, 238),
                muted: Color::Rgb(165, 170, 182),
                primary: Color::Rgb(168, 199, 250),
                secondary: Color::Rgb(191, 198, 220),
                accent: Color::Rgb(208, 188, 255),
                success: Color::Rgb(166, 227, 183),
                warning: Color::Rgb(245, 211, 138),
                error: Color::Rgb(242, 184, 181),
            },
            ThemeName::Light => Self {
                bg: Color::Rgb(248, 249, 255),
                surface: Color::Rgb(255, 255, 255),
                surface_alt: Color::Rgb(238, 241, 248),
                border: Color::Rgb(200, 207, 220),
                text: Color::Rgb(27, 31, 39),
                muted: Color::Rgb(95, 104, 120),
                primary: Color::Rgb(52, 93, 171),
                secondary: Color::Rgb(85, 95, 117),
                accent: Color::Rgb(101, 85, 143),
                success: Color::Rgb(27, 110, 60),
                warning: Color::Rgb(138, 91, 0),
                error: Color::Rgb(186, 26, 26),
            },
        }
    }
}
