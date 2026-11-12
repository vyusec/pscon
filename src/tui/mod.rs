mod app;
mod input;
mod scan_events;
#[cfg(test)]
mod tests;
mod ui;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::core::{
    AppEvent, Asset, BridgeEvent, Config, DecisionRecord, Endpoint, EventBus, ScanEvent,
    ScanRecord, ScanSnapshot, SecretFinding, Store, Verification,
};
use crate::engine::{BridgeHandle, ScanEngine, ScanOptions};
use anyhow::Result;
use app::*;
use input::Input;
use ratatui::crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
};
use tokio::sync::mpsc;
use url::Url;
use uuid::Uuid;

/// Runs the interactive console, using the existing scan engine and bridge.
pub async fn run(
    engine: Arc<ScanEngine>,
    store: Store,
    events: EventBus,
    config: Config,
    target: Option<Url>,
    bridge: Option<BridgeHandle>,
) -> Result<()> {
    let mut app = App::new(engine, store, events.clone(), config, target);
    if let Some(bridge) = &bridge {
        app.bridge_status = format!("listening on {}", bridge.addr);
        app.bridge_connected_flag = Some(bridge.connected.clone());
    }
    let mut terminal = ratatui::init();
    // This guard also runs on setup errors and unwinding.
    let _guard = TerminalGuard;
    execute!(std::io::stdout(), EnableBracketedPaste)?;
    run_loop(&mut terminal, &mut app).await
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), DisableBracketedPaste);
        ratatui::restore();
    }
}

async fn run_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    let mut events_rx = app.events.subscribe();
    let (key_tx, mut key_rx) = mpsc::unbounded_channel();
    let stopped = Arc::new(AtomicBool::new(false));
    struct ReaderGuard(Arc<AtomicBool>);
    impl Drop for ReaderGuard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let _reader_guard = ReaderGuard(stopped.clone());
    std::thread::spawn(move || {
        while !stopped.load(Ordering::Relaxed) {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                match event::read() {
                    Ok(ev) => {
                        if key_tx.send(ev).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    });
    let mut tick = tokio::time::interval(Duration::from_millis(80));
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;
        tokio::select! {
            Some(event) = key_rx.recv() => match event {
                Event::Key(key) => app.on_key(key),
                Event::Paste(text) => app.on_paste(&text),
                _ => {},
            },
            event = events_rx.recv() => match event {
                Ok(event) => app.on_event(event),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {},
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            _ = tick.tick() => {
                while let Ok(event) = events_rx.try_recv() { app.on_event(event); }
                app.poll_tasks().await;
                if let Some(flag) = &app.bridge_connected_flag {
                    app.bridge_connected = flag.load(Ordering::SeqCst);
                }
            },
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}
