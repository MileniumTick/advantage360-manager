//! Ratatui/Crossterm terminal interface for `adv360-config`.
//!
//! The interface is deliberately split in two:
//!
//! * [`App`] and [`Msg`] are a pure state machine. `App::handle` returns whether the interface
//!   should keep running, and `App::draw` renders it into a Ratatui frame. Neither touches a
//!   terminal, which is what makes [`run_headless`] possible.
//! * [`run`] is the thin Crossterm wiring: raw mode, the alternate screen, bracketed paste, the
//!   event loop, and a terminal that is always restored on normal exit, on error and on panic.
//!
//! Every configuration action goes through [`adv360_core::session::Session`]. This crate never
//! parses, validates, backs up or writes a configuration file itself.

pub mod app;

mod draw;

#[cfg(test)]
mod tests;

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;
use std::time::Duration;

use adv360_core::prelude::*;
use crossterm::cursor::Show;
use crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::Terminal;

pub use app::{App, Msg, Panel};

/// True while the terminal is in raw mode and on the alternate screen.
static TERMINAL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Installs the panic hook exactly once.
static PANIC_HOOK: Once = Once::new();

/// Run the terminal interface until the operator quits.
///
/// If the v-Drive cannot be opened, the core error and its recovery hints are printed together
/// with the mount instructions, then the error is returned. If it can, the terminal is always
/// restored: on a normal quit, on an event-loop error, and from the panic hook, which restores
/// and then re-raises the original panic.
pub fn run(options: SessionOptions) -> Result<(), Box<dyn std::error::Error>> {
    let session = match Session::open(options) {
        Ok(session) => session,
        Err(err) => {
            report_startup_error(&err);
            return Err(Box::new(err));
        }
    };

    enable_raw_mode()?;
    TERMINAL_ACTIVE.store(true, Ordering::SeqCst);
    let mut stdout = io::stdout();
    if let Err(err) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste) {
        restore_terminal();
        return Err(Box::new(err));
    }
    install_panic_hook();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(err) => {
            restore_terminal();
            return Err(Box::new(err));
        }
    };

    let mut app = App::new(session);
    let result = run_loop(&mut terminal, &mut app);
    restore_terminal();
    result?;
    Ok(())
}

/// Drive the state machine with a scripted message list and render every step into a Ratatui
/// [`TestBackend`], returning each frame as text.
///
/// This is how the interface is tested without a terminal. The first frame is the freshly opened
/// interface; after that there is one frame per message, so a test can assert on the exact screen
/// after each key.
pub fn run_headless(session: Session, script: &[Msg], size: (u16, u16)) -> Vec<String> {
    let mut app = App::new(session);
    let backend = TestBackend::new(size.0.max(1), size.1.max(1));
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(err) => return vec![format!("terminal error: {err}")],
    };

    let mut frames = vec![render_frame(&mut terminal, &app)];
    for message in script {
        let keep_running = app.handle(message.clone());
        frames.push(render_frame(&mut terminal, &app));
        if !keep_running {
            break;
        }
    }
    frames
}

/// The event loop.
///
/// It is deliberately not generic over the backend: it reads Crossterm events, so the only backend
/// it can ever be paired with is [`CrosstermBackend`].
fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| app.draw(frame))?;
        if !event::poll(Duration::from_millis(200))? {
            if !app.handle(Msg::Tick) {
                break;
            }
            continue;
        }
        match event::read()? {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                if !app.handle(Msg::Key(key)) {
                    break;
                }
            }
            Event::Resize(width, height) => {
                let _ = app.handle(Msg::Resize(width, height));
            }
            // Bracketed paste is enabled so pasted text cannot be interpreted as commands; the
            // interface does not consume it as keystrokes.
            Event::Paste(_) => {}
            _ => {}
        }
    }
    Ok(())
}

fn render_frame(terminal: &mut Terminal<TestBackend>, app: &App) -> String {
    if terminal.draw(|frame| app.draw(frame)).is_err() {
        return "terminal draw error".to_string();
    }
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    let mut text = String::with_capacity((area.width as usize + 1) * area.height as usize);
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

fn report_startup_error(err: &CoreError) {
    eprintln!("adv360-config: {err}");
    for hint in err.recovery_hints() {
        eprintln!("  - {hint}");
    }
    eprintln!("Mount instructions:");
    for step in device::mount_instructions() {
        eprintln!("  - {step}");
    }
}

fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            previous(info);
        }));
    });
}

/// Put the terminal back the way it was. Safe to call more than once, and from a panic hook.
fn restore_terminal() {
    if !TERMINAL_ACTIVE.swap(false, Ordering::SeqCst) {
        return;
    }
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen, DisableBracketedPaste, Show);
    let _ = io::Write::flush(&mut stdout);
}
