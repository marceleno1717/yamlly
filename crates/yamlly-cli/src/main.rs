use std::io;
use std::time::Duration;

use ratatui::crossterm::event;
use ratcn::crossterm::InputModes;
use yamlly::app::App;
use yamlly::ui;

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(first) = args.first().map(String::as_str) {
        match first {
            "-h" | "--help" => {
                println!("yamlly (ymly) 0.0.1 — Kubernetes Manifest Studio (TUI powered by ratcn)");
                println!();
                println!("USAGE:");
                println!(
                    "  yamlly | ymly [FILE]          Launch interactive TUI (optionally opening FILE)"
                );
                println!(
                    "  yamlly | ymly --list          List all supported Kubernetes resource kinds"
                );
                println!(
                    "  yamlly | ymly --emit <KIND>   Print a starter YAML manifest for KIND"
                );
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
                let form = yamlly::form::FormState::new(kind, None, Vec::new());
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
    let mut terminal = ratatui::init();
    let _input_modes = InputModes::new().mouse().paste().enable()?;

    let mut ratcn = ui::new_runtime();
    let mut app = App::new(files.first().cloned());
    for extra_file in files.iter().skip(1) {
        app.load_file(extra_file);
    }

    let res = event_loop(&mut terminal, &mut ratcn, &mut app);

    drop(_input_modes);
    ratatui::restore();
    res
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    ratcn: &mut ratcn::runtime::Ratcn<App, yamlly::app::Msg>,
    app: &mut App,
) -> io::Result<()> {
    while app.running {
        app.poll_background();
        app.tick_clock();

        if app.ui_dirty {
            terminal.draw(|frame| {
                ui::render(ratcn, frame, app);
            })?;
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
