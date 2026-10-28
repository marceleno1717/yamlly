//! Generates `assets/demo.svg` (an animated SVG terminal demo of `yamlly`)
//! and `assets/screenshot.svg` (a static hero screenshot) directly from
//! `ratatui::backend::TestBackend` buffers, using true vector paths for all
//! box-drawing borders and exact cell-locked `textLength` for text runs.
//!
//! Run with:
//!   cargo run --example generate_demo

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use ratcn::InputState;
use ratcn::runtime::{Event, KeyCode, KeyEvent, Ratcn};
use yamlly::app::{App, Msg};
use yamlly::modals::FIELD_PICKER_GROUP_PREFIX;
use yamlly::ui;

const TERM_W: u16 = 140;
const TERM_H: u16 = 36;
const CELL_W: f64 = 11.0;
const CELL_H: f64 = 22.0;
const PAD_X: f64 = 14.0;
const PAD_TOP: f64 = 46.0;
const PAD_BOTTOM: f64 = 14.0;

struct Scene {
    caption: &'static str,
    buf: Buffer,
}

fn capture(ratcn: &mut Ratcn<App, Msg>, app: &mut App) -> Buffer {
    app.tick_clock();
    let backend = TestBackend::new(TERM_W, TERM_H);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| {
            ui::render(ratcn, frame, app);
        })
        .expect("draw");
    terminal.backend().buffer().clone()
}

fn send_key(ratcn: &mut Ratcn<App, Msg>, app: &mut App, code: KeyCode) {
    ui::handle_event(ratcn, app, Event::Key(KeyEvent::new(code)));
}

fn send_text(ratcn: &mut Ratcn<App, Msg>, app: &mut App, text: &str) {
    for ch in text.chars() {
        send_key(ratcn, app, KeyCode::Char(ch));
    }
}

fn color_to_hex(color: Color, default_hex: &str) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{:02x}{:02x}{:02x}", r, g, b),
        Color::Black => "#000000".to_string(),
        Color::Red => "#ef4444".to_string(),
        Color::Green => "#22c55e".to_string(),
        Color::Yellow => "#eab308".to_string(),
        Color::Blue => "#3b82f6".to_string(),
        Color::Magenta => "#a855f7".to_string(),
        Color::Cyan => "#06b6d4".to_string(),
        Color::Gray => "#9ca3af".to_string(),
        Color::DarkGray => "#4b5563".to_string(),
        Color::LightRed => "#f87171".to_string(),
        Color::LightGreen => "#4ade80".to_string(),
        Color::LightYellow => "#facc15".to_string(),
        Color::LightBlue => "#60a5fa".to_string(),
        Color::LightMagenta => "#c084fc".to_string(),
        Color::LightCyan => "#22d3ee".to_string(),
        Color::White => "#f9fafb".to_string(),
        _ => default_hex.to_string(),
    }
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            ' ' => out.push_str("&#160;"),
            _ => out.push(ch),
        }
    }
    out
}

fn is_box_or_block_char(sym: &str) -> bool {
    matches!(
        sym,
        "─" | "━"
            | "│"
            | "┃"
            | "╭"
            | "┌"
            | "╮"
            | "┐"
            | "╰"
            | "└"
            | "╯"
            | "┘"
            | "├"
            | "┤"
            | "┬"
            | "┴"
            | "┼"
            | "╴"
            | "╶"
            | "╸"
            | "╺"
            | "█"
            | "▌"
            | "▐"
    )
}

fn render_box_char_svg(out: &mut String, sym: &str, x: f64, y: f64, fg: &str) {
    let cx = x + CELL_W / 2.0;
    let cy = y + CELL_H / 2.0;
    let x2 = x + CELL_W;
    let y2 = y + CELL_H;
    let r = 4.0;
    let sw = if matches!(sym, "━" | "┃" | "╸" | "╺") {
        2.4
    } else {
        1.4
    };

    match sym {
        "─" | "━" => {
            let _ = writeln!(
                out,
                "<line x1=\"{x:.1}\" y1=\"{cy:.1}\" x2=\"{x2:.1}\" y2=\"{cy:.1}\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "╴" | "╸" => {
            let _ = writeln!(
                out,
                "<line x1=\"{x:.1}\" y1=\"{cy:.1}\" x2=\"{cx:.1}\" y2=\"{cy:.1}\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "╶" | "╺" => {
            let _ = writeln!(
                out,
                "<line x1=\"{cx:.1}\" y1=\"{cy:.1}\" x2=\"{x2:.1}\" y2=\"{cy:.1}\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "│" | "┃" => {
            let _ = writeln!(
                out,
                "<line x1=\"{cx:.1}\" y1=\"{y:.1}\" x2=\"{cx:.1}\" y2=\"{y2:.1}\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "╭" => {
            let _ = writeln!(
                out,
                "<path d=\"M {x2:.1} {cy:.1} L {rx:.1} {cy:.1} Q {cx:.1} {cy:.1} {cx:.1} {ry:.1} L {cx:.1} {y2:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>",
                rx = cx + r,
                ry = cy + r,
            );
        }
        "┌" => {
            let _ = writeln!(
                out,
                "<path d=\"M {x2:.1} {cy:.1} L {cx:.1} {cy:.1} L {cx:.1} {y2:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "╮" => {
            let _ = writeln!(
                out,
                "<path d=\"M {x:.1} {cy:.1} L {rx:.1} {cy:.1} Q {cx:.1} {cy:.1} {cx:.1} {ry:.1} L {cx:.1} {y2:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>",
                rx = cx - r,
                ry = cy + r,
            );
        }
        "┐" => {
            let _ = writeln!(
                out,
                "<path d=\"M {x:.1} {cy:.1} L {cx:.1} {cy:.1} L {cx:.1} {y2:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "╰" => {
            let _ = writeln!(
                out,
                "<path d=\"M {cx:.1} {y:.1} L {cx:.1} {ry:.1} Q {cx:.1} {cy:.1} {rx:.1} {cy:.1} L {x2:.1} {cy:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>",
                rx = cx + r,
                ry = cy - r,
            );
        }
        "└" => {
            let _ = writeln!(
                out,
                "<path d=\"M {cx:.1} {y:.1} L {cx:.1} {cy:.1} L {x2:.1} {cy:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "╯" => {
            let _ = writeln!(
                out,
                "<path d=\"M {cx:.1} {y:.1} L {cx:.1} {ry:.1} Q {cx:.1} {cy:.1} {rx:.1} {cy:.1} L {x:.1} {cy:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>",
                rx = cx - r,
                ry = cy - r,
            );
        }
        "┘" => {
            let _ = writeln!(
                out,
                "<path d=\"M {cx:.1} {y:.1} L {cx:.1} {cy:.1} L {x:.1} {cy:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "├" => {
            let _ = writeln!(
                out,
                "<path d=\"M {cx:.1} {y:.1} L {cx:.1} {y2:.1} M {cx:.1} {cy:.1} L {x2:.1} {cy:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "┤" => {
            let _ = writeln!(
                out,
                "<path d=\"M {cx:.1} {y:.1} L {cx:.1} {y2:.1} M {x:.1} {cy:.1} L {cx:.1} {cy:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "┬" => {
            let _ = writeln!(
                out,
                "<path d=\"M {x:.1} {cy:.1} L {x2:.1} {cy:.1} M {cx:.1} {cy:.1} L {cx:.1} {y2:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "┴" => {
            let _ = writeln!(
                out,
                "<path d=\"M {x:.1} {cy:.1} L {x2:.1} {cy:.1} M {cx:.1} {y:.1} L {cx:.1} {cy:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "┼" => {
            let _ = writeln!(
                out,
                "<path d=\"M {x:.1} {cy:.1} L {x2:.1} {cy:.1} M {cx:.1} {y:.1} L {cx:.1} {y2:.1}\" fill=\"none\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
            );
        }
        "█" => {
            let _ = writeln!(
                out,
                "<rect fill=\"{fg}\" x=\"{x:.1}\" y=\"{y:.1}\" width=\"{CELL_W:.1}\" height=\"{CELL_H:.1}\" shape-rendering=\"crispEdges\"/>"
            );
        }
        "▌" => {
            let _ = writeln!(
                out,
                "<rect fill=\"{fg}\" x=\"{x:.1}\" y=\"{y:.1}\" width=\"{hw:.1}\" height=\"{CELL_H:.1}\" shape-rendering=\"crispEdges\"/>",
                hw = CELL_W / 2.0,
            );
        }
        "▐" => {
            let _ = writeln!(
                out,
                "<rect fill=\"{fg}\" x=\"{cx:.1}\" y=\"{y:.1}\" width=\"{hw:.1}\" height=\"{CELL_H:.1}\" shape-rendering=\"crispEdges\"/>",
                hw = CELL_W / 2.0,
            );
        }
        _ => {}
    }
}

fn render_buffer_group(
    buf: &Buffer,
    styles: &mut HashMap<(String, bool), usize>,
    out_rects: &mut String,
    out_vectors: &mut String,
    out_texts: &mut String,
) {
    let default_bg = "#0b0f19";
    let default_fg = "#e2e8f0";

    for y in 0..buf.area.height {
        let row_y = (y as f64) * CELL_H;
        // 1. Coalesce background runs
        let mut x = 0u16;
        while x < buf.area.width {
            let bg = color_to_hex(buf[(x, y)].bg, default_bg);
            let mut end_x = x + 1;
            while end_x < buf.area.width && color_to_hex(buf[(end_x, y)].bg, default_bg) == bg {
                end_x += 1;
            }
            let rx = (x as f64) * CELL_W;
            let rw = ((end_x - x) as f64) * CELL_W;
            let _ = writeln!(
                out_rects,
                "<rect fill=\"{bg}\" x=\"{rx:.1}\" y=\"{row_y:.1}\" width=\"{rw:.1}\" height=\"{CELL_H:.1}\" shape-rendering=\"crispEdges\"/>"
            );
            x = end_x;
        }

        // 2. Render box/block characters as true SVG vectors and text runs with exact cell positions
        let mut tx = 0u16;
        while tx < buf.area.width {
            let cell = &buf[(tx, y)];
            let sym = cell.symbol();
            if sym.is_empty() || sym == " " {
                tx += 1;
                continue;
            }
            let fg = color_to_hex(cell.fg, default_fg);

            if is_box_or_block_char(sym) {
                // Coalesce horizontal line runs (`─` or `━`) into a single <line>
                if sym == "─" || sym == "━" {
                    let start_x = tx;
                    let mut end_x = tx + 1;
                    while end_x < buf.area.width
                        && buf[(end_x, y)].symbol() == sym
                        && color_to_hex(buf[(end_x, y)].fg, default_fg) == fg
                    {
                        end_x += 1;
                    }
                    let x1 = (start_x as f64) * CELL_W;
                    let x2 = (end_x as f64) * CELL_W;
                    let cy = row_y + CELL_H / 2.0;
                    let sw = if sym == "━" { 2.4 } else { 1.4 };
                    let _ = writeln!(
                        out_vectors,
                        "<line x1=\"{x1:.1}\" y1=\"{cy:.1}\" x2=\"{x2:.1}\" y2=\"{cy:.1}\" stroke=\"{fg}\" stroke-width=\"{sw}\"/>"
                    );
                    tx = end_x;
                    continue;
                }

                render_box_char_svg(out_vectors, sym, (tx as f64) * CELL_W, row_y, &fg);
                tx += 1;
                continue;
            }

            let bold = cell.modifier.contains(Modifier::BOLD);
            let key = (fg.clone(), bold);
            let next_id = styles.len() + 1;
            let class_id = *styles.entry(key).or_insert(next_id);

            // Coalesce contiguous text (including single spaces between words of same style/bg)
            let start_x = tx;
            let bg = color_to_hex(cell.bg, default_bg);
            let mut run_text = String::from(sym);
            let mut run_cells = 1u16;
            tx += 1;

            while tx < buf.area.width {
                let c = &buf[(tx, y)];
                let c_sym = c.symbol();
                if c_sym.is_empty() {
                    tx += 1;
                    continue;
                }
                if is_box_or_block_char(c_sym) {
                    break;
                }
                let c_fg = color_to_hex(c.fg, default_fg);
                let c_bg = color_to_hex(c.bg, default_bg);
                let c_bold = c.modifier.contains(Modifier::BOLD);

                if c_sym == " " {
                    // Only include a single space if the next cell continues the same text style & bg
                    if tx + 1 < buf.area.width {
                        let next_c = &buf[(tx + 1, y)];
                        let next_sym = next_c.symbol();
                        if !next_sym.is_empty()
                            && next_sym != " "
                            && !is_box_or_block_char(next_sym)
                            && color_to_hex(next_c.fg, default_fg) == fg
                            && color_to_hex(next_c.bg, default_bg) == bg
                            && next_c.modifier.contains(Modifier::BOLD) == bold
                            && c_bg == bg
                        {
                            run_text.push(' ');
                            run_cells += 1;
                            tx += 1;
                            continue;
                        }
                    }
                    break;
                }

                if c_fg == fg && c_bg == bg && c_bold == bold {
                    run_text.push_str(c_sym);
                    run_cells += 1;
                    tx += 1;
                } else {
                    break;
                }
            }

            let x_pos = (start_x as f64) * CELL_W;
            let text_y = row_y + CELL_H - 5.0;
            let escaped = xml_escape(&run_text);
            let target_w = (run_cells as f64) * CELL_W;
            if run_cells > 1 {
                let _ = writeln!(
                    out_texts,
                    "<text class=\"s{class_id}\" x=\"{x_pos:.1}\" y=\"{text_y:.1}\" textLength=\"{target_w:.1}\">{escaped}</text>"
                );
            } else {
                let cx_pos = x_pos + CELL_W / 2.0;
                let _ = writeln!(
                    out_texts,
                    "<text class=\"s{class_id}\" x=\"{cx_pos:.1}\" y=\"{text_y:.1}\" text-anchor=\"middle\">{escaped}</text>"
                );
            }
        }
    }
}

fn shared_css_header() -> &'static str {
    "@font-face {\n\
       font-family: 'Fira Code';\n\
       src: local('FiraCode-Regular'),\n\
            url('https://cdnjs.cloudflare.com/ajax/libs/firacode/6.2.0/woff2/FiraCode-Regular.woff2') format('woff2');\n\
       font-style: normal;\n\
       font-weight: 400;\n\
     }\n\
     @font-face {\n\
       font-family: 'Fira Code';\n\
       src: local('FiraCode-Bold'),\n\
            url('https://cdnjs.cloudflare.com/ajax/libs/firacode/6.2.0/woff2/FiraCode-Bold.woff2') format('woff2');\n\
       font-style: normal;\n\
       font-weight: 700;\n\
     }\n\
     .matrix { font-family: 'Fira Code', 'JetBrains Mono', 'DejaVu Sans Mono', monospace; font-size: 16.5px; }\n\
     .caption { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; font-size: 15px; font-weight: 600; fill: #cbd5e1; text-anchor: middle; }\n"
}

fn build_animated_svg(scenes: &[Scene]) -> String {
    let total_w = (TERM_W as f64) * CELL_W + PAD_X * 2.0;
    let total_h = (TERM_H as f64) * CELL_H + PAD_TOP + PAD_BOTTOM;
    let mut styles: HashMap<(String, bool), usize> = HashMap::new();
    let mut frame_groups = Vec::with_capacity(scenes.len());

    for (idx, scene) in scenes.iter().enumerate() {
        let mut rects = String::new();
        let mut vectors = String::new();
        let mut texts = String::new();
        render_buffer_group(
            &scene.buf,
            &mut styles,
            &mut rects,
            &mut vectors,
            &mut texts,
        );
        let caption = xml_escape(scene.caption);
        let group = format!(
            "<g class=\"frame f{idx}\">\n\
             <text class=\"caption\" x=\"{cx:.1}\" y=\"28\">yamlly (ymly) — {caption}</text>\n\
             <g transform=\"translate({PAD_X:.1}, {PAD_TOP:.1})\">\n\
             {rects}\
             {vectors}\
             <g class=\"matrix\">\n{texts}</g>\n\
             </g>\n\
             </g>",
            cx = total_w / 2.0,
        );
        frame_groups.push(group);
    }

    let n = scenes.len();
    let step_pct = 100.0 / (n as f64);
    let total_secs = (n as f64) * 2.6;

    let mut css = String::from(shared_css_header());
    css.push_str(".frame { opacity: 0; }\n");

    for idx in 0..n {
        let start = (idx as f64) * step_pct;
        let end = start + step_pct;
        let _ = writeln!(
            css,
            ".f{idx} {{ animation: show{idx} {total_secs:.1}s infinite; }}\n\
             @keyframes show{idx} {{\n\
               0%, {s_prev:.2}% {{ opacity: 0; }}\n\
               {start:.2}%, {e_hold:.2}% {{ opacity: 1; }}\n\
               {end:.2}%, 100% {{ opacity: 0; }}\n\
             }}",
            s_prev = (start - 0.01).max(0.0),
            e_hold = (end - 0.01).max(start),
        );
    }

    let mut sorted_styles: Vec<((String, bool), usize)> = styles.into_iter().collect();
    sorted_styles.sort_by_key(|(_, id)| *id);
    for ((fg, bold), id) in sorted_styles {
        if bold {
            let _ = writeln!(css, ".s{id} {{ fill: {fg}; font-weight: 700; }}");
        } else {
            let _ = writeln!(css, ".s{id} {{ fill: {fg}; }}");
        }
    }

    let mut svg = String::new();
    let _ = writeln!(
        svg,
        "<svg viewBox=\"0 0 {total_w:.1} {total_h:.1}\" width=\"{total_w:.0}\" height=\"{total_h:.0}\" xmlns=\"http://www.w3.org/2000/svg\">\n\
         <style>\n{css}</style>\n\
         <rect fill=\"#111827\" stroke=\"#334155\" stroke-width=\"1.5\" x=\"1\" y=\"1\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"10\"/>\n\
         <g transform=\"translate(22, 23)\">\n\
           <circle cx=\"0\" cy=\"0\" r=\"6.5\" fill=\"#ff5f57\"/>\n\
           <circle cx=\"20\" cy=\"0\" r=\"6.5\" fill=\"#febc2e\"/>\n\
           <circle cx=\"40\" cy=\"0\" r=\"6.5\" fill=\"#28c840\"/>\n\
         </g>",
        w = total_w - 2.0,
        h = total_h - 2.0,
    );

    for g in frame_groups {
        svg.push_str(&g);
        svg.push('\n');
    }
    svg.push_str("</svg>\n");
    svg
}

fn build_static_svg(scene: &Scene) -> String {
    let total_w = (TERM_W as f64) * CELL_W + PAD_X * 2.0;
    let total_h = (TERM_H as f64) * CELL_H + PAD_TOP + PAD_BOTTOM;
    let mut styles: HashMap<(String, bool), usize> = HashMap::new();
    let mut rects = String::new();
    let mut vectors = String::new();
    let mut texts = String::new();
    render_buffer_group(
        &scene.buf,
        &mut styles,
        &mut rects,
        &mut vectors,
        &mut texts,
    );

    let mut css = String::from(shared_css_header());
    let mut sorted_styles: Vec<((String, bool), usize)> = styles.into_iter().collect();
    sorted_styles.sort_by_key(|(_, id)| *id);
    for ((fg, bold), id) in sorted_styles {
        if bold {
            let _ = writeln!(css, ".s{id} {{ fill: {fg}; font-weight: 700; }}");
        } else {
            let _ = writeln!(css, ".s{id} {{ fill: {fg}; }}");
        }
    }

    let caption = xml_escape(scene.caption);
    format!(
        "<svg viewBox=\"0 0 {total_w:.1} {total_h:.1}\" width=\"{total_w:.0}\" height=\"{total_h:.0}\" xmlns=\"http://www.w3.org/2000/svg\">\n\
         <style>\n{css}</style>\n\
         <rect fill=\"#111827\" stroke=\"#334155\" stroke-width=\"1.5\" x=\"1\" y=\"1\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"10\"/>\n\
         <g transform=\"translate(22, 23)\">\n\
           <circle cx=\"0\" cy=\"0\" r=\"6.5\" fill=\"#ff5f57\"/>\n\
           <circle cx=\"20\" cy=\"0\" r=\"6.5\" fill=\"#febc2e\"/>\n\
           <circle cx=\"40\" cy=\"0\" r=\"6.5\" fill=\"#28c840\"/>\n\
         </g>\n\
         <text class=\"caption\" x=\"{cx:.1}\" y=\"28\">yamlly (ymly) — {caption}</text>\n\
         <g transform=\"translate({PAD_X:.1}, {PAD_TOP:.1})\">\n\
         {rects}\
         {vectors}\
         <g class=\"matrix\">\n{texts}</g>\n\
         </g>\n\
         </svg>\n",
        w = total_w - 2.0,
        h = total_h - 2.0,
        cx = total_w / 2.0,
    )
}

fn main() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    let mut scenes = Vec::new();

    // Scene 1: Launch screen / Empty workspace with quick-launch cards
    let buf1 = capture(&mut ratcn, &mut app);
    scenes.push(Scene {
        caption: "1/7 · Interactive Launch Screen & 22 Built-in Kubernetes Schemas",
        buf: buf1,
    });

    // Scene 2: Open Deployment tab and fill metadata.name
    app.open_resource("Deployment", None);
    let _ = capture(&mut ratcn, &mut app);
    send_text(&mut ratcn, &mut app, "payments-api");
    let buf2 = capture(&mut ratcn, &mut app);
    scenes.push(Scene {
        caption: "2/7 · Schema-Driven Form Editor (Deployment: payments-api)",
        buf: buf2,
    });

    // Scene 3: Fill replicas and container fields
    let rows_len = app.active_form().map(|f| f.rows.len()).unwrap_or(0);
    for idx in 0..rows_len {
        let path = app
            .active_form()
            .and_then(|f| match &f.rows[idx].kind {
                yamlly_core::Row::Field { field, .. } => Some(field.path.clone()),
                _ => None,
            })
            .unwrap_or_default();
        match path.as_str() {
            "spec.replicas" => app.update(Msg::FormInputChanged {
                row_idx: idx,
                state: InputState::new("3"),
            }),
            "spec.template.spec.containers[0].name" => app.update(Msg::FormInputChanged {
                row_idx: idx,
                state: InputState::new("payments"),
            }),
            "spec.template.spec.containers[0].image" => app.update(Msg::FormInputChanged {
                row_idx: idx,
                state: InputState::new("ghcr.io/org/payments:v1.4.0"),
            }),
            _ => {}
        }
    }
    let buf3 = capture(&mut ratcn, &mut app);
    scenes.push(Scene {
        caption: "3/7 · Live Dirty Tracking, Collapsible Sections & Container Spec",
        buf: buf3.clone(),
    });

    // Scene 4: Open Interactive Field Picker (+ Fields) and select `spec.strategy`
    app.update(Msg::OpenFieldPicker);
    app.field_picker_modal.cursor = Some(format!("{FIELD_PICKER_GROUP_PREFIX}spec.strategy"));
    app.update(Msg::FieldPickerToggleGroup);
    let buf4 = capture(&mut ratcn, &mut app);
    scenes.push(Scene {
        caption: "4/7 · Interactive Field Picker (+ Fields) — Mount Optional Schema Categories",
        buf: buf4,
    });

    // Scene 5: Commit Field Picker and show newly mounted `strategy` category with [✕ Remove]
    app.update(Msg::FieldPickerCommit);
    let buf5 = capture(&mut ratcn, &mut app);
    scenes.push(Scene {
        caption: "5/7 · Mounted Optional Category (`spec.strategy`) with Removable Badge",
        buf: buf5,
    });

    // Scene 6: Open YAML Preview & Save modal (`p` / `Ctrl+S`)
    app.update(Msg::OpenPreview);
    let buf6 = capture(&mut ratcn, &mut app);
    scenes.push(Scene {
        caption: "6/7 · Syntax-Highlighted YAML Preview & Save Dialog (`~/payments-api.yaml`)",
        buf: buf6,
    });

    // Scene 7: Close modal and cycle theme (`t`)
    app.update(Msg::CloseModal);
    app.update(Msg::CycleTheme);
    let buf7 = capture(&mut ratcn, &mut app);
    scenes.push(Scene {
        caption: "7/7 · Instant Theme Cycling (`t`) & Clickable Bottom Bar",
        buf: buf7,
    });

    let assets_dir = Path::new("assets");
    fs::create_dir_all(assets_dir).expect("create assets dir");

    let animated_svg = build_animated_svg(&scenes);
    fs::write(assets_dir.join("demo.svg"), animated_svg).expect("write assets/demo.svg");

    let static_svg = build_static_svg(&scenes[2]);
    fs::write(assets_dir.join("screenshot.svg"), static_svg).expect("write assets/screenshot.svg");

    println!("✓ Generated assets/demo.svg and assets/screenshot.svg");
}
