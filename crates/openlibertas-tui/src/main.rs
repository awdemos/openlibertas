//! OpenLibertas TUI — terminal user interface with async event loop.

#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![cfg_attr(not(test), warn(clippy::panic))]

use anyhow::Result;
use base64::Engine;
use crossterm::event::{Event as CEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{stdout, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

mod app;
mod event;
mod markdown;
mod terminal;
mod theme;
mod ui;

use crate::event::{Event, EventStream};
use app::{App, Overlay, Screen, SlashCommand};
use openlibertas_core::backend::registry::BackendRegistry;
use openlibertas_core::backend::OpenAiBackend;
use openlibertas_core::domain::ChatEvent;
use openlibertas_core::domain::Message;
use openlibertas_core::config::Config;
use openlibertas_core::domain::{Model, ProviderId, Role};
use openlibertas_core::state::State;
use terminal::TerminalGuard;
use unicode_width::UnicodeWidthStr;

fn attach_chat_stream(
    app: &mut App,
    registry: &BackendRegistry,
    event_stream: &mut EventStream,
) {
    use openlibertas_core::engine::AgentStatus;
    let messages = app.engine.chat_mut().messages.clone();
    let messages = if app.engine.agents_mut().status == AgentStatus::Active {
        let compacted = app.engine.chat_mut().compactor.compact(&messages);
        if compacted.len() < messages.len() {
            app.engine.add_system_message(format!(
                "[Context compacted: {} → {} messages]",
                messages.len(),
                compacted.len()
            ));
        }
        compacted
    } else {
        messages
    };
    let model = app.models.current.clone().unwrap_or_default();
    let max_tokens = app.config.max_tokens;
    let tools = app.engine.tools_for_request();
    let backend = registry
        .get(&app.models.provider)
        .or_else(|| registry.default_backend())
        .cloned()
        .unwrap_or_else(|| {
            Arc::new(OpenAiBackend::new(
                "".to_string(),
                openlibertas_core::config::SecretString::new("".to_string()),
            ))
        });
    app.engine.chat_mut().cancel_token = tokio_util::sync::CancellationToken::new();
    app.engine.tools_mut().clear_pending_tool_calls();
    let stream_rx = backend.chat(
        model,
        messages,
        max_tokens,
        tools,
        app.engine.chat_mut().cancel_token.clone(),
    );
    event_stream.attach_chat_stream(stream_rx);
}

fn spawn_voice_transcription(
    api_key: Option<openlibertas_core::config::SecretString>,
    audio_bytes: Vec<u8>,
    sender: tokio::sync::mpsc::UnboundedSender<Event>,
) {
    tokio::spawn(async move {
        match openlibertas_core::voice::stt_transcribe(api_key, audio_bytes).await {
            Ok(text) => {
                let _ = sender.send(Event::VoiceTranscription(text));
            }
            Err(e) => {
                let _ = sender.send(Event::VoiceError(e.to_string()));
            }
        }
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    TerminalGuard::setup_panic_hook();
    let _guard = TerminalGuard::new(true)?;
    let mut mouse_captured = true;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let config = Config::load()?;
    let mut state = State::load();
    let mut app = App::new(config.clone());

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let registry = BackendRegistry::new(&config.providers);

    let (mut event_stream, mut event_rx) = EventStream::new();

    for provider in &config.providers {
        if !provider.enabled {
            continue;
        }
        let sender = event_stream.sender();
        let provider_id = ProviderId::new(&provider.name);
        if let Some(backend) = registry.get(&provider_id) {
            let backend = backend.clone();
            tokio::spawn(async move {
                match backend.fetch_models().await {
                    Ok(mut models) => {
                        for model in &mut models {
                            model.provider = provider_id.clone();
                        }
                        let _ = sender.send(Event::ModelsLoaded(Ok(models)));
                    }
                    Err(e) => {
                        let _ = sender.send(Event::ModelsLoaded(Err(e.to_string())));
                    }
                }
            });
        }
    }

    if let Some(ref last_model) = state.last_model {
        app.models.current = Some(last_model.clone());
        app.set_provider("default");
        app.screen = Screen::Chat;
    } else if let Some(ref model) = config.model {
        app.models.current = Some(model.clone());
        app.set_provider("default");
        app.screen = Screen::Chat;
    }

    app.load_context_files();

    if let Some(backend_check) = registry.default_backend().cloned() {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let _ = backend_check.fetch_models().await;
            }
        });
    }

    app.engine.tools_mut().set_client(match openlibertas_core::mcp::McpClient::from_opencode_config() {
        Ok(client) => Some(std::sync::Arc::new(client)),
        Err(e) => {
            eprintln!("[MCP] Failed to initialize MCP client: {}", e);
            None
        }
    });

    if let Some(client) = app.engine.tools_mut().client() {
        let sender = event_stream.sender();
        tokio::spawn(async move {
            match client.discover_tools().await {
                Ok(tools) => {
                    let statuses = client.server_statuses().await;
                    let _ = sender.send(Event::McpToolsLoaded(Ok((tools, statuses))));
                }
                Err(e) => {
                    let _ = sender.send(Event::McpToolsLoaded(Err(e.to_string())));
                }
            }
        });
    }

    loop {
        app.engine.advance_spinner();

        if app.voice.is_enabled()
            && matches!(
                app.voice.state(),
                openlibertas_core::voice::VoiceState::Recording
            )
            && app.voice.has_max_recording_duration()
        {
            app.engine.add_system_message(
                "[Voice] Recording stopped — maximum duration reached.".to_string(),
            );
            app.voice_status = None;
            match app.voice.stop_recording() {
                Ok(audio_bytes) => {
                    if audio_bytes.len() > 44 {
                        let sender = event_stream.sender();
                        let api_key = app.voice.config.api_key.clone();
                        spawn_voice_transcription(api_key, audio_bytes, sender);
                    }
                }
                Err(e) => {
                    app.voice_status = Some(format!("Recording failed: {}", e));
                    app.voice.cancel();
                }
            }
        }

        if mouse_captured != app.mouse_enabled {
            mouse_captured = app.mouse_enabled;
            if mouse_captured {
                let _ = stdout().execute(crossterm::event::EnableMouseCapture);
            } else {
                let _ = stdout().execute(crossterm::event::DisableMouseCapture);
            }
        }

        terminal.draw(|f| ui::draw(f, &app))?;

        let recv_result =
            tokio::time::timeout(std::time::Duration::from_millis(120), event_rx.recv()).await;

        match recv_result {
            Ok(Some(event)) => match event {
                Event::Input(CEvent::Key(key)) if key.kind == KeyEventKind::Release => {
                    let is_space = key.code == KeyCode::Char(' ') || key.code == KeyCode::Null;
                    debug!(
                        "KeyRelease: code={:?}, is_space={}, push_to_talk={}, state={:?}",
                        key.code,
                        is_space,
                        app.voice.push_to_talk_active,
                        app.voice.state()
                    );
                    if is_space
                        && app.voice.push_to_talk_active
                        && matches!(
                            app.voice.state(),
                            openlibertas_core::voice::VoiceState::Recording
                        )
                    {
                        info!("Ctrl+Space release: stopping recording");
                        app.voice_status = None;
                        if !app.voice.has_min_recording_duration() {
                            app.voice_status = Some(
                                "Recording too short — hold Ctrl+Space longer".to_string(),
                            );
                            app.voice.cancel();
                        } else {
                            match app.voice.stop_recording() {
                                Ok(audio_bytes) => {
                                    if audio_bytes.len() <= 44 {
                                        app.voice_status = Some(
                                            "No audio captured — check microphone".to_string(),
                                        );
                                        app.voice.cancel();
                                    } else {
                                        let sender = event_stream.sender();
                                        let api_key = app.voice.config.api_key.clone();
                                        spawn_voice_transcription(api_key, audio_bytes, sender);
                                    }
                                }
                                Err(e) => {
                                    app.voice_status =
                                        Some(format!("Recording failed: {}", e));
                                    app.voice.cancel();
                                }
                            }
                        }
                    }
                }
                Event::Input(CEvent::Mouse(mouse)) => {
                    use crossterm::event::{MouseButton, MouseEventKind};
                    if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                        continue;
                    }
                    match mouse.kind {
                        MouseEventKind::ScrollUp => app.engine.scroll_page_up(),
                        MouseEventKind::ScrollDown => app.engine.scroll_page_down(),
                        MouseEventKind::Down(MouseButton::Left) if app.screen == Screen::Chat => {
                            let now = Instant::now();
                            let is_double_click = app.last_click_time.is_some_and(|t| {
                                now.duration_since(t) < Duration::from_millis(300)
                            }) && (app
                                .last_click_pos == Some((mouse.column, mouse.row)));
                            app.last_click_time = Some(now);
                            app.last_click_pos = Some((mouse.column, mouse.row));

                            let input_area_y = terminal
                                .size()
                                .map(|s| s.height.saturating_sub(3))
                                .unwrap_or(0) as u16;
                            let is_input_area = mouse.row == input_area_y
                                || mouse.row == input_area_y + 1
                                || mouse.row == input_area_y + 2;

                            if is_double_click && !is_input_area {
                                let term_size = terminal.size().unwrap_or_default();
                                let messages_area_width =
                                    term_size.width.saturating_sub(4) as usize;
                                let y_in_messages = (mouse.row.saturating_sub(3)) as usize;
                                if let Some(msg_idx) =
                                    app.engine.message_at_y(y_in_messages, messages_area_width)
                                {
                                    if let Some(msg) = app.engine.chat_mut().messages.get(msg_idx) {
                                        let text = msg.content.clone();
                                        let encoded =
                                            base64::engine::general_purpose::STANDARD.encode(&text);
                                        print!("\x1b]52;c;{}\x07", encoded);
                                        let _ = std::io::stdout().flush();
                                    }
                                }
                            } else if is_input_area {
                                let prompt_symbol = if app.voice.is_enabled() {
                                    "🎙 "
                                } else {
                                    "> "
                                };
                                let prompt_width = prompt_symbol.width();
                                app.engine
                                    .set_cursor_from_click(mouse.column as usize, prompt_width);
                                app.engine.input_mut().selection_anchor =
                                    Some(app.engine.input_mut().cursor_pos);
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) if app.screen == Screen::Chat => {
                            let input_area_y = terminal
                                .size()
                                .map(|s| s.height.saturating_sub(3))
                                .unwrap_or(0) as u16;
                            if mouse.row == input_area_y
                                || mouse.row == input_area_y + 1
                                || mouse.row == input_area_y + 2
                            {
                                let prompt_symbol = if app.voice.is_enabled() {
                                    "🎙 "
                                } else {
                                    "> "
                                };
                                let prompt_width = prompt_symbol.width();
                                app.engine
                                    .set_cursor_from_click(mouse.column as usize, prompt_width);
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) if app.screen == Screen::Chat => {
                            let anchor = app.engine.input_mut().selection_anchor;
                            let cursor = app.engine.input_mut().cursor_pos;
                            if anchor == Some(cursor) {
                                app.engine.input_mut().selection_anchor = None;
                            }
                        }
                        MouseEventKind::Down(MouseButton::Right) if app.screen == Screen::Chat => {
                            let input_area_y = terminal
                                .size()
                                .map(|s| s.height.saturating_sub(3))
                                .unwrap_or(0) as u16;
                            let is_input_area = mouse.row == input_area_y
                                || mouse.row == input_area_y + 1
                                || mouse.row == input_area_y + 2;
                            if !is_input_area {
                                let term_size = terminal.size().unwrap_or_default();
                                let messages_area_width =
                                    term_size.width.saturating_sub(4) as usize;
                                let y_in_messages = (mouse.row.saturating_sub(3)) as usize;
                                if let Some(msg_idx) =
                                    app.engine.message_at_y(y_in_messages, messages_area_width)
                                {
                                    if let Some(msg) = app.engine.chat_mut().messages.get(msg_idx) {
                                        let text = msg.content.clone();
                                        let encoded =
                                            base64::engine::general_purpose::STANDARD.encode(&text);
                                        print!("\x1b]52;c;{}\x07", encoded);
                                        let _ = std::io::stdout().flush();
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Event::Input(CEvent::Key(key)) if key.kind == KeyEventKind::Press => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match key.code {
                            KeyCode::Char('c') => {
                                if app.screen == Screen::Chat && app.engine.has_selection() {
                                    if let Some(text) = app.engine.copy_selection() {
                                        // Copy to system clipboard via OSC 52
                                        let encoded =
                                            base64::engine::general_purpose::STANDARD.encode(&text);
                                        print!("\x1b]52;c;{}\x07", encoded);
                                        let _ = std::io::stdout().flush();
                                        app.engine.clear_selection();
                                    }
                                } else {
                                    let _ = state.save();
                                    break;
                                }
                            }
                            KeyCode::Char('x') if app.screen == Screen::Chat => {
                                if let Some(text) = app.engine.cut_selection() {
                                    let encoded =
                                        base64::engine::general_purpose::STANDARD.encode(&text);
                                    print!("\x1b]52;c;{}\x07", encoded);
                                    let _ = std::io::stdout().flush();
                                }
                            }
                            KeyCode::Char('v') if app.screen == Screen::Chat => {
                                // Terminal handles paste via bracketed paste; if we get here,
                                // the terminal doesn't support it. Try OSC 52 paste request.
                                print!("\x1b]52;c;?\x07");
                                let _ = std::io::stdout().flush();
                            }
                            KeyCode::Char('a') if app.screen == Screen::Chat => {
                                app.engine.select_all();
                            }
                            KeyCode::Char('z') => {
                                #[cfg(unix)]
                                {
                                    let _ = crossterm::terminal::disable_raw_mode();
                                    let _ = std::io::stdout().execute(LeaveAlternateScreen);
                                    {
                                        let _ = nix::sys::signal::raise(nix::sys::signal::Signal::SIGTSTP);
                                    }
                                    let _ = crossterm::terminal::enable_raw_mode();
                                    let _ = std::io::stdout().execute(EnterAlternateScreen);
                                }
                                continue;
                            }
                            KeyCode::Char('w') if app.screen == Screen::Chat => {
                                app.engine.delete_word_backward();
                                continue;
                            }
                            KeyCode::Char('e') if app.screen == Screen::Chat => {
                                app.engine.move_cursor_to_end();
                                continue;
                            }
                            KeyCode::Char(' ') | KeyCode::Null
                                if app.screen == Screen::Chat
                                    && app.voice.is_enabled()
                                    && key.modifiers.contains(KeyModifiers::CONTROL)
                                    && key.kind != KeyEventKind::Repeat =>
                            {
                                if app.voice_key_debounce() {
                                    debug!("Ctrl+Space debounced");
                                    continue;
                                }
                                app.last_voice_key_at = Some(Instant::now());

                                // Intentionally no toggle-off on press: avoids terminal
                                // autorepeat stopping the recording prematurely.
                                info!("Ctrl+Space: starting recording");
                                if let Err(e) = app.voice.start_recording(true) {
                                    warn!("Failed to start recording: {}", e);
                                    app.voice_status =
                                        Some(format!("Failed to start recording: {}", e));
                                } else {
                                    app.voice_status =
                                        Some("Recording... 🎙  (release to stop)".to_string());
                                }
                                continue;
                            }
                            _ => {}
                        }
                    }

                    match app.screen {
                        Screen::Models => match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') => {
                                let _ = state.save();
                                break;
                            }
                            KeyCode::Esc => {
                                app.screen = Screen::Chat;
                            }
                            KeyCode::Down => app.select_next_model(),
                            KeyCode::Up => app.select_prev_model(),
                            KeyCode::Enter => {
                                app.select_current_model();
                                if let Some(ref model) = app.models.current {
                                    state.last_model = Some(model.clone());
                                }
                            }
                            _ => {}
                        },
                        Screen::Chat => match key.code {
                            KeyCode::Esc => {
                                if app.engine.chat_mut().streaming {
                                    app.engine.chat_mut().cancel_token.cancel();
                                    app.finish_stream();
                                    app.engine.add_system_message("[Request cancelled]".to_string());
                                } else if matches!(
                                    app.voice.state(),
                                    openlibertas_core::voice::VoiceState::Recording
                                ) {
                                    app.voice_status = Some("Recording cancelled".to_string());
                                    app.voice.cancel();
                                } else if matches!(
                                    app.voice.state(),
                                    openlibertas_core::voice::VoiceState::ProcessingTts
                                        | openlibertas_core::voice::VoiceState::Playing
                                ) {
                                    app.voice.cancel();
                                    app.voice_status = Some("Speaking cancelled".to_string());
                                } else if app.overlay != Overlay::None {
                                    app.overlay = Overlay::None;
                                } else {
                                    app.go_to_models();
                                }
                            }
                            KeyCode::F(1) | KeyCode::Char('?')
                                if app.engine.input_mut().buffer.is_empty() =>
                            {
                                app.overlay = Overlay::Help;
                            }
                            KeyCode::Tab if app.engine.input_mut().buffer.is_empty() => {
                                if app.engine.agents_mut().status
                                    == openlibertas_core::engine::AgentStatus::Disabled
                                {
                                    let personas = app.agent_personas();
                                    if let Some((name, _)) = personas.first() {
                                        app.engine.agents_mut().status =
                                            openlibertas_core::engine::AgentStatus::Idle;
                                        app.engine.agents_mut().persona = name.clone();
                                        app.engine.add_system_message(format!(
                                        "Agents enabled with '{}' persona. Press Tab to cycle, Enter to chat with agent.",
                                        name
                                    ));
                                    }
                                } else {
                                    app.cycle_agent_persona();
                                }
                            }
                            KeyCode::Tab if app.engine.input_mut().buffer.starts_with('/') => {
                                if app.overlay != Overlay::Palette {
                                    app.overlay = Overlay::Palette;
                                    app.update_command_palette();
                                }
                                if let Some(cmd) = app.select_palette_command() {
                                    app.engine.input_mut().buffer = cmd;
                                    app.engine.input_mut().cursor_pos = app.engine.input_mut().buffer.len();
                                    app.palette_next();
                                } else {
                                    app.overlay = Overlay::None;
                                }
                            }
                            KeyCode::Enter => {
                                match app.overlay {
                                    Overlay::Agents => app.select_agent_option(),
                                    Overlay::Themes => app.select_theme(),
                                    Overlay::Palette => {
                                        if let Some(cmd) = app.select_palette_command() {
                                            app.engine.input_mut().buffer = cmd;
                                            app.engine.input_mut().cursor_pos =
                                                app.engine.input_mut().buffer.len();
                                        }
                                        app.overlay = Overlay::None;
                                    }
                                    _ => {}
                                }
                                let voice_recording = app.voice.is_enabled()
                                    && matches!(
                                        app.voice.state(),
                                        openlibertas_core::voice::VoiceState::Recording
                                    );
                                if !app.engine.input_mut().buffer.trim().is_empty()
                                    && !app.engine.chat_mut().streaming
                                    && !voice_recording
                                {
                                    app.voice_status = None;
                                    app.engine.input_mut().show_autocomplete = false;
                                    let input = app.engine.input_mut().buffer.trim().to_string();
                                    app.engine.push_to_history(input.clone());

                                    if let Some(cmd) = App::parse_slash_command(&input) {
                                        let is_quit = matches!(cmd, SlashCommand::Quit);
                                        app.engine.input_mut().buffer.clear();
                                        app.engine.input_mut().cursor_pos = 0;
                                        if let Some(response) = app.execute_slash_command(cmd) {
                                            app.engine.add_system_message(response);
                                        }
                                        if is_quit {
                                            if let Some(ref model) = app.models.current {
                                                state.last_model = Some(model.clone());
                                            }
                                            let _ = state.save();
                                            break;
                                        }
                                        if let Some(ref model) = app.models.current {
                                            state.last_model = Some(model.clone());
                                            let _ = state.save();
                                        }
                                    } else {
                                        if app.engine.agents_mut().status
                                            == openlibertas_core::engine::AgentStatus::Idle
                                        {
                                            app.engine.start_agent_loop();
                                        }
                        let _ = app.push_user_message();
                        let _ = app.autosave();
                        attach_chat_stream(&mut app, &registry, &mut event_stream);
                        app.engine.input_mut().buffer.clear();
                        app.engine.input_mut().cursor_pos = 0;
                        app.engine.input_mut().selection_anchor = None;
                                    }
                                }
                            }
                            KeyCode::Char('n') if app.search.active => app.search_next(),
                            KeyCode::Char('N') if app.search.active => app.search_prev(),
                            KeyCode::Char(c) => {
                                if app.engine.input_mut().selection_anchor.is_some() {
                                    app.engine.delete_selection();
                                }
                                let pos = app.engine.input_mut().cursor_pos
                                    .min(app.engine.input_mut().buffer.len());
                                app.engine.input_mut().cursor_pos = pos;
                                let cursor_pos = app.engine.input_mut().cursor_pos;
                                app.engine.input_mut().buffer.insert(cursor_pos, c);
                                app.engine.input_mut().cursor_pos += c.len_utf8();
                                app.engine.input_mut().show_autocomplete = false;
                                app.engine.input_mut().autocomplete_index = 0;
                                app.engine.input_mut().history_index = None;
                                if app.engine.input_mut().buffer.starts_with('/')
                                    && !app.engine.input_mut().buffer.contains(' ')
                                {
                                    app.overlay = Overlay::Palette;
                                    app.update_command_palette();
                                } else {
                                    app.overlay = Overlay::None;
                                }
                            }
                            KeyCode::Backspace => {
                                if app.engine.input_mut().selection_anchor.is_some() {
                                    app.engine.delete_selection();
                                } else if app.engine.input_mut().cursor_pos > 0 {
                                    let pos = app.engine.input_mut().cursor_pos
                                        .min(app.engine.input_mut().buffer.len());
                                    let safe_pos = pos;
                                    let prev = app.engine.input_mut().buffer[..safe_pos]
                                        .char_indices()
                                        .next_back()
                                        .map(|(i, _)| i)
                                        .unwrap_or(0);
                                    app.engine.input_mut().buffer.remove(prev);
                                    app.engine.input_mut().cursor_pos = prev;
                                }
                                app.engine.input_mut().show_autocomplete = false;
                                app.engine.input_mut().autocomplete_index = 0;
                                if app.engine.input_mut().buffer.starts_with('/')
                                    && !app.engine.input_mut().buffer.contains(' ')
                                {
                                    app.overlay = Overlay::Palette;
                                    app.update_command_palette();
                                } else {
                                    app.overlay = Overlay::None;
                                }
                            }
                            KeyCode::Left
                                if key.modifiers.contains(KeyModifiers::SHIFT)
                                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                app.engine.extend_selection_left();
                                app.engine.move_cursor_word_left();
                            }
                            KeyCode::Right
                                if key.modifiers.contains(KeyModifiers::SHIFT)
                                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                app.engine.extend_selection_right();
                                app.engine.move_cursor_word_right();
                            }
                            KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.engine.extend_selection_left();
                                app.engine.move_cursor_left();
                            }
                            KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.engine.extend_selection_right();
                                app.engine.move_cursor_right();
                            }
                            KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.engine.move_cursor_word_left()
                            }
                            KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.engine.move_cursor_word_right()
                            }
                            KeyCode::Left => {
                                app.engine.clear_selection();
                                app.engine.move_cursor_left();
                            }
                            KeyCode::Right => {
                                app.engine.clear_selection();
                                app.engine.move_cursor_right();
                            }
                            KeyCode::Up => match app.overlay {
                                Overlay::Agents => app.agent_prev(),
                                Overlay::Themes => app.theme_prev(),
                                Overlay::Palette => app.palette_prev(),
                                _ => app.engine.history_prev(),
                            },
                            KeyCode::Down => match app.overlay {
                                Overlay::Agents => app.agent_next(),
                                Overlay::Themes => app.theme_next(),
                                Overlay::Palette => app.palette_next(),
                                _ => app.engine.history_next(),
                            },
                            KeyCode::PageUp => app.engine.scroll_page_up(),
                            KeyCode::PageDown => app.engine.scroll_page_down(),
                            _ => {}
                        },
                    }
                }
                Event::ModelsLoaded(Ok(models)) => {
                    let filtered_models: Vec<Model> = if config.filter_require_voice_and_tools {
                        models
                            .into_iter()
                            .filter(|m| m.supports_tools && m.supports_voice)
                            .collect()
                    } else {
                        models
                    };
                    app.models.models = filtered_models.clone();
                    app.loading = false;
                    app.connection_status = app::ConnectionStatus::Connected;

                    if let Some(ref current) = app.models.current {
                        if let Some(model) = filtered_models.iter().find(|m| &m.id == current) {
                            app.set_provider(model.provider.clone());
                        }
                    } else if let Some(ref last_model) = state.last_model {
                        if let Some(model) = filtered_models.iter().find(|m| &m.id == last_model) {
                            app.models.current = Some(last_model.clone());
                            app.set_provider(model.provider.clone());
                        }
                    } else if let Some(ref model) = config.model {
                        if let Some(m) = filtered_models.iter().find(|m| &m.id == model) {
                            app.models.current = Some(model.clone());
                            app.set_provider(m.provider.clone());
                        }
                    }
                }
                Event::ModelsLoaded(Err(e)) => {
                    app.error = Some(e);
                    app.loading = false;
                    app.connection_status = app::ConnectionStatus::Disconnected;
                }
                Event::McpToolsLoaded(Ok((tools, statuses))) => {
                    app.engine.tools_mut().set_available_tools(tools);
                    app.engine.tools_mut().set_server_statuses(statuses);
                }
                Event::McpToolsLoaded(Err(e)) => {
                    app.error = Some(format!("MCP discovery failed: {}", e));
                }
                Event::VoiceTranscription(text) => {
                    if text.trim().is_empty() {
                        app.voice_status =
                            Some("No speech detected — check mic and speak louder".to_string());
                        app.voice.cancel();
                    } else {
                        app.voice_status = None;
                        app.voice.state = openlibertas_core::voice::VoiceState::Ready;
                        app.engine.input_mut().buffer = text;
                        app.engine.input_mut().cursor_pos = app.engine.input_mut().buffer.len();
                        app.engine.input_mut().selection_anchor = None;
                        let input = app.engine.input_mut().buffer.trim().to_string();
                        app.engine.push_to_history(input.clone());
                        if app.engine.agents_mut().status == openlibertas_core::engine::AgentStatus::Idle
                        {
                            app.engine.start_agent_loop();
                        }
                        let _ = app.push_user_message();
                        let _ = app.autosave();
                        attach_chat_stream(&mut app, &registry, &mut event_stream);
                    }
                }
                Event::VoiceError(err) => {
                    app.voice_status = Some(format!("Voice error: {}", err));
                    if err.contains("MissingApiKey") || err.contains("No ElevenLabs API key") {
                        app.voice.clear_tts_queue();
                    } else if let Some(text) = app.voice.next_queued_speech() {
                        let sender = event_stream.sender();
                        let api_key = app.voice.config.api_key.clone();
                        let voice_id = app.voice.config.voice_id.clone();
                        let cancel_flag = app.voice.cancel_flag.clone();
                        tokio::spawn(async move {
                            match openlibertas_core::voice::tts_synthesize(api_key, voice_id, &text)
                                .await
                            {
                                Ok(audio_bytes) => {
                                    match openlibertas_core::voice::play_audio_cancellable(
                                        audio_bytes,
                                        cancel_flag,
                                    ) {
                                        Ok(_) => {
                                            let _ = sender.send(Event::VoicePlaybackComplete);
                                        }
                                        Err(e) => {
                                            let _ = sender.send(Event::VoiceError(e.to_string()));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = sender.send(Event::VoiceError(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Event::VoicePlaybackComplete => {
                    app.voice_status = None;
                    if let Some(text) = app.voice.next_queued_speech() {
                        let sender = event_stream.sender();
                        let api_key = app.voice.config.api_key.clone();
                        let voice_id = app.voice.config.voice_id.clone();
                        let cancel_flag = app.voice.cancel_flag.clone();
                        tokio::spawn(async move {
                            match openlibertas_core::voice::tts_synthesize(api_key, voice_id, &text)
                                .await
                            {
                                Ok(audio_bytes) => {
                                    match openlibertas_core::voice::play_audio_cancellable(
                                        audio_bytes,
                                        cancel_flag,
                                    ) {
                                        Ok(_) => {
                                            let _ = sender.send(Event::VoicePlaybackComplete);
                                        }
                                        Err(e) => {
                                            let _ = sender.send(Event::VoiceError(e.to_string()));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = sender.send(Event::VoiceError(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Event::ChatEvent(ChatEvent::Text(text)) => {
                    app.engine.append_stream_chunk(&text);
                }
                Event::ChatEvent(ChatEvent::Reasoning(text)) => {
                    app.engine.append_reasoning_chunk(&text);
                }
                Event::ChatEvent(ChatEvent::ToolCall(tool_call)) => {
                    app.engine.add_tool_call(tool_call);
                }
                Event::ChatEvent(ChatEvent::Done) => {
                    app.finish_stream();
                    app.engine.increment_agent_iteration();

                    if app.engine.agent_iteration_exceeded() {
                        app.engine.finish_agent_loop();
                        let max_iterations = app.engine.agents_mut().max_iterations;
                        app.engine.add_system_message(format!(
                        "[Agent stopped after {} iterations. Provide more specific instructions if needed.]",
                        max_iterations
                    ));
                        app.engine.tools_mut().clear_pending_tool_calls();
                    } else if app.engine.tools_mut().has_pending_tool_calls() {
                        let results = app.engine.execute_pending_tools().await;

                        for result in &results {
                            match result {
                                openlibertas_core::domain::ToolExecutionResult::Success {
                                    tool_name,
                                    ..
                                } => {
                                    if tool_name != "switch_persona" {
                                        app.engine.add_system_message(format!(
                                            "[Tool: {} executed successfully]",
                                            tool_name
                                        ));
                                    }
                                }
                                openlibertas_core::domain::ToolExecutionResult::Error {
                                    tool_name,
                                    error,
                                    ..
                                } => {
                                    app.engine.add_system_message(format!(
                                        "[Tool: {} failed: {}]",
                                        tool_name, error
                                    ));
                                }
                                openlibertas_core::domain::ToolExecutionResult::Skipped {
                                    tool_name,
                                    reason,
                                } => {
                                    app.engine.add_system_message(format!(
                                        "[Tool: {} skipped: {}]",
                                        tool_name, reason
                                    ));
                                }
                            }
                        }

                        let switch_requests: Vec<String> = app.engine.tools_mut().pending_tool_calls().iter().filter_map(|tc| {
                            if tc.function.name == "switch_persona" {
                                serde_json::from_str::<serde_json::Value>(&tc.function.arguments).ok()
                                    .and_then(|args| args.get("persona").and_then(|v| v.as_str()).map(String::from))
                            } else {
                                None
                            }
                        }).collect();

                        for persona in switch_requests {
                            let current_persona = app.engine.agents_mut().persona.clone();
                            if persona.eq_ignore_ascii_case(&current_persona) {
                                continue;
                            }
                            if let Some(prompt) = app.get_persona_prompt(&persona) {
                                app.switch_agent_persona(&persona, &prompt);
                            } else {
                                app.engine.add_system_message(format!(
                                    "[Agent: persona '{}' not found, staying as '{}']",
                                    persona, current_persona
                                ));
                            }
                        }

                        let tool_messages = app.engine.assemble_tool_result_messages();

                        let tool_messages = if app.engine.agents_mut().status
                            == openlibertas_core::engine::AgentStatus::Active
                        {
                            let compacted = app.engine.chat_mut().compactor.compact(&tool_messages);
                            if compacted.len() < tool_messages.len() {
                                app.engine.add_system_message(format!(
                                    "[Context compacted: {} → {} messages]",
                                    tool_messages.len(),
                                    compacted.len()
                                ));
                            }
                            compacted
                        } else {
                            tool_messages
                        };

                        let model = app.models.current.clone().unwrap_or_default();
                        let max_tokens = app.config.max_tokens;
                        let tools = app.engine.tools_for_request();

                        app.engine.chat_mut().messages.push(Message { role: Role::Assistant, content: String::new(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None });
                        app.engine.chat_mut().streaming = true;
                        app.engine.tools_mut().clear_pending_tool_calls();

                        let backend = registry
                            .get(&app.models.provider)
                            .or_else(|| registry.default_backend())
                            .cloned()
                            .unwrap_or_else(|| {
                                Arc::new(OpenAiBackend::new(
                                    "".to_string(),
                                    openlibertas_core::config::SecretString::new("".to_string()),
                                ))
                            });
                        app.engine.chat_mut().cancel_token = tokio_util::sync::CancellationToken::new();
                        let stream_rx = backend.chat(
                            model,
                            tool_messages,
                            max_tokens,
                            tools,
        app.engine.chat_mut().cancel_token.clone(),
                        );
                        event_stream.attach_chat_stream(stream_rx);
                    } else {
                        app.engine.finish_agent_loop();
                        app.engine.tools_mut().clear_pending_tool_calls();

                        if app.voice.is_enabled() {
                            if let Some(last_msg) = app.engine.chat_mut().messages.last() {
                                if last_msg.role == Role::Assistant && !last_msg.content.is_empty()
                                {
                                    let text = last_msg.content.clone();
                                    if let Some(text_to_speak) = app.voice.queue_speech(text) {
                                        let sender = event_stream.sender();
                                        let api_key = app.voice.config.api_key.clone();
                                        let voice_id = app.voice.config.voice_id.clone();
                                        let cancel_flag = app.voice.cancel_flag.clone();
                                        tokio::spawn(async move {
                                            match openlibertas_core::voice::tts_synthesize(
                                                api_key, voice_id, &text_to_speak,
                                            )
                                            .await
                                            {
                                                Ok(audio_bytes) => {
                                                    match openlibertas_core::voice::play_audio_cancellable(
                                                        audio_bytes, cancel_flag,
                                                    )
                                                    {
                                                        Ok(_) => {
                                                            let _ = sender
                                                                .send(Event::VoicePlaybackComplete);
                                                        }
                                                        Err(e) => {
                                                            let _ = sender
                                                                .send(Event::VoiceError(e.to_string()));
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    let _ =
                                                        sender.send(Event::VoiceError(e.to_string()));
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                Event::ChatEvent(ChatEvent::Cancelled) => {
                    app.finish_stream();
                    app.engine.finish_agent_loop();
                    app.engine.tools_mut().clear_pending_tool_calls();
                }
                Event::ChatEvent(ChatEvent::Error(err)) => {
                    app.finish_stream();
                    app.engine.finish_agent_loop();
                    app.engine.tools_mut().clear_pending_tool_calls();
                    app.engine.add_error_message(err);
                }
                Event::Input(CEvent::Resize(_, _)) => {}
                Event::Input(_) => {}
            },
            Ok(None) => break,
            Err(_) => {}
        }
    }

    Ok(())
}
