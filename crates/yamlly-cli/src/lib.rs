pub mod app;
pub mod b64;
pub mod form;
pub mod k8s;
pub mod modals;
pub mod ui;

use std::io::{self, BufWriter, Write};
use std::time::Duration;

use crate::app::App;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    self, BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen,
};
use ratcn::crossterm::InputModes;

pub type AppTerminal = Terminal<CrosstermBackend<BufWriter<io::Stdout>>>;

/// Initialize the terminal with raw mode, alternate screen, and a buffered stdout backend
/// to consolidate rendering writes and avoid packet fragmentation over SSH.
fn init_terminal() -> io::Result<AppTerminal> {
    terminal::enable_raw_mode()?;
    let mut stdout = BufWriter::with_capacity(64 * 1024, io::stdout());
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

/// Restore the terminal by disabling raw mode and leaving the alternate screen.
fn restore_terminal(terminal: &mut AppTerminal) {
    let _ = execute!(terminal.backend_mut(), terminal::LeaveAlternateScreen);
    let _ = terminal.backend_mut().flush();
    let _ = terminal::disable_raw_mode();
}

/// Entry point shared by the `yamlly` and `ymly` CLI binaries.
pub fn run_cli() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(first) = args.first().map(String::as_str) {
        match first {
            "-h" | "--help" => {
                println!(
                    "yamlly (ymly) {} — Kubernetes Manifest Studio (TUI powered by ratcn)",
                    env!("CARGO_PKG_VERSION")
                );
                println!();
                println!("USAGE:");
                println!(
                    "  yamlly | ymly [FILE]          Launch interactive TUI (optionally opening FILE)"
                );
                println!(
                    "  yamlly | ymly --list          List all supported Kubernetes resource kinds"
                );
                println!("  yamlly | ymly --emit <KIND>   Print a starter YAML manifest for KIND");
                println!("  yamlly | ymly --version       Print version information");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("yamlly {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--list" => {
                for r in yamlly_core::get_all_resources() {
                    println!("{:<32} {}", r.kind, r.api_version);
                }
                return Ok(());
            }
            "--emit" => {
                let Some(kind) = args.get(1) else {
                    eprintln!("error: --emit requires a resource kind (e.g. Deployment)");
                    std::process::exit(1);
                };
                let form = crate::form::FormState::new(kind, None, Vec::new());
                if form.rows.is_empty() {
                    eprintln!("error: unknown resource kind '{kind}'");
                    std::process::exit(1);
                }
                print!("{}", yamlly_core::emit_yaml_owned(form.data));
                return Ok(());
            }
            _ => {}
        }
    }

    let files: Vec<String> = args.into_iter().filter(|a| !a.starts_with('-')).collect();
    run_tui(files)
}

fn run_tui(files: Vec<String>) -> io::Result<()> {
    let mut terminal = init_terminal()?;
    let _input_modes = InputModes::new().mouse().paste().enable()?;

    let mut ratcn = ui::new_runtime();
    let mut app = App::new(files.first().cloned());
    for extra_file in files.iter().skip(1) {
        app.load_file(extra_file);
    }

    let res = event_loop(&mut terminal, &mut ratcn, &mut app);

    drop(_input_modes);
    restore_terminal(&mut terminal);
    res
}

fn event_loop(
    terminal: &mut AppTerminal,
    ratcn: &mut ratcn::runtime::Ratcn<App, crate::app::Msg>,
    app: &mut App,
) -> io::Result<()> {
    while app.running {
        app.poll_background();
        app.tick_clock();

        if app.ui_dirty {
            execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
            terminal.draw(|frame| {
                ui::render(ratcn, frame, app);
            })?;
            execute!(terminal.backend_mut(), EndSynchronizedUpdate)?;
            app.ui_dirty = false;
        }

        let max_wait = Duration::from_millis(50);
        let timeout = app
            .toasts
            .time_until_next_expiry(app.clock)
            .map_or(max_wait, |t| t.min(max_wait));

        if event::poll(timeout)? {
            loop {
                let ev = event::read()?;
                ui::handle_crossterm_event(ratcn, app, ev);
                if !app.running {
                    break;
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
    }
    Ok(())
}
