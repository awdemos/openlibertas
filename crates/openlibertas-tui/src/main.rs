//! OpenLibertas TUI — terminal user interface with async event loop.

#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![cfg_attr(not(test), warn(clippy::panic))]

use anyhow::Result;
use base64::Engine;
use crossterm::event::{Event as CEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal};
use std::io::{stdout, Write};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

mod app;
mod avatar;
mod event;
mod markdown;
mod terminal;
mod theme;
mod ui;

use crate::event::{Event, EventStream};
use app::{App, Overlay, Screen, SlashCommand};
use openlibertas_core::backend::registry::BackendRegistry;
use openlibertas_core::config::Config;
use openlibertas_core::domain::ChatEvent;
use openlibertas_core::domain::{Model, ProviderId, Role};
use openlibertas_core::state::State;
use terminal::TerminalGuard;
use unicode_width::UnicodeWidthStr;

fn attach_chat_stream(
    app: &mut App,
    registry: &BackendRegistry,
    event_stream: &mut EventStream,
    messages: Vec<openlibertas_core::domain::Message>,
) {
    use openlibertas_core::engine::AgentStatus;
    let messages = if app.engine.agents_mut().status == AgentStatus::Active {
        let compacted = app.engine.chat_mut().compactor.compact(&messages);
        if compacted.len() < messages.len() {
            info!(
                "Context compacted: {} → {} messages",
                messages.len(),
                compacted.len()
            );
        }
        compacted
    } else {
        messages
    };
    let model = app.models.current.clone().unwrap_or_default();
    let max_tokens = app.config.max_tokens;
    let tools = app.engine.tools_for_request();
    app.engine.chat_mut().cancel_token = tokio_util::sync::CancellationToken::new();
    app.engine.tool_executor_mut().clear_pending_tool_calls();
    let stream_rx = registry.chat_with_fallback(
        &app.models.provider,
        model,
        messages,
        max_tokens,
        tools,
        app.engine.chat_mut().cancel_token.clone(),
        app.temperature,
    );
    event_stream.attach_chat_stream(stream_rx);
}

fn enhance_error_with_suggestion(err: &str) -> String {
    let err_lower = err.to_lowercase();
    let suggestion = if err_lower.contains("401") || err_lower.contains("403") {
        "Check your API key in ~/.config/openlibertas/config.toml"
    } else if err_lower.contains("429") {
        "Rate limited. Waiting before retry..."
    } else if err_lower.contains("404") {
        "Model not found. Try /models to see available models."
    } else if err_lower.contains("connection refused") {
        "Cannot connect to provider. Check if the service is running."
    } else if err_lower.contains("timeout") || err_lower.contains("timed out") {
        "Request timed out. Try reducing max_tokens or using a faster provider."
    } else {
        return err.to_string();
    };
    format!(
        "{}
Suggestion: {}",
        err, suggestion
    )
}

fn spawn_voice_transcription(
    api_key: Option<openlibertas_core::config::SecretString>,
    audio_bytes: Vec<u8>,
    generation: u64,
    sender: tokio::sync::mpsc::UnboundedSender<Event>,
) {
    tokio::spawn(async move {
        match openlibertas_core::voice::stt_transcribe(api_key, audio_bytes).await {
            Ok(text) => {
                let _ = sender.send(Event::VoiceTranscription(text, generation));
            }
            Err(e) => {
                let _ = sender.send(Event::VoiceError(e.to_string(), generation));
            }
        }
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    TerminalGuard::setup_panic_hook();
    let _guard = TerminalGuard::new(false)?;
    let mut mouse_captured = false;
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
        let health_sender = event_stream.sender();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                match backend_check.fetch_models().await {
                    Ok(_) => {
                        let _ = health_sender.send(Event::BackendHealthCheck(Ok(())));
                    }
                    Err(e) => {
                        let _ = health_sender.send(Event::BackendHealthCheck(Err(e.to_string())));
                    }
                }
            }
        });
    }

    app.engine.tools_mut().set_client(
        match openlibertas_core::mcp::McpClient::from_opencode_config() {
            Ok(client) => Some(std::sync::Arc::new(client)),
            Err(e) => {
                eprintln!("[MCP] Failed to initialize MCP client: {}", e);
                None
            }
        },
    );

    if let Some(client) = app.engine.tools_mut().client() {
        let sender = event_stream.sender();
        tokio::spawn(async move {
            match client.discover_tools().await {
                Ok(tools) => {
                    let statuses = client.server_statuses().await;
                    let tool_map = client.tool_server_map().await;
                    let _ = sender.send(Event::McpToolsLoaded(Ok((tools, statuses, tool_map))));
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
                Ok((generation, audio_bytes)) => {
                    if audio_bytes.len() > 44 {
                        app.pending_voice_generation = Some(generation);
                        let sender = event_stream.sender();
                        let api_key = app.voice.config.api_key.clone();
                        spawn_voice_transcription(api_key, audio_bytes, generation, sender);
                    } else {
                        app.voice_status = Some("No audio captured — check microphone".to_string());
                        app.voice.cancel();
                    }
                }
                Err(e) => {
                    app.voice_status = Some(format!("Recording failed: {}", e));
                    app.voice.cancel();
                }
            }
        }

        // Activity timeout fallback: if push-to-talk is active and no key
        // activity for 2000ms, assume the key was released and stop recording.
        // This handles terminals that don't send Release events reliably.
        if app.voice.push_to_talk_active
            && matches!(
                app.voice.state(),
                openlibertas_core::voice::VoiceState::Recording
            )
            && app
                .voice_activity_at
                .is_some_and(|t| t.elapsed().as_millis() >= 2000)
        {
            info!("Voice activity timeout: stopping recording");
            app.voice_status = None;
            match app.voice.stop_recording() {
                Ok((generation, audio_bytes)) => {
                    if audio_bytes.len() > 44 {
                        app.pending_voice_generation = Some(generation);
                        let sender = event_stream.sender();
                        let api_key = app.voice.config.api_key.clone();
                        spawn_voice_transcription(api_key, audio_bytes, generation, sender);
                    } else {
                        app.voice_status = Some("No audio captured — check microphone".to_string());
                        app.voice.cancel();
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

        if app.avatar_enabled {
            if let Ok(size) = terminal.size() {
                let area = Rect::new(0, 0, size.width, size.height);
                for avatar in &mut app.avatars {
                    avatar.set_bounds(area);
                    avatar.update(120.0);
                }
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
                            app.voice_status =
                                Some("Recording too short — hold Ctrl+Space longer".to_string());
                            app.voice.cancel();
                        } else {
                            match app.voice.stop_recording() {
                                Ok((generation, audio_bytes)) => {
                                    if audio_bytes.len() <= 44 {
                                        app.voice_status = Some(
                                            "No audio captured — check microphone".to_string(),
                                        );
                                        app.voice.cancel();
                                    } else {
                                        app.pending_voice_generation = Some(generation);
                                        let sender = event_stream.sender();
                                        let api_key = app.voice.config.api_key.clone();
                                        spawn_voice_transcription(
                                            api_key,
                                            audio_bytes,
                                            generation,
                                            sender,
                                        );
                                    }
                                }
                                Err(e) => {
                                    app.voice_status = Some(format!("Recording failed: {}", e));
                                    app.voice.cancel();
                                }
                            }
                        }
                    }
                    // Handle Esc release on model screen (some terminals only send Release)
                    if key.code == KeyCode::Esc && app.screen == Screen::Models {
                        app.screen = Screen::Chat;
                    }
                }
                Event::Input(CEvent::Key(key))
                    if key.kind == KeyEventKind::Repeat
                        && (key.code == KeyCode::Char(' ') || key.code == KeyCode::Null)
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    // Repeat events while holding Ctrl+Space keep the activity timer alive
                    // so the main-loop timeout doesn't stop recording prematurely.
                    app.voice_activity_at = Some(Instant::now());
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
                            }) && (app.last_click_pos
                                == Some((mouse.column, mouse.row)));
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
                                        let _ = nix::sys::signal::raise(
                                            nix::sys::signal::Signal::SIGTSTP,
                                        );
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

                                app.voice_activity_at = Some(Instant::now());
                                if matches!(
                                    app.voice.state(),
                                    openlibertas_core::voice::VoiceState::Recording
                                ) {
                                    debug!("Ctrl+Space pressed while already recording");
                                    continue;
                                }
                                info!("Ctrl+Space: starting recording");
                                match app.voice.start_recording(true) {
                                    Ok(generation) => {
                                        app.pending_voice_generation = Some(generation);
                                        app.voice_status =
                                            Some("Recording... 🎙  (release to stop)".to_string());
                                    }
                                    Err(e) => {
                                        warn!("Failed to start recording: {}", e);
                                        app.voice_status =
                                            Some(format!("Failed to start recording: {}", e));
                                    }
                                }
                                continue;
                            }
                            KeyCode::Char(' ') | KeyCode::Null
                                if app.screen == Screen::Chat
                                    && app.voice.is_enabled()
                                    && key.modifiers.contains(KeyModifiers::CONTROL)
                                    && key.kind == KeyEventKind::Repeat
                                    && matches!(
                                        app.voice.state(),
                                        openlibertas_core::voice::VoiceState::Recording
                                    ) =>
                            {
                                app.voice_activity_at = Some(Instant::now());
                                continue;
                            }
                            _ => {}
                        }
                    }

                    match app.screen {
                        Screen::Models => match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
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
                                    app.engine
                                        .add_system_message("[Request cancelled]".to_string());
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
                                } else if app.completion_active() {
                                    app.clear_completions();
                                } else if app.overlay != Overlay::None {
                                    if app.overlay == Overlay::Sessions {
                                        app.session_search.clear();
                                        app.session_selected = 0;
                                    }
                                    if app.overlay == Overlay::Mcp {
                                        app.mcp_show_detail = false;
                                        app.mcp_test_result = None;
                                    }
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
                            KeyCode::BackTab if app.completion_active() => {
                                app.cycle_completion_prev();
                            }
                            KeyCode::Tab if app.completion_active() => {
                                app.cycle_completion_next();
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
                                    app.engine.input_mut().cursor_pos =
                                        app.engine.input_mut().buffer.len();
                                    app.palette_next();
                                } else {
                                    app.overlay = Overlay::None;
                                }
                            }
                            KeyCode::Enter => {
                                if app.completion_active() {
                                    app.apply_completion();
                                    continue;
                                }
                                match app.overlay {
                                    Overlay::Agents => app.select_agent_option(),
                                    Overlay::AvatarMenu => app.avatar_menu_select(),
                                    Overlay::Themes => app.select_theme(),
                                    Overlay::Palette => {
                                        if let Some(cmd) = app.select_palette_command() {
                                            app.engine.input_mut().buffer = cmd;
                                            app.engine.input_mut().cursor_pos =
                                                app.engine.input_mut().buffer.len();
                                        }
                                        app.overlay = Overlay::None;
                                    }
                                    Overlay::Sessions => {
                                        if let Some(msg) = app.load_selected_session() {
                                            app.engine.add_system_message(msg);
                                        }
                                        continue;
                                    }
                                    _ => {}
                                }
                                let voice_recording = app.voice.is_enabled()
                                    && matches!(
                                        app.voice.state(),
                                        openlibertas_core::voice::VoiceState::Recording
                                    );
                                if !app.engine.input_mut().buffer.trim().is_empty()
                                    && !voice_recording
                                {
                                    if app.engine.chat_mut().streaming {
                                        if app.engine.agents_mut().status
                                            == openlibertas_core::engine::AgentStatus::Active
                                        {
                                            app.engine.chat_mut().cancel_token.cancel();
                                            app.finish_stream();
                                        } else {
                                            continue;
                                        }
                                    }
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
                                        if app.overlay == Overlay::Mcp {
                                            if let Some(client) = app.engine.tools().client() {
                                                let sender = event_stream.sender();
                                                let client_clone = client.clone();
                                                tokio::spawn(async move {
                                                    let diagnostics = client_clone.get_diagnostics().await;
                                                    let _ = sender.send(Event::McpDiagnosticsLoaded(diagnostics));
                                                    let health = client_clone.health_check().await;
                                                    let _ = sender.send(Event::McpHealthCheck(Ok(health)));
                                                });
                                            }
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
                                        let messages = app.push_user_message();
                                        let _ = app.autosave();
                                        attach_chat_stream(
                                            &mut app,
                                            &registry,
                                            &mut event_stream,
                                            messages,
                                        );
                                        app.engine.input_mut().buffer.clear();
                                        app.engine.input_mut().cursor_pos = 0;
                                        app.engine.input_mut().selection_anchor = None;
                                    }
                                }
                            }
                            KeyCode::Char('n') if app.search.active => app.search_next(),
                            KeyCode::Char('N') if app.search.active => app.search_prev(),
                            KeyCode::Char(c) => {
                                if app.overlay == Overlay::Mcp {
                                    match c {
                                        'r' | 'R' => {
                                            if let Some(client) = app.engine.tools().client() {
                                                let sender = event_stream.sender();
                                                let client_clone = client.clone();
                                                tokio::spawn(async move {
                                                    let diagnostics = client_clone.get_diagnostics().await;
                                                    let _ = sender.send(Event::McpDiagnosticsLoaded(diagnostics));
                                                    let health = client_clone.health_check().await;
                                                    let _ = sender.send(Event::McpHealthCheck(Ok(health)));
                                                });
                                            }
                                            continue;
                                        }
                                        't' | 'T' => {
                                            if let Some(tool_name) = app.selected_tool_name() {
                                                if let Some(client) = app.engine.tools().client() {
                                                    let sender = event_stream.sender();
                                                    let client_clone = client.clone();
                                                    tokio::spawn(async move {
                                                        match client_clone.test_tool(&tool_name).await {
                                                            Ok(result) => {
                                                                let _ = sender.send(Event::McpToolTest(Ok(result)));
                                                            }
                                                            Err(e) => {
                                                                let _ = sender.send(Event::McpToolTest(Err(e.to_string())));
                                                            }
                                                        }
                                                    });
                                                }
                                            }
                                            continue;
                                        }
                                        'd' | 'D' => {
                                            app.toggle_mcp_detail();
                                            continue;
                                        }
                                        _ => {}
                                    }
                                }
                                if app.overlay == Overlay::Sessions {
                                    app.session_search.push(c);
                                    app.session_selected = 0;
                                    continue;
                                }
                                if app.engine.input_mut().selection_anchor.is_some() {
                                    app.engine.delete_selection();
                                }
                                let pos = app
                                    .engine
                                    .input_mut()
                                    .cursor_pos
                                    .min(app.engine.input_mut().buffer.len());
                                app.engine.input_mut().cursor_pos = pos;
                                let cursor_pos = app.engine.input_mut().cursor_pos;
                                app.engine.input_mut().buffer.insert(cursor_pos, c);
                                app.engine.input_mut().cursor_pos += c.len_utf8();
                                app.engine.input_mut().show_autocomplete = false;
                                app.engine.input_mut().autocomplete_index = 0;
                                app.engine.input_mut().history_index = None;
                                let buf = &app.engine.input_mut().buffer;
                                if buf.starts_with('@')
                                    || (buf.starts_with("/model ") && !buf.contains("  "))
                                {
                                    app.overlay = Overlay::None;
                                    app.refresh_completions();
                                } else if buf.starts_with('/') && !buf.contains(' ') {
                                    app.overlay = Overlay::Palette;
                                    app.update_command_palette();
                                    app.refresh_completions();
                                } else {
                                    app.overlay = Overlay::None;
                                    app.clear_completions();
                                }
                            }
                            KeyCode::Backspace => {
                                if app.overlay == Overlay::Sessions {
                                    app.session_search.pop();
                                    app.session_selected = 0;
                                    continue;
                                }
                                if app.engine.input_mut().selection_anchor.is_some() {
                                    app.engine.delete_selection();
                                } else if app.engine.input_mut().cursor_pos > 0 {
                                    let pos = app
                                        .engine
                                        .input_mut()
                                        .cursor_pos
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
                                    app.refresh_completions();
                                } else {
                                    app.overlay = Overlay::None;
                                    app.clear_completions();
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
                                Overlay::AvatarMenu => app.avatar_menu_prev(),
                                Overlay::Themes => app.theme_prev(),
                                Overlay::Palette => app.palette_prev(),
                                Overlay::Sessions => app.session_prev(),
                                Overlay::Mcp => app.mcp_server_prev(),
                                _ => app.engine.history_prev(),
                            },
                            KeyCode::Down => match app.overlay {
                                Overlay::Agents => app.agent_next(),
                                Overlay::AvatarMenu => app.avatar_menu_next(),
                                Overlay::Themes => app.theme_next(),
                                Overlay::Palette => app.palette_next(),
                                Overlay::Sessions => {
                                    let count = app.filtered_sessions().len();
                                    app.session_next(count);
                                }
                                Overlay::Mcp => app.mcp_server_next(),
                                _ => app.engine.history_next(),
                            },
                            KeyCode::PageUp => app.engine.scroll_page_up(),
                            KeyCode::PageDown => app.engine.scroll_page_down(),
                            KeyCode::Delete => {
                                if app.overlay == Overlay::Sessions {
                                    if let Some(msg) = app.delete_selected_session() {
                                        app.engine.add_system_message(msg);
                                    }
                                }
                            }
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
                Event::McpToolsLoaded(Ok((tools, statuses, tool_map))) => {
                    app.engine.tools_mut().set_available_tools(tools);
                    app.engine.tools_mut().set_server_statuses(statuses);
                    app.engine.tools_mut().set_tool_server_map(tool_map);
                    if let Some(client) = app.engine.tools().client() {
                        let sender = event_stream.sender();
                        let client_clone = client.clone();
                        tokio::spawn(async move {
                            let diagnostics = client_clone.get_diagnostics().await;
                            let _ = sender.send(Event::McpDiagnosticsLoaded(diagnostics));
                        });
                    }
                }
                Event::McpToolsLoaded(Err(e)) => {
                    app.error = Some(format!("MCP discovery failed: {}", e));
                }
                Event::McpDiagnosticsLoaded(diagnostics) => {
                    app.engine.tools_mut().set_diagnostics(diagnostics);
                }
                Event::McpHealthCheck(Ok(health)) => {
                    app.mcp_health = health;
                }
                Event::McpHealthCheck(Err(e)) => {
                    app.mcp_test_result = Some(format!("Health check failed: {}", e));
                }
                Event::McpToolTest(Ok(result)) => {
                    app.mcp_test_result = Some(result);
                }
                Event::McpToolTest(Err(e)) => {
                    app.mcp_test_result = Some(format!("✗ {}", e));
                }
                Event::VoiceTranscription(text, generation) => {
                    // Discard stale transcriptions from cancelled or superseded recordings
                    if let Some(expected) = app.pending_voice_generation {
                        if generation != expected {
                            debug!(generation, expected, "Discarding stale voice transcription");
                            continue;
                        }
                    }
                    app.pending_voice_generation = None;

                    // Validate UTF-8 and filter out garbled content
                    let text = text.trim();
                    if text.is_empty()
                        || !text.is_char_boundary(0)
                        || !text.is_char_boundary(text.len())
                    {
                        app.voice_status =
                            Some("No speech detected — check mic and speak louder".to_string());
                        app.voice.cancel();
                        continue;
                    }
                    // Reject text with excessive replacement characters (indicates encoding corruption)
                    let replacement_count = text.matches('\u{FFFD}').count();
                    if replacement_count > 2 || replacement_count * 10 > text.len() {
                        app.voice_status =
                            Some("Speech garbled — try speaking more clearly".to_string());
                        app.voice.cancel();
                        continue;
                    }

                    app.voice_status = None;
                    app.voice.state = openlibertas_core::voice::VoiceState::Ready;
                    app.engine.input_mut().buffer = text.to_string();
                    app.engine.input_mut().cursor_pos = app.engine.input_mut().buffer.len();
                    app.engine.input_mut().selection_anchor = None;
                    let input = app.engine.input_mut().buffer.trim().to_string();
                    app.engine.push_to_history(input.clone());
                    if app.engine.agents_mut().status
                        == openlibertas_core::engine::AgentStatus::Idle
                    {
                        app.engine.start_agent_loop();
                    }
                    let messages = app.push_user_message();
                    let _ = app.autosave();
                    attach_chat_stream(&mut app, &registry, &mut event_stream, messages);
                }
                Event::VoiceError(err, generation) => {
                    // Discard stale errors from cancelled or superseded recordings
                    if let Some(expected) = app.pending_voice_generation {
                        if generation != expected {
                            debug!(generation, expected, "Discarding stale voice error");
                            continue;
                        }
                    }
                    app.pending_voice_generation = None;
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
                                            let _ =
                                                sender.send(Event::VoiceError(e.to_string(), 0));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = sender.send(Event::VoiceError(e.to_string(), 0));
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
                                            let _ =
                                                sender.send(Event::VoiceError(e.to_string(), 0));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = sender.send(Event::VoiceError(e.to_string(), 0));
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
                    let personas = app.agent_personas();
                    let persona_resolver =
                        |name: &str| personas.iter().find(|(n, _)| n == name).map(|(_, p)| p.clone());
                    match openlibertas_core::agent_loop::AgentLoop::run(
                        &mut app.engine,
                        &persona_resolver,
                    )
                    .await
                    {
                        openlibertas_core::agent_loop::LoopAction::Continue(tool_messages) => {
                            let _ = app.autosave();
                            let model = app.models.current.clone().unwrap_or_default();
                            let max_tokens = app.config.max_tokens;
                            let tools = app.engine.tools_for_request();

                            app.engine.chat_mut().cancel_token =
                                tokio_util::sync::CancellationToken::new();
                            let stream_rx = registry.chat_with_fallback(
                                &app.models.provider,
                                model,
                                tool_messages,
                                max_tokens,
                                tools,
                                app.engine.chat_mut().cancel_token.clone(),
                                app.temperature,
                            );
                            event_stream.attach_chat_stream(stream_rx);
                        }
                        openlibertas_core::agent_loop::LoopAction::Stop => {
                            let _ = app.autosave();
                            if app.rlm_mode {
                                let final_answer = app
                                    .engine
                                    .chat()
                                    .messages
                                    .last()
                                    .filter(|m| m.role == Role::Assistant)
                                    .and_then(|m| {
                                        let start = m.content.find("FINAL(")?;
                                        let end = m.content[start..].find(')')?;
                                        Some(m.content[start + 6..start + end].trim().to_string())
                                    });
                                if let Some(answer) = final_answer {
                                    app.engine.add_system_message(format!(
                                        "[RLM] Final answer: {}",
                                        answer
                                    ));
                                    app.rlm_mode = false;
                                    app.set_provider(app.models.provider.clone());
                                }
                            }
                            if app.voice.is_enabled() {
                                if let Some(last_msg) = app.engine.chat_mut().messages.last() {
                                    if last_msg.role == Role::Assistant
                                        && !last_msg.content.is_empty()
                                    {
                                        let text = last_msg.content.clone();
                                        if let Some(text_to_speak) = app.voice.queue_speech(text) {
                                            let sender = event_stream.sender();
                                            let api_key = app.voice.config.api_key.clone();
                                            let voice_id = app.voice.config.voice_id.clone();
                                            let cancel_flag = app.voice.cancel_flag.clone();
                                            tokio::spawn(async move {
                                                match openlibertas_core::voice::tts_synthesize(
                                                    api_key,
                                                    voice_id,
                                                    &text_to_speak,
                                                )
                                                .await
                                                {
                                                    Ok(audio_bytes) => {
                                                        match openlibertas_core::voice::play_audio_cancellable(
                                                            audio_bytes,
                                                            cancel_flag,
                                                        ) {
                                                            Ok(_) => {
                                                                let _ = sender.send(
                                                                    Event::VoicePlaybackComplete,
                                                                );
                                                            }
                                                            Err(e) => {
                                                                let _ = sender.send(
                                                                    Event::VoiceError(
                                                                        e.to_string(),
                                                                        0,
                                                                    ),
                                                                );
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        let _ = sender.send(Event::VoiceError(
                                                            e.to_string(),
                                                            0,
                                                        ));
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        openlibertas_core::agent_loop::LoopAction::Error(_) => {
                            let _ = app.autosave();
                        }
                    }
                }
                Event::ChatEvent(ChatEvent::Cancelled) => {
                    app.finish_stream();
                    app.engine.finish_agent_loop();
                    app.engine.tool_executor_mut().clear_pending_tool_calls();
                }
                Event::BackendHealthCheck(Ok(())) => {
                    app.connection_status = app::ConnectionStatus::Connected;
                }
                Event::BackendHealthCheck(Err(_e)) => {
                    app.connection_status = app::ConnectionStatus::Disconnected;
                }
                Event::ChatEvent(ChatEvent::Error(err)) => {
                    let enhanced = enhance_error_with_suggestion(&err);
                    app.finish_stream();
                    app.engine.finish_agent_loop();
                    app.engine.tool_executor_mut().clear_pending_tool_calls();
                    app.engine.add_error_message(enhanced);
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
