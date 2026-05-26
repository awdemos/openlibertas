use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use std::io::{self, stdout, Write};

pub struct TerminalGuard {
    mouse_enabled: bool,
}

impl TerminalGuard {
    pub fn new(mouse_enabled: bool) -> io::Result<Self> {
        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        let _ = stdout().execute(PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES,
        ));
        if mouse_enabled {
            let _ = stdout().execute(EnableMouseCapture);
        }
        Ok(Self { mouse_enabled })
    }

    pub fn setup_panic_hook() {
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = stdout().execute(LeaveAlternateScreen);
            original_hook(info);
        }));
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.mouse_enabled {
            let _ = stdout().execute(DisableMouseCapture);
        }
        let _ = stdout().execute(PopKeyboardEnhancementFlags);
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
    }
}

fn enable_raw_mode() -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()
}

pub fn restore_normal_terminal() -> io::Result<()> {
    let _ = stdout().execute(PopKeyboardEnhancementFlags);
    let _ = stdout().execute(DisableMouseCapture);
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    stdout().flush()?;
    Ok(())
}

pub fn init_terminal(mouse_enabled: bool) -> io::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let _ = stdout().execute(PushKeyboardEnhancementFlags(
        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KeyboardEnhancementFlags::REPORT_EVENT_TYPES,
    ));
    if mouse_enabled {
        let _ = stdout().execute(EnableMouseCapture);
    }
    stdout().flush()?;
    Ok(())
}
