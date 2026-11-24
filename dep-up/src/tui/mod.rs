//! The interactive list.

pub mod app;
pub mod text;
pub mod ui;
pub mod view;

use crate::engine::{CheckState, Engine};
use crate::style::Paint;
use app::{App, Request};
use ratatui::crossterm::ExecutableCommand;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use std::time::Duration;

fn mouse(on: bool) {
    let mut out = std::io::stdout();
    let _ = if on {
        out.execute(EnableMouseCapture)
    } else {
        out.execute(DisableMouseCapture)
    };
}

/// Runs the TUI until the user quits. Returns true when a job failed.
pub fn run(mut engine: Engine, paint: Paint) -> std::io::Result<bool> {
    // The first check: every job with a check and no result from the cache.
    let waiting: Vec<usize> = (0..engine.jobs.len())
        .filter(|&j| engine.states[j].check == CheckState::Waiting)
        .collect();
    engine.enqueue_checks(&waiting);
    let mut app = App::new(engine);

    let mut terminal = ratatui::init();
    mouse(true);
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        mouse(false);
        hook(info);
    }));
    let result = event_loop(&mut terminal, &mut app);
    mouse(false);
    ratatui::restore();

    if !app.engine.idle() {
        app.engine.stop_all();
        app.engine.drain_stopped(Duration::from_secs(3));
    }
    result?;
    let (text, failed) = crate::summary::report(&app.engine, paint, false);
    print!("{text}");
    Ok(failed)
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    while !app.quit {
        app.engine.pump();
        app.engine.poll();
        app.drain();
        app.refresh();
        terminal.draw(|f| ui::draw(f, app))?;
        if let Some(Request::Sudo(batch)) = app.request.take() {
            // Leave the screen, so sudo can ask for the password in the normal terminal.
            mouse(false);
            ratatui::restore();
            println!("dep-up: sudo asks for your password for the macOS updates.");
            let ok = std::process::Command::new("sudo")
                .arg("-v")
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            *terminal = ratatui::init();
            mouse(true);
            terminal.clear()?;
            app.sudo_done(batch, ok);
            continue;
        }
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => app.on_key(k),
                Event::Mouse(m) => app.on_mouse(m),
                _ => {}
            }
        }
    }
    Ok(())
}
