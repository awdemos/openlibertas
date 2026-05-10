use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Theme {
    #[default]
    Default,
    SolarizedDark,
    SolarizedLight,
    TokyoNight,
    TokyoNightStorm,
    Catppuccin,
    CatppuccinMocha,
    GruvboxDark,
    GruvboxLight,
    OneDark,
    Dracula,
    Nord,
    Monokai,
}

impl Theme {
    pub fn all() -> &'static [(&'static str, Theme)] {
        &[
            ("default", Theme::Default),
            ("solarized-dark", Theme::SolarizedDark),
            ("solarized-light", Theme::SolarizedLight),
            ("tokyo-night", Theme::TokyoNight),
            ("tokyo-night-storm", Theme::TokyoNightStorm),
            ("catppuccin", Theme::Catppuccin),
            ("catppuccin-mocha", Theme::CatppuccinMocha),
            ("gruvbox-dark", Theme::GruvboxDark),
            ("gruvbox-light", Theme::GruvboxLight),
            ("one-dark", Theme::OneDark),
            ("dracula", Theme::Dracula),
            ("nord", Theme::Nord),
            ("monokai", Theme::Monokai),
        ]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Theme::all()
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, t)| *t)
    }

    pub fn available_themes() -> Vec<&'static str> {
        Theme::all().iter().map(|(n, _)| *n).collect()
    }

    pub fn name(&self) -> &'static str {
        match self {
            Theme::Default => "default",
            Theme::SolarizedDark => "solarized-dark",
            Theme::SolarizedLight => "solarized-light",
            Theme::TokyoNight => "tokyo-night",
            Theme::TokyoNightStorm => "tokyo-night-storm",
            Theme::Catppuccin => "catppuccin",
            Theme::CatppuccinMocha => "catppuccin-mocha",
            Theme::GruvboxDark => "gruvbox-dark",
            Theme::GruvboxLight => "gruvbox-light",
            Theme::OneDark => "one-dark",
            Theme::Dracula => "dracula",
            Theme::Nord => "nord",
            Theme::Monokai => "monokai",
        }
    }

    /// Primary accent color (headers, highlights)
    pub fn primary(&self) -> Color {
        match self {
            Theme::Default => Color::Cyan,
            Theme::SolarizedDark => Color::Rgb(42, 161, 152),
            Theme::SolarizedLight => Color::Rgb(42, 161, 152),
            Theme::TokyoNight => Color::Rgb(122, 162, 247),
            Theme::TokyoNightStorm => Color::Rgb(122, 162, 247),
            Theme::Catppuccin => Color::Rgb(140, 170, 238),
            Theme::CatppuccinMocha => Color::Rgb(137, 180, 250),
            Theme::GruvboxDark => Color::Rgb(131, 165, 152),
            Theme::GruvboxLight => Color::Rgb(69, 133, 136),
            Theme::OneDark => Color::Rgb(97, 175, 239),
            Theme::Dracula => Color::Rgb(139, 233, 253),
            Theme::Nord => Color::Rgb(136, 192, 208),
            Theme::Monokai => Color::Rgb(102, 217, 239),
        }
    }

    /// Secondary color (borders, secondary highlights)
    pub fn secondary(&self) -> Color {
        match self {
            Theme::Default => Color::Yellow,
            Theme::SolarizedDark => Color::Rgb(181, 137, 0),
            Theme::SolarizedLight => Color::Rgb(181, 137, 0),
            Theme::TokyoNight => Color::Rgb(224, 175, 104),
            Theme::TokyoNightStorm => Color::Rgb(224, 175, 104),
            Theme::Catppuccin => Color::Rgb(223, 142, 29),
            Theme::CatppuccinMocha => Color::Rgb(250, 179, 135),
            Theme::GruvboxDark => Color::Rgb(215, 153, 33),
            Theme::GruvboxLight => Color::Rgb(181, 118, 20),
            Theme::OneDark => Color::Rgb(229, 192, 123),
            Theme::Dracula => Color::Rgb(241, 250, 140),
            Theme::Nord => Color::Rgb(235, 203, 139),
            Theme::Monokai => Color::Rgb(253, 151, 31),
        }
    }

    /// User message color
    pub fn user_color(&self) -> Color {
        match self {
            Theme::Default => Color::Green,
            Theme::SolarizedDark => Color::Rgb(133, 153, 0),
            Theme::SolarizedLight => Color::Rgb(133, 153, 0),
            Theme::TokyoNight => Color::Rgb(158, 206, 106),
            Theme::TokyoNightStorm => Color::Rgb(158, 206, 106),
            Theme::Catppuccin => Color::Rgb(64, 160, 43),
            Theme::CatppuccinMocha => Color::Rgb(166, 227, 161),
            Theme::GruvboxDark => Color::Rgb(184, 187, 38),
            Theme::GruvboxLight => Color::Rgb(121, 116, 14),
            Theme::OneDark => Color::Rgb(152, 195, 121),
            Theme::Dracula => Color::Rgb(80, 250, 123),
            Theme::Nord => Color::Rgb(163, 190, 140),
            Theme::Monokai => Color::Rgb(166, 226, 46),
        }
    }

    /// Assistant message color
    pub fn assistant_color(&self) -> Color {
        match self {
            Theme::Default => Color::Blue,
            Theme::SolarizedDark => Color::Rgb(38, 139, 210),
            Theme::SolarizedLight => Color::Rgb(38, 139, 210),
            Theme::TokyoNight => Color::Rgb(122, 162, 247),
            Theme::TokyoNightStorm => Color::Rgb(122, 162, 247),
            Theme::Catppuccin => Color::Rgb(114, 135, 253),
            Theme::CatppuccinMocha => Color::Rgb(137, 180, 250),
            Theme::GruvboxDark => Color::Rgb(131, 165, 152),
            Theme::GruvboxLight => Color::Rgb(69, 133, 136),
            Theme::OneDark => Color::Rgb(97, 175, 239),
            Theme::Dracula => Color::Rgb(189, 147, 249),
            Theme::Nord => Color::Rgb(129, 161, 193),
            Theme::Monokai => Color::Rgb(102, 217, 239),
        }
    }

    /// System message color
    pub fn system_color(&self) -> Color {
        match self {
            Theme::Default => Color::Gray,
            Theme::SolarizedDark => Color::Rgb(147, 161, 161),
            Theme::SolarizedLight => Color::Rgb(147, 161, 161),
            Theme::TokyoNight => Color::Rgb(86, 95, 137),
            Theme::TokyoNightStorm => Color::Rgb(86, 95, 137),
            Theme::Catppuccin => Color::Rgb(140, 143, 161),
            Theme::CatppuccinMocha => Color::Rgb(108, 112, 134),
            Theme::GruvboxDark => Color::Rgb(168, 153, 132),
            Theme::GruvboxLight => Color::Rgb(146, 131, 116),
            Theme::OneDark => Color::Rgb(92, 99, 112),
            Theme::Dracula => Color::Rgb(98, 114, 164),
            Theme::Nord => Color::Rgb(76, 86, 106),
            Theme::Monokai => Color::Rgb(117, 113, 94),
        }
    }

    /// Tool message color
    pub fn tool_color(&self) -> Color {
        match self {
            Theme::Default => Color::Magenta,
            Theme::SolarizedDark => Color::Rgb(211, 54, 130),
            Theme::SolarizedLight => Color::Rgb(211, 54, 130),
            Theme::TokyoNight => Color::Rgb(180, 249, 248),
            Theme::TokyoNightStorm => Color::Rgb(180, 249, 248),
            Theme::Catppuccin => Color::Rgb(221, 120, 120),
            Theme::CatppuccinMocha => Color::Rgb(250, 179, 135),
            Theme::GruvboxDark => Color::Rgb(211, 134, 155),
            Theme::GruvboxLight => Color::Rgb(177, 98, 134),
            Theme::OneDark => Color::Rgb(198, 120, 221),
            Theme::Dracula => Color::Rgb(255, 121, 198),
            Theme::Nord => Color::Rgb(180, 142, 173),
            Theme::Monokai => Color::Rgb(249, 38, 114),
        }
    }

    /// Error color
    pub fn error_color(&self) -> Color {
        match self {
            Theme::Default => Color::Red,
            Theme::SolarizedDark => Color::Rgb(220, 50, 47),
            Theme::SolarizedLight => Color::Rgb(220, 50, 47),
            Theme::TokyoNight => Color::Rgb(247, 118, 142),
            Theme::TokyoNightStorm => Color::Rgb(247, 118, 142),
            Theme::Catppuccin => Color::Rgb(210, 15, 57),
            Theme::CatppuccinMocha => Color::Rgb(243, 139, 168),
            Theme::GruvboxDark => Color::Rgb(204, 36, 29),
            Theme::GruvboxLight => Color::Rgb(157, 0, 6),
            Theme::OneDark => Color::Rgb(224, 108, 117),
            Theme::Dracula => Color::Rgb(255, 85, 85),
            Theme::Nord => Color::Rgb(191, 97, 106),
            Theme::Monokai => Color::Rgb(249, 38, 114),
        }
    }

    /// Background color for panels/popups
    pub fn panel_bg(&self) -> Color {
        match self {
            Theme::Default => Color::Black,
            Theme::SolarizedDark => Color::Rgb(0, 43, 54),
            Theme::SolarizedLight => Color::Rgb(253, 246, 227),
            Theme::TokyoNight => Color::Rgb(26, 27, 38),
            Theme::TokyoNightStorm => Color::Rgb(36, 40, 59),
            Theme::Catppuccin => Color::Rgb(239, 241, 245),
            Theme::CatppuccinMocha => Color::Rgb(30, 30, 46),
            Theme::GruvboxDark => Color::Rgb(40, 40, 40),
            Theme::GruvboxLight => Color::Rgb(251, 241, 199),
            Theme::OneDark => Color::Rgb(40, 44, 52),
            Theme::Dracula => Color::Rgb(40, 42, 54),
            Theme::Nord => Color::Rgb(46, 52, 64),
            Theme::Monokai => Color::Rgb(39, 40, 34),
        }
    }

    /// Foreground/text color
    pub fn foreground(&self) -> Color {
        match self {
            Theme::Default => Color::White,
            Theme::SolarizedDark => Color::Rgb(131, 148, 150),
            Theme::SolarizedLight => Color::Rgb(101, 123, 131),
            Theme::TokyoNight => Color::Rgb(169, 177, 214),
            Theme::TokyoNightStorm => Color::Rgb(169, 177, 214),
            Theme::Catppuccin => Color::Rgb(76, 79, 105),
            Theme::CatppuccinMocha => Color::Rgb(205, 214, 244),
            Theme::GruvboxDark => Color::Rgb(235, 219, 178),
            Theme::GruvboxLight => Color::Rgb(60, 56, 54),
            Theme::OneDark => Color::Rgb(171, 178, 191),
            Theme::Dracula => Color::Rgb(248, 248, 242),
            Theme::Nord => Color::Rgb(216, 222, 233),
            Theme::Monokai => Color::Rgb(248, 248, 242),
        }
    }

    /// Border color
    pub fn border_color(&self) -> Color {
        match self {
            Theme::Default => Color::Gray,
            Theme::SolarizedDark => Color::Rgb(88, 110, 117),
            Theme::SolarizedLight => Color::Rgb(131, 148, 150),
            Theme::TokyoNight => Color::Rgb(86, 95, 137),
            Theme::TokyoNightStorm => Color::Rgb(86, 95, 137),
            Theme::Catppuccin => Color::Rgb(140, 143, 161),
            Theme::CatppuccinMocha => Color::Rgb(69, 71, 90),
            Theme::GruvboxDark => Color::Rgb(102, 92, 84),
            Theme::GruvboxLight => Color::Rgb(168, 153, 132),
            Theme::OneDark => Color::Rgb(92, 99, 112),
            Theme::Dracula => Color::Rgb(68, 71, 90),
            Theme::Nord => Color::Rgb(59, 66, 82),
            Theme::Monokai => Color::Rgb(73, 72, 62),
        }
    }

    pub fn heading_color(&self, level: u8) -> Color {
        match (self, level) {
            (Theme::Default, 1) => Color::Cyan,
            (Theme::Default, 2) => Color::Yellow,
            (Theme::Default, _) => Color::White,
            (Theme::SolarizedDark, 1) => Color::Rgb(42, 161, 152),
            (Theme::SolarizedDark, 2) => Color::Rgb(181, 137, 0),
            (Theme::SolarizedDark, _) => Color::Rgb(131, 148, 150),
            (Theme::SolarizedLight, 1) => Color::Rgb(42, 161, 152),
            (Theme::SolarizedLight, 2) => Color::Rgb(181, 137, 0),
            (Theme::SolarizedLight, _) => Color::Rgb(101, 123, 131),
            (Theme::TokyoNight, 1) => Color::Rgb(122, 162, 247),
            (Theme::TokyoNight, 2) => Color::Rgb(224, 175, 104),
            (Theme::TokyoNight, _) => Color::Rgb(169, 177, 214),
            (Theme::TokyoNightStorm, 1) => Color::Rgb(122, 162, 247),
            (Theme::TokyoNightStorm, 2) => Color::Rgb(224, 175, 104),
            (Theme::TokyoNightStorm, _) => Color::Rgb(169, 177, 214),
            (Theme::Catppuccin, 1) => Color::Rgb(140, 170, 238),
            (Theme::Catppuccin, 2) => Color::Rgb(223, 142, 29),
            (Theme::Catppuccin, _) => Color::Rgb(76, 79, 105),
            (Theme::CatppuccinMocha, 1) => Color::Rgb(137, 180, 250),
            (Theme::CatppuccinMocha, 2) => Color::Rgb(250, 179, 135),
            (Theme::CatppuccinMocha, _) => Color::Rgb(205, 214, 244),
            (Theme::GruvboxDark, 1) => Color::Rgb(131, 165, 152),
            (Theme::GruvboxDark, 2) => Color::Rgb(215, 153, 33),
            (Theme::GruvboxDark, _) => Color::Rgb(235, 219, 178),
            (Theme::GruvboxLight, 1) => Color::Rgb(69, 133, 136),
            (Theme::GruvboxLight, 2) => Color::Rgb(181, 118, 20),
            (Theme::GruvboxLight, _) => Color::Rgb(60, 56, 54),
            (Theme::OneDark, 1) => Color::Rgb(97, 175, 239),
            (Theme::OneDark, 2) => Color::Rgb(229, 192, 123),
            (Theme::OneDark, _) => Color::Rgb(171, 178, 191),
            (Theme::Dracula, 1) => Color::Rgb(139, 233, 253),
            (Theme::Dracula, 2) => Color::Rgb(241, 250, 140),
            (Theme::Dracula, _) => Color::Rgb(248, 248, 242),
            (Theme::Nord, 1) => Color::Rgb(136, 192, 208),
            (Theme::Nord, 2) => Color::Rgb(235, 203, 139),
            (Theme::Nord, _) => Color::Rgb(216, 222, 233),
            (Theme::Monokai, 1) => Color::Rgb(102, 217, 239),
            (Theme::Monokai, 2) => Color::Rgb(253, 151, 31),
            (Theme::Monokai, _) => Color::Rgb(248, 248, 242),
        }
    }

    pub fn code_block_bg(&self) -> Color {
        match self {
            Theme::Default => Color::Rgb(30, 30, 30),
            Theme::SolarizedDark => Color::Rgb(7, 54, 66),
            Theme::SolarizedLight => Color::Rgb(238, 232, 213),
            Theme::TokyoNight => Color::Rgb(22, 22, 30),
            Theme::TokyoNightStorm => Color::Rgb(30, 32, 48),
            Theme::Catppuccin => Color::Rgb(204, 208, 218),
            Theme::CatppuccinMocha => Color::Rgb(24, 24, 37),
            Theme::GruvboxDark => Color::Rgb(50, 48, 47),
            Theme::GruvboxLight => Color::Rgb(235, 219, 178),
            Theme::OneDark => Color::Rgb(33, 37, 43),
            Theme::Dracula => Color::Rgb(68, 71, 90),
            Theme::Nord => Color::Rgb(59, 66, 82),
            Theme::Monokai => Color::Rgb(39, 40, 34),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_from_name() {
        assert_eq!(
            Theme::from_name("solarized-dark"),
            Some(Theme::SolarizedDark)
        );
        assert_eq!(Theme::from_name("tokyo-night"), Some(Theme::TokyoNight));
        assert_eq!(Theme::from_name("catppuccin"), Some(Theme::Catppuccin));
        assert_eq!(Theme::from_name("unknown"), None);
    }

    #[test]
    fn theme_roundtrip() {
        for (name, theme) in Theme::all() {
            assert_eq!(theme.name(), *name);
            assert_eq!(Theme::from_name(name), Some(*theme));
        }
    }
}
