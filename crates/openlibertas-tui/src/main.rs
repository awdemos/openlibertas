use anyhow::Result;
use crossterm::event::{Event as CEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io::stdout;
use std::sync::Arc;

mod app;
mod event;
mod markdown;
mod terminal;
mod theme;
mod ui;

use app::{App, Screen, SlashCommand};
use openlibertas_core::backend::registry::BackendRegistry;
use openlibertas_core::backend::{ChatEvent, Message, OpenAiBackend};
use openlibertas_core::domain::{ProviderId, Role};
use openlibertas_core::config::Config;
use openlibertas_core::state::State;
use crate::event::{Event, EventStream};
use terminal::TerminalGuard;

#[tokio::main]
async fn main() -> Result<()> {
    TerminalGuard::setup_panic_hook();
    let _guard = TerminalGuard::new()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let config = Config::load()?;
    let mut state = State::load();
    let mut app = App::new(config.clone());
    let registry = BackendRegistry::new(&config.providers);

    let (event_stream, mut event_rx) = EventStream::new();

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

    app.mcp.client = openlibertas_core::mcp::McpClient::from_opencode_config().ok().map(std::sync::Arc::new);

    if let Some(client) = app.mcp.client.clone() {
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
        app.advance_spinner();
        terminal.draw(|f| ui::draw(f, &app))?;

        let recv_result = tokio::time::timeout(
            std::time::Duration::from_millis(120),
            event_rx.recv(),
        )
        .await;

        match recv_result {
            Ok(Some(event)) => match event {
                Event::Input(CEvent::Key(key)) if key.kind == KeyEventKind::Press => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('c') => {
                            let _ = state.save();
                            break;
                        }
                        KeyCode::Char('z') => {
                            #[cfg(unix)]
                            {
                                let _ = crossterm::terminal::disable_raw_mode();
                                let _ = std::io::stdout().execute(LeaveAlternateScreen);
                                unsafe { libc::raise(libc::SIGTSTP); }
                                let _ = crossterm::terminal::enable_raw_mode();
                                let _ = std::io::stdout().execute(EnterAlternateScreen);
                            }
                            continue;
                        }
                        KeyCode::Char('w') if app.screen == Screen::Chat => {
                            app.delete_word_backward();
                            continue;
                        }
                        KeyCode::Char('a') if app.screen == Screen::Chat => {
                            app.move_cursor_to_start();
                            continue;
                        }
                        KeyCode::Char('e') if app.screen == Screen::Chat => {
                            app.move_cursor_to_end();
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
                            if app.chat.streaming {
                                app.chat.cancel_token.cancel();
                                app.finish_stream();
                                app.add_system_message("[Request cancelled]".to_string());
                            } else if app.panels.show_agents {
                                app.panels.show_agents = false;
                            } else if app.panels.show_help {
                                app.panels.show_help = false;
                            } else if app.panels.show_themes {
                                app.panels.show_themes = false;
                            } else if app.panels.show_palette {
                                app.panels.show_palette = false;
                            } else if app.panels.show_tools || app.panels.show_mcp || app.panels.show_sessions {
                                app.panels.show_tools = false;
                                app.panels.show_mcp = false;
                                app.panels.show_sessions = false;
                            } else {
                                app.go_to_models();
                            }
                        }
                        KeyCode::F(1) | KeyCode::Char('?') if app.input.buffer.is_empty() => {
                            app.panels.show_help = true;
                        }
                        KeyCode::Tab if app.input.buffer.is_empty() => {
                            if app.agents.status == app::AgentStatus::Disabled {
                                let personas = app.agent_personas();
                                if let Some((name, _)) = personas.first() {
                                    app.agents.status = app::AgentStatus::Idle;
                                    app.agents.persona = name.clone();
                                    app.add_system_message(format!(
                                        "Agents enabled with '{}' persona. Press Tab to cycle, Enter to chat with agent.",
                                        name
                                    ));
                                }
                            } else {
                                app.cycle_agent_persona();
                            }
                        }
                        KeyCode::Tab if app.input.buffer.starts_with('/') => {
                            if !app.panels.show_palette {
                                app.panels.show_palette = true;
                                app.update_command_palette();
                            }
                            if let Some(cmd) = app.select_palette_command() {
                                app.input.buffer = cmd;
                                app.input.cursor_pos = app.input.buffer.len();
                                app.palette_next();
                            } else {
                                app.panels.show_palette = false;
                            }
                        }
                        KeyCode::Enter => {
                            if app.panels.show_agents {
                                app.select_agent_option();
                            } else if app.panels.show_themes {
                                app.select_theme();
                            } else if app.panels.show_palette {
                                if let Some(cmd) = app.select_palette_command() {
                                    app.input.buffer = cmd;
                                    app.input.cursor_pos = app.input.buffer.len();
                                    app.panels.show_palette = false;
                                } else {
                                    app.panels.show_palette = false;
                                }
                            }
                            if !app.input.buffer.trim().is_empty() && !app.chat.streaming {
                                app.input.show_autocomplete = false;
                                let input = app.input.buffer.trim().to_string();
                                app.push_to_history(input.clone());

                                if let Some(cmd) = App::parse_slash_command(&input) {
                                    let is_quit = matches!(cmd, SlashCommand::Quit);
                                    app.input.buffer.clear();
                                    app.input.cursor_pos = 0;
                                    if let Some(response) = app.execute_slash_command(cmd) {
                                        app.add_system_message(response);
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
                                    if app.agents.status == app::AgentStatus::Idle {
                                        app.start_agent_loop();
                                    }
                                    let messages = app.push_user_message();

                                    let messages = if app.agents.status == app::AgentStatus::Active {
                                        let compacted = app.chat.compactor.compact(&messages);
                                        if compacted.len() < messages.len() {
                                            app.add_system_message(format!(
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
                                    let tools = app.get_tools_for_request();
                                    let backend = registry.get(&app.models.provider)
                                        .or_else(|| registry.default_backend())
                                        .cloned()
                                        .unwrap_or_else(|| Arc::new(OpenAiBackend::new("".to_string(), "".to_string())));
                                    app.chat.cancel_token = tokio_util::sync::CancellationToken::new();
                                    app.mcp.pending_tool_calls.clear();
                                    let stream_rx = backend.chat(model, messages, max_tokens, tools, app.chat.cancel_token.clone());
                                    event_stream.attach_chat_stream(stream_rx);
                                }
                            }
                        }
                        KeyCode::Char('n') if app.search.active => app.search_next(),
                        KeyCode::Char('N') if app.search.active => app.search_prev(),
                        KeyCode::Char(c) => {
                            let pos = app.input.cursor_pos.min(app.input.buffer.len());
                            app.input.cursor_pos = if app.input.buffer.is_char_boundary(pos) {
                                pos
                            } else {
                                app.input.buffer.char_indices()
                                    .map(|(i, _)| i)
                                    .take_while(|&i| i < pos)
                                    .last()
                                    .unwrap_or(0)
                            };
                            app.input.buffer.insert(app.input.cursor_pos, c);
                            app.input.cursor_pos += c.len_utf8();
                            app.input.show_autocomplete = false;
                            app.input.autocomplete_index = 0;
                            app.input.history_index = None;
                            if app.input.buffer.starts_with('/') && !app.input.buffer.contains(' ') {
                                app.panels.show_palette = true;
                                app.update_command_palette();
                            } else {
                                app.panels.show_palette = false;
                            }
                        }
                        KeyCode::Backspace => {
                            if app.input.cursor_pos > 0 {
                                let pos = app.input.cursor_pos.min(app.input.buffer.len());
                                let safe_pos = if app.input.buffer.is_char_boundary(pos) {
                                    pos
                                } else {
                                    app.input.buffer.char_indices()
                                        .map(|(i, _)| i)
                                        .take_while(|&i| i < pos)
                                        .last()
                                        .unwrap_or(0)
                                };
                                let prev = app.input.buffer[..safe_pos]
                                    .char_indices()
                                    .next_back()
                                    .map(|(i, _)| i)
                                    .unwrap_or(0);
                                app.input.buffer.remove(prev);
                                app.input.cursor_pos = prev;
                            }
                            app.input.show_autocomplete = false;
                            app.input.autocomplete_index = 0;
                            if app.input.buffer.starts_with('/') && !app.input.buffer.contains(' ') {
                                app.panels.show_palette = true;
                                app.update_command_palette();
                            } else {
                                app.panels.show_palette = false;
                            }
                        }
                        KeyCode::Left => app.move_cursor_left(),
                        KeyCode::Right => app.move_cursor_right(),
                        KeyCode::Up => {
                            if app.panels.show_agents {
                                app.agent_prev();
                            } else if app.panels.show_themes {
                                app.theme_prev();
                            } else if app.panels.show_palette {
                                app.palette_prev();
                            } else {
                                app.history_prev();
                            }
                        }
                        KeyCode::Down => {
                            if app.panels.show_agents {
                                app.agent_next();
                            } else if app.panels.show_themes {
                                app.theme_next();
                            } else if app.panels.show_palette {
                                app.palette_next();
                            } else {
                                app.history_next();
                            }
                        }
                        KeyCode::PageUp => app.scroll_page_up(),
                        KeyCode::PageDown => app.scroll_page_down(),
                        _ => {}
                    },
                }
            }
            Event::ModelsLoaded(Ok(models)) => {
                app.models.models = models.clone();
                app.loading = false;
                app.connection_status = app::ConnectionStatus::Connected;

                if let Some(ref current) = app.models.current {
                    if let Some(model) = models.iter().find(|m| &m.id == current) {
                        app.set_provider(model.provider.clone());
                    }
                } else if let Some(ref last_model) = state.last_model {
                    if let Some(model) = models.iter().find(|m| &m.id == last_model) {
                        app.models.current = Some(last_model.clone());
                        app.set_provider(model.provider.clone());
                    }
                } else if let Some(ref model) = config.model {
                    if let Some(m) = models.iter().find(|m| &m.id == model) {
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
                app.mcp.available_tools = tools;
                app.mcp.server_statuses = statuses;
            }
            Event::McpToolsLoaded(Err(e)) => {
                app.error = Some(format!("MCP discovery failed: {}", e));
            }
            Event::ChatEvent(ChatEvent::Text(text)) => {
                app.append_stream_chunk(&text);
            }
            Event::ChatEvent(ChatEvent::ToolCall(tool_call)) => {
                app.add_tool_call(tool_call);
            }
            Event::ChatEvent(ChatEvent::Done) => {
                app.finish_stream();
                app.increment_agent_iteration();

                if app.agent_iteration_exceeded() {
                    app.finish_agent_loop();
                    app.add_system_message(format!(
                        "[Agent stopped after {} iterations. Provide more specific instructions if needed.]",
                        app.agents.max_iterations
                    ));
                    app.mcp.pending_tool_calls.clear();
                } else if !app.mcp.pending_tool_calls.is_empty() {
                    let mut results: Vec<openlibertas_core::domain::ToolExecutionResult> = Vec::new();
                    for tool_call in &app.mcp.pending_tool_calls {
                        let key_arg = App::extract_key_argument(&tool_call.function.name, &tool_call.function.arguments);
                        
                        if !app.agents.yolo_mode && App::tool_needs_approval(&tool_call.function.name) {
                            results.push(openlibertas_core::domain::ToolExecutionResult::Skipped {
                                tool_name: tool_call.function.name.clone(),
                                reason: format!("Approval required for '{}'. Enable YOLO mode (/yolo) to skip confirmations.", tool_call.function.name),
                            });
                            continue;
                        }
                        
                        let result = if let Some(ref client) = app.mcp.client {
                            match serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
                                Ok(args) => {
                                    match client.call_tool(&tool_call.function.name, args).await {
                                        Ok(output) => {
                                            let text = output
                                                .content
                                                .into_iter()
                                                .map(|c| c.text)
                                                .collect::<Vec<_>>()
                                                .join("\n");
                                            openlibertas_core::domain::ToolExecutionResult::Success {
                                                tool_name: tool_call.function.name.clone(),
                                                key_arg: key_arg.clone(),
                                                output: text,
                                            }
                                        }
                                        Err(e) => openlibertas_core::domain::ToolExecutionResult::Error {
                                            tool_name: tool_call.function.name.clone(),
                                            key_arg: key_arg.clone(),
                                            error: e.to_string(),
                                        },
                                    }
                                }
                                Err(e) => openlibertas_core::domain::ToolExecutionResult::Error {
                                    tool_name: tool_call.function.name.clone(),
                                    key_arg: key_arg.clone(),
                                    error: format!("Parse error: {}", e),
                                },
                            }
                        } else {
                            openlibertas_core::domain::ToolExecutionResult::Error {
                                tool_name: tool_call.function.name.clone(),
                                key_arg: key_arg.clone(),
                                error: "MCP client not available".to_string(),
                            }
                        };
                        results.push(result);
                    }

                    app.mcp.tool_results = results.iter().map(|r| r.content_for_message()).collect();

                    for (i, result) in results.iter().enumerate() {
                        if let Some(tool_call) = app.mcp.pending_tool_calls.get(i) {
                            app.chat.messages.push(Message {
                                role: Role::Tool,
                                content: result.content_for_message(),
                                tool_calls: None,
                                tool_call_id: Some(tool_call.id.clone()),
                            });
                        }
                    }

                    let tool_messages = app.build_tool_result_messages();

                    let tool_messages = if app.agents.status == app::AgentStatus::Active {
                        let compacted = app.chat.compactor.compact(&tool_messages);
                        if compacted.len() < tool_messages.len() {
                            app.add_system_message(format!(
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
                    let tools = app.get_tools_for_request();

                    app.chat.messages.push(Message {
                        role: Role::Assistant,
                        content: String::new(),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                    app.chat.streaming = true;
                    app.mcp.pending_tool_calls.clear();

                    let backend = registry.get(&app.models.provider)
                        .or_else(|| registry.default_backend())
                        .cloned()
                        .unwrap_or_else(|| Arc::new(OpenAiBackend::new("".to_string(), "".to_string())));
                    app.chat.cancel_token = tokio_util::sync::CancellationToken::new();
                    let stream_rx = backend.chat(model, tool_messages, max_tokens, tools, app.chat.cancel_token.clone());
                    event_stream.attach_chat_stream(stream_rx);
                } else {
                    app.finish_agent_loop();
                    app.mcp.pending_tool_calls.clear();
                }
            }
            Event::ChatEvent(ChatEvent::Cancelled) => {
                app.finish_stream();
                app.finish_agent_loop();
                app.mcp.pending_tool_calls.clear();
            }
            Event::ChatEvent(ChatEvent::Error(err)) => {
                app.finish_stream();
                app.finish_agent_loop();
                app.mcp.pending_tool_calls.clear();
                app.add_error_message(err);
            }
            Event::Input(CEvent::Resize(_, _)) => {}
            Event::Input(_) => {}
        }
        Ok(None) => break,
        Err(_) => {}
    }
    }

    Ok(())
}