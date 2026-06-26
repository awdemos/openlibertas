use crate::app::App;
use crate::theme::Theme;
use openlibertas_core::commands::{command_description, SLASH_COMMANDS};
use openlibertas_core::engine::AgentModeStatus;

impl App {
    pub fn theme_next(&mut self) {
        let themes = Theme::all();
        if !themes.is_empty() {
            self.theme_selected = (self.theme_selected + 1).min(themes.len() - 1);
        }
    }

    pub fn theme_prev(&mut self) {
        self.theme_selected = self.theme_selected.saturating_sub(1);
    }

    pub fn select_theme(&mut self) {
        let themes = Theme::all();
        if let Some((_, theme)) = themes.get(self.theme_selected) {
            self.theme = *theme;
        }
        self.overlay = crate::app::Overlay::None;
    }

    pub fn open_themes_panel(&mut self) {
        let themes = Theme::all();
        self.theme_selected = themes
            .iter()
            .position(|(_, t)| *t == self.theme)
            .unwrap_or(0);
        self.overlay = crate::app::Overlay::Themes;
    }

    pub fn agent_prev(&mut self) {
        self.agent_selected = self.agent_selected.saturating_sub(1);
    }

    pub fn agent_next(&mut self) {
        let count = self.agent_personas().len().saturating_add(3);
        self.agent_selected = (self.agent_selected + 1) % count.max(1);
    }

    pub fn avatar_menu_prev(&mut self) {
        self.avatar_menu_selected = self.avatar_menu_selected.saturating_sub(1);
    }

    pub fn avatar_menu_next(&mut self) {
        self.avatar_menu_selected = (self.avatar_menu_selected + 1) % 4;
    }

    pub fn avatar_menu_select(&mut self) {
        match self.avatar_menu_selected {
            0 => {
                self.avatar_enabled = !self.avatar_enabled;
            }
            1 => {
                if let Some(avatar) = self.avatars.first_mut() {
                    avatar.anim_speed = match avatar.anim_speed as u32 {
                        120 => 240.0,
                        240 => 480.0,
                        480 => 960.0,
                        _ => 120.0,
                    };
                }
            }
            2 => {
                if let Some(avatar) = self.avatars.first_mut() {
                    avatar.velocity.0 = match (avatar.velocity.0 * 100.0) as i32 {
                        0 => 0.3,
                        30 => 0.6,
                        60 => 1.2,
                        _ => 0.0,
                    };
                }
            }
            3 => {
                if let Some(avatar) = self.avatars.first_mut() {
                    avatar.velocity.1 = match (avatar.velocity.1 * 100.0) as i32 {
                        0 => 0.15,
                        15 => 0.3,
                        30 => 0.6,
                        _ => 0.0,
                    };
                }
            }
            _ => {}
        }
    }

    pub fn select_agent_option(&mut self) {
        match self.agent_selected {
            0 => {
                self.engine.agents_mut().status =
                    if self.engine.agents_mut().status == AgentModeStatus::Disabled {
                        AgentModeStatus::Idle
                    } else {
                        AgentModeStatus::Disabled
                    };
            }
            1 => {
                self.cycle_agent_persona();
            }
            2 => {
                self.engine.agents_mut().max_iterations =
                    if self.engine.agents_mut().max_iterations >= 50 {
                        5
                    } else {
                        (self.engine.agents_mut().max_iterations + 5).min(50)
                    };
            }
            _ => {}
        }
    }

    pub fn update_command_palette(&mut self) {
        let prefix = self.engine.input_mut().buffer.to_lowercase();
        self.palette_commands = SLASH_COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(&prefix))
            .map(|cmd| (cmd.to_string(), command_description(cmd).to_string()))
            .collect();
        self.palette_selected = 0;
    }

    pub fn palette_prev(&mut self) {
        if !self.palette_commands.is_empty() {
            self.palette_selected = self.palette_selected.saturating_sub(1);
        }
    }

    pub fn palette_next(&mut self) {
        if !self.palette_commands.is_empty() {
            self.palette_selected =
                (self.palette_selected + 1).min(self.palette_commands.len() - 1);
        }
    }

    pub fn select_palette_command(&mut self) -> Option<String> {
        self.palette_commands
            .get(self.palette_selected)
            .map(|(cmd, _)| cmd.clone())
    }
}
