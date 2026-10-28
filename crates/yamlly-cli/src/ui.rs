//! `ratcn` UI declaration, rendering, and event routing for `yamlly`.
//!
//! Uses `ratcn`'s interactive component architecture (`Ratcn<App, Msg>`) and
//! built-in large-sized components with generous spacing:
//! - [`Button`] with [`ButtonSize::Large`] (3-row tall buttons) and `.outline()`
//! - [`Input`] + [`InputState`] with `.title(...)` (3-row tall bordered inputs)
//! - [`List`] + [`ListItem`] (single-selection and multi-selection)
//! - [`Tabs`] + [`Tab`] with [`TabsSize::Large`] (3-row tall tab strip)
//! - [`Checkbox`] (inside 3-row titled field containers)
//! - [`Cycle`] (inside 3-row titled field containers)
//! - [`ScrollArea`] (scrollable form viewport, YAML preview, and map editor)
//! - [`Dialog`] (all modal layers managed via [`ratcn::runtime::ModalState`])
//! - [`ToasterWidget`] (transient corner notifications)

use std::collections::HashMap;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use ratcn::runtime::{
    ChildId, DeclareCtx, Event, EventResult, FocusState, KeyCode, KeyEvent, MouseButton, MouseKind,
    PaintCtx, Ratcn, ScopeOptions, TabWrap,
};
use ratcn::{
    BorderStyle, Button, ButtonSize, Checkbox, Dialog, Input, List, ListItem, ScrollArea, Tab,
    Tabs, TabsActivation, TabsSize, ToastPosition, ToasterWidget,
};
use yamlly_core::field::{FieldKind, Row};

use crate::app::{App, Msg};
use crate::k8s::extract_map_from_cluster_item;
use crate::modals::{
    ClusterImportModalState, FIELD_PICKER_GROUP_PREFIX, FieldPickerRow, MODAL_CLUSTER_IMPORT,
    MODAL_CONFIRM, MODAL_CONTEXT_SWITCHER, MODAL_DIR_PICKER, MODAL_FIELD_PICKER, MODAL_MAP_EDITOR,
    MODAL_PREVIEW, MODAL_SELECT_PICKER, SELECT_UNSET_VALUE,
};

/// Construct the configured `Ratcn` runtime bound to [`App::focus`] and [`App::modals`].
pub fn new_runtime() -> Ratcn<App, Msg> {
    Ratcn::new()
        .focus(|app: &App| &app.focus, Msg::FocusChanged)
        .modals(|app: &App| &app.modals)
}

/// Render a full frame through `ratcn` and overlay active toasts.
pub fn render(ratcn: &mut Ratcn<App, Msg>, frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    let theme = *app.theme();
    ratcn.render(frame, area, app, &theme, |ctx| {
        declare_app(ctx);
    });

    // Render transient corner toasts above all layers
    frame.render_widget(
        ToasterWidget::new(&app.toasts, app.clock)
            .themed(&theme)
            .position(ToastPosition::BottomRight)
            .max_visible_toasts(3),
        area,
    );
}

/// Route a `ratcn::runtime::Event` through the retained surface, falling back to
/// app-level shortcuts when the focused component ignores the event.
pub fn handle_event(ratcn: &mut Ratcn<App, Msg>, app: &mut App, event: Event) -> bool {
    if app.handle_global_ctrl_shortcut(&event) {
        return true;
    }
    if let Event::Mouse(mouse) = &event {
        if mouse.kind == MouseKind::Down(MouseButton::Left)
            && handle_footer_click(ratcn, app, mouse.column, mouse.row)
        {
            return true;
        }
        if mouse.kind == MouseKind::Up(MouseButton::Left) && is_on_footer_row(app, mouse.row) {
            return true;
        }
    }
    if let Event::Key(key) = &event {
        if !key.modifiers.any() {
            if app.modals.top().map(|m| m.as_str()) == Some(MODAL_CLUSTER_IMPORT) {
                match key.code {
                    KeyCode::Up => {
                        app.cluster_import_modal.select_next(false);
                        app.ui_dirty = true;
                        return true;
                    }
                    KeyCode::Down => {
                        app.cluster_import_modal.select_next(true);
                        app.ui_dirty = true;
                        return true;
                    }
                    _ => {}
                }
            }
            if app.modals.top().map(|m| m.as_str()) == Some(MODAL_CONTEXT_SWITCHER) {
                match key.code {
                    KeyCode::Up => {
                        app.context_switcher_modal.select_next(false);
                        app.ui_dirty = true;
                        return true;
                    }
                    KeyCode::Down => {
                        app.context_switcher_modal.select_next(true);
                        app.ui_dirty = true;
                        return true;
                    }
                    _ => {}
                }
            }
        }
        if key.code == KeyCode::Enter && !key.modifiers.any() {
            if app.modals.top().map(|m| m.as_str()) == Some(MODAL_FIELD_PICKER)
                && !app.focus.contains_path([
                    ChildId::Static(MODAL_FIELD_PICKER),
                    ChildId::Static("cancel"),
                ])
            {
                app.update(Msg::FieldPickerCommit);
                return true;
            }
            if app.modals.top().map(|m| m.as_str()) == Some(MODAL_CLUSTER_IMPORT)
                && !app.focus.contains_path([
                    ChildId::Static(MODAL_CLUSTER_IMPORT),
                    ChildId::Static("cancel"),
                ])
            {
                app.update(Msg::ClusterImportCommitSelected);
                return true;
            }
            if app.modals.top().map(|m| m.as_str()) == Some(MODAL_CONTEXT_SWITCHER)
                && !app.focus.contains_path([
                    ChildId::Static(MODAL_CONTEXT_SWITCHER),
                    ChildId::Static("cancel"),
                ])
            {
                app.update(Msg::ContextSwitcherCommitSelected);
                return true;
            }
        }
    }
    match ratcn.handle_event(event.clone(), app) {
        EventResult::Emit(msg) => {
            app.update(msg);
            true
        }
        EventResult::Consumed => {
            app.ui_dirty = true;
            true
        }
        EventResult::Ignored => app.handle_fallback_event(event),
    }
}

/// Convert and route a `crossterm::event::Event`.
pub fn handle_crossterm_event(
    ratcn: &mut Ratcn<App, Msg>,
    app: &mut App,
    ev: ratatui::crossterm::event::Event,
) -> bool {
    match Event::try_from(ev) {
        Ok(event) => handle_event(ratcn, app, event),
        Err(_) => {
            // Terminal resize or focus change triggers a clean redraw
            app.ui_dirty = true;
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Root Declaration
// ---------------------------------------------------------------------------

fn declare_app(ctx: &mut DeclareCtx<'_, App, Msg>) {
    let root = ctx.area();

    // Base background fill
    ctx.paint(move |pctx| {
        let bg = pctx.theme.background;
        let fg = pctx.theme.foreground;
        pctx.with_buffer(root, |area, buf| {
            buf.set_style(area, Style::default().bg(bg).fg(fg));
        });
    });

    if root.height < 6 || root.width < 20 {
        return;
    }

    let header_h = if root.height >= 14 { 3 } else { 1 };
    let header_area = Rect::new(root.x, root.y, root.width, header_h);
    let footer_area = Rect::new(root.x, root.bottom().saturating_sub(1), root.width, 1);
    let body_area = Rect::new(
        root.x,
        root.y.saturating_add(header_h),
        root.width,
        root.height.saturating_sub(header_h.saturating_add(1)),
    );

    declare_header(ctx, header_area);
    declare_body(ctx, body_area);
    declare_footer(ctx, footer_area);

    // Declare open modal dialogs in exact stack order
    let open_modals: Vec<ChildId> = ctx.state().modals.ids().cloned().collect();
    for modal_id in open_modals {
        match modal_id.as_str() {
            MODAL_CONFIRM => declare_confirm_modal(ctx),
            MODAL_FIELD_PICKER => declare_field_picker_modal(ctx),
            MODAL_PREVIEW => declare_preview_modal(ctx),
            MODAL_DIR_PICKER => declare_dir_picker_modal(ctx),
            MODAL_MAP_EDITOR => declare_map_editor_modal(ctx),
            MODAL_SELECT_PICKER => declare_select_picker_modal(ctx),
            MODAL_CLUSTER_IMPORT => declare_cluster_import_modal(ctx),
            MODAL_CONTEXT_SWITCHER => declare_context_switcher_modal(ctx),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Header Bar (3 rows tall with large buttons & spacing)
// ---------------------------------------------------------------------------

fn declare_header(ctx: &mut DeclareCtx<'_, App, Msg>, area: Rect) {
    let cluster = ctx.state().cluster_status.clone();
    let theme_name = ctx.state().theme().name;
    let theme_label = format!("◐ {theme_name}");
    let reconnect_label = "⟳ Reconnect";

    let context_label = if area.width >= 130 {
        let raw_ctx_name = cluster.context_name.as_deref().unwrap_or("Context");
        let display_ctx_name = if raw_ctx_name.chars().count() > 14 {
            let truncated: String = raw_ctx_name.chars().take(12).collect();
            format!("{truncated}…")
        } else {
            raw_ctx_name.to_string()
        };
        format!("⎈ {display_ctx_name}")
    } else {
        "⎈ Context".to_string()
    };

    let context_btn_w = (context_label.chars().count() as u16 + 4)
        .clamp(12, 22)
        .min(area.width);
    let reconnect_btn_w = (reconnect_label.chars().count() as u16 + 4).min(area.width);
    let theme_btn_w = (theme_label.chars().count() as u16 + 4).min(area.width);
    let btn_gap = 2u16;
    let right_total = context_btn_w
        .saturating_add(reconnect_btn_w)
        .saturating_add(theme_btn_w)
        .saturating_add(btn_gap * 2)
        .saturating_add(1);

    ctx.paint(move |pctx| {
        let surface = pctx.theme.surface;
        let primary = pctx.theme.primary;
        let primary_fg = pctx.theme.primary_foreground;
        let muted = pctx.theme.muted_foreground;
        let warning = pctx.theme.warning;
        let destructive = pctx.theme.destructive;

        let (status_dot, status_text, status_color) = if cluster.is_checking {
            ("◌", format!("Checking ({})", cluster.namespace), warning)
        } else if cluster.connected {
            let ctx_name = cluster.context_name.as_deref().unwrap_or("cluster");
            (
                "●",
                format!("Connected: {ctx_name} [{}]", cluster.namespace),
                Color::Rgb(74, 222, 128),
            )
        } else {
            (
                "○",
                format!("Disconnected [{}]", cluster.namespace),
                destructive,
            )
        };

        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled(
                " ◆ yamlly ",
                Style::default()
                    .bg(primary)
                    .fg(primary_fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  K8s Manifest Studio  ", Style::default().fg(muted)),
            Span::styled("│  ", Style::default().fg(pctx.theme.border)),
            Span::styled(
                format!("{status_dot} "),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(status_text, Style::default().fg(pctx.theme.foreground)),
        ]);

        pctx.with_buffer(area, |rect, buf| {
            buf.set_style(rect, Style::default().bg(surface));
            let text_y = rect.y.saturating_add(rect.height / 2);
            let text_max_w = if rect.width > right_total {
                rect.width.saturating_sub(right_total)
            } else {
                rect.width
            };
            let text_rect = Rect::new(rect.x, text_y, text_max_w, 1);
            Paragraph::new(line).render(text_rect, buf);
        });
    });

    if area.width > right_total.saturating_add(20) {
        let btn_size = if area.height >= 3 {
            ButtonSize::Large
        } else {
            ButtonSize::default()
        };
        let btn_h = btn_size.height();
        let header_btns_area = Rect::new(
            area.right().saturating_sub(right_total),
            area.y,
            right_total,
            btn_h,
        );
        ctx.scope(
            "header",
            header_btns_area,
            ScopeOptions::default().tab_wrap(TabWrap::Escape),
            move |ctx| {
                let a = ctx.area();
                let context_rect = Rect::new(a.x, a.y, context_btn_w, btn_h);
                let reconnect_rect = Rect::new(
                    a.x.saturating_add(context_btn_w).saturating_add(btn_gap),
                    a.y,
                    reconnect_btn_w,
                    btn_h,
                );
                let theme_rect = Rect::new(
                    reconnect_rect.right().saturating_add(btn_gap),
                    a.y,
                    theme_btn_w,
                    btn_h,
                );
                ctx.component(
                    "context",
                    Button::new(context_label)
                        .outline()
                        .size(btn_size)
                        .on_press(|| Msg::OpenContextSwitcher),
                    context_rect,
                );
                ctx.component(
                    "reconnect",
                    Button::new(reconnect_label)
                        .outline()
                        .size(btn_size)
                        .on_press(|| Msg::Reconnect),
                    reconnect_rect,
                );
                ctx.component(
                    "theme",
                    Button::new(theme_label)
                        .outline()
                        .size(btn_size)
                        .on_press(|| Msg::CycleTheme),
                    theme_rect,
                );
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Body Split: Sidebar + Gap + Main Workspace
// ---------------------------------------------------------------------------

fn declare_body(ctx: &mut DeclareCtx<'_, App, Msg>, area: Rect) {
    let sidebar_w = (area.width / 4).clamp(26, 30).min(area.width);
    let gap = if area.width > sidebar_w.saturating_add(12) {
        1
    } else {
        0
    };
    let sidebar_area = Rect::new(area.x, area.y, sidebar_w, area.height);
    let workspace_area = Rect::new(
        area.x.saturating_add(sidebar_w).saturating_add(gap),
        area.y,
        area.width.saturating_sub(sidebar_w.saturating_add(gap)),
        area.height,
    );

    declare_sidebar(ctx, sidebar_area);
    if workspace_area.width > 0 {
        declare_workspace(ctx, workspace_area);
    }
}

fn declare_sidebar(ctx: &mut DeclareCtx<'_, App, Msg>, area: Rect) {
    ctx.scope(
        "sidebar",
        area,
        ScopeOptions::default().tab_wrap(TabWrap::Escape),
        |ctx| {
            let pane = ctx.area();
            let filtered = ctx.state().filtered_resources();
            let filtered_len = filtered.len();
            let total_len = ctx.state().resources.len();
            let items: Vec<ListItem<String>> = filtered
                .into_iter()
                .map(|r| ListItem::new(r.kind.clone(), r.kind.clone()))
                .collect();
            let api_map: HashMap<String, String> = ctx
                .state()
                .resources
                .iter()
                .map(|r| (r.kind.clone(), r.api_version.clone()))
                .collect();
            let muted_fg = ctx.state().theme().muted_foreground;

            // Paint rounded border box around sidebar
            ctx.paint(move |pctx| {
                let border_color = if pctx.contains_focus() {
                    pctx.theme.ring
                } else {
                    pctx.theme.border
                };
                let title_color = if pctx.contains_focus() {
                    pctx.theme.primary
                } else {
                    pctx.theme.foreground
                };
                let title = if filtered_len == total_len {
                    format!(" Resources ({total_len}) ")
                } else {
                    format!(" Resources ({filtered_len}/{total_len}) ")
                };
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_set(BorderStyle::Rounded.to_border_set())
                    .border_style(Style::default().fg(border_color))
                    .title(Span::styled(
                        title,
                        Style::default()
                            .fg(title_color)
                            .add_modifier(Modifier::BOLD),
                    ));
                pctx.widget(block, pane);
            });

            if pane.width < 6 || pane.height < 6 {
                return;
            }
            let inner = Rect::new(
                pane.x.saturating_add(2),
                pane.y.saturating_add(1),
                pane.width.saturating_sub(4),
                pane.height.saturating_sub(2),
            );

            // Large 3-row titled filter input
            let filter_h = if inner.height >= 6 { 3 } else { 1 };
            let filter_rect = Rect::new(inner.x, inner.y, inner.width, filter_h);
            let mut filter_input = Input::new()
                .prefix("⌕ ")
                .placeholder("Filter kinds (/)...")
                .value(|app: &App| &app.resource_filter, Msg::ResourceFilterChanged)
                .on_submit(|| Msg::ResourceFilterSubmit);
            if filter_h == 3 {
                filter_input = filter_input.title(" Search Resources (/) ");
            }
            ctx.component("filter", filter_input, filter_rect);

            let list_top_offset = filter_h.saturating_add(1); // 1 blank row of spacing below search
            if inner.height <= list_top_offset {
                return;
            }

            // Resource List with spacing below the filter box
            let list_rect = Rect::new(
                inner.x,
                inner.y.saturating_add(list_top_offset),
                inner.width,
                inner.height.saturating_sub(list_top_offset),
            );
            let inner_w = list_rect.width;
            ctx.component(
                "resources",
                List::new(items)
                    .focus_symbol("▸ ")
                    .selected_marker("● ")
                    .unselected_marker("  ")
                    .item_focus(
                        |app: &App| app.focused_resource_kind.clone(),
                        Msg::ResourceFocusChanged,
                    )
                    .selection(
                        |app: &App| app.active_resource_kind(),
                        Msg::ResourceSelected,
                    )
                    .paint_item(move |_app: &App, row| {
                        let api = api_map.get(row.value).map(String::as_str).unwrap_or("");
                        let avail = inner_w.saturating_sub(6) as usize;
                        let api_len = api.chars().count();
                        let label_len = row.label.chars().count();
                        if avail >= label_len.saturating_add(api_len).saturating_add(2) {
                            let kind_pad = avail.saturating_sub(api_len);
                            Text::from(Line::from(vec![
                                Span::raw(format!("{:<width$}", row.label, width = kind_pad)),
                                Span::styled(api.to_string(), Style::default().fg(muted_fg)),
                            ]))
                        } else {
                            Text::from(row.label.to_string())
                        }
                    }),
                list_rect,
            );
        },
    );
}

// ---------------------------------------------------------------------------
// Main Workspace (Large Tabs + Large Toolbar + Spaced Form / Empty State)
// ---------------------------------------------------------------------------

fn declare_workspace(ctx: &mut DeclareCtx<'_, App, Msg>, area: Rect) {
    ctx.scope(
        "workspace",
        area,
        ScopeOptions::default().tab_wrap(TabWrap::Escape),
        |ctx| {
            let pane = ctx.area();
            let active_summary = ctx
                .state()
                .active_tab_idx
                .and_then(|i| ctx.state().tabs.get(i))
                .map(|t| (t.kind.clone(), t.form.api_version.clone(), t.dirty));

            ctx.paint(move |pctx| {
                let border_color = if pctx.contains_focus() {
                    pctx.theme.ring
                } else {
                    pctx.theme.border
                };
                let title_spans = match &active_summary {
                    Some((kind, api, dirty)) => {
                        let (badge, badge_color) = if *dirty {
                            ("● Modified", pctx.theme.warning)
                        } else {
                            ("✓ Clean", Color::Rgb(74, 222, 128))
                        };
                        Line::from(vec![
                            Span::styled(
                                format!(" {kind} "),
                                Style::default()
                                    .fg(pctx.theme.primary)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!("({api}) "),
                                Style::default().fg(pctx.theme.muted_foreground),
                            ),
                            Span::styled(
                                format!("{badge} "),
                                Style::default()
                                    .fg(badge_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ])
                    }
                    None => Line::from(Span::styled(
                        " Workspace ",
                        Style::default()
                            .fg(pctx.theme.foreground)
                            .add_modifier(Modifier::BOLD),
                    )),
                };
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_set(BorderStyle::Rounded.to_border_set())
                    .border_style(Style::default().fg(border_color))
                    .title(title_spans);
                pctx.widget(block, pane);
            });

            if pane.width < 8 || pane.height < 6 {
                return;
            }
            let inner = Rect::new(
                pane.x.saturating_add(2),
                pane.y.saturating_add(1),
                pane.width.saturating_sub(4),
                pane.height.saturating_sub(2),
            );

            if ctx.state().tabs.is_empty() {
                declare_empty_workspace(ctx, inner);
            } else {
                declare_active_workspace(ctx, inner);
            }
        },
    );
}

fn declare_empty_workspace(ctx: &mut DeclareCtx<'_, App, Msg>, area: Rect) {
    let hero_y = area.y.saturating_add(area.height / 4);
    ctx.paint(move |pctx| {
        let lines = vec![
            Line::from(Span::styled(
                "◆ Kubernetes Manifest Studio",
                Style::default()
                    .fg(pctx.theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "No resource tabs are open yet.",
                Style::default().fg(pctx.theme.foreground),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Select any resource kind in the left sidebar (Enter / Click) or quick-start below:",
                Style::default().fg(pctx.theme.muted_foreground),
            )),
        ];
        let text_rect = Rect::new(
            area.x.saturating_add(2),
            hero_y.min(area.bottom().saturating_sub(5)),
            area.width.saturating_sub(4),
            5.min(area.height),
        );
        pctx.widget(Paragraph::new(lines), text_rect);
    });

    // Large 3-row quick-launch buttons with 2-column horizontal spacing
    let btn_size = if area.height >= 12 {
        ButtonSize::Large
    } else {
        ButtonSize::default()
    };
    let btn_h = btn_size.height();
    let btn_y = hero_y
        .saturating_add(7)
        .min(area.bottom().saturating_sub(btn_h));

    if btn_y.saturating_add(btn_h) <= area.bottom() {
        let quick_kinds = ["Deployment", "Service", "Ingress", "ConfigMap", "Pod"];
        let mut x = area.x.saturating_add(2);
        let mut y = btn_y;
        for (i, kind) in quick_kinds.into_iter().enumerate() {
            let label = format!("+ {kind}");
            let w = (label.chars().count() as u16).saturating_add(4);
            if x.saturating_add(w) > area.right() {
                // Wrap to next button row with 1 blank row of vertical spacing
                x = area.x.saturating_add(2);
                y = y.saturating_add(btn_h).saturating_add(1);
                if y.saturating_add(btn_h) > area.bottom() {
                    break;
                }
            }
            let rect = Rect::new(x, y, w, btn_h);
            let k = kind.to_string();
            let btn = if i == 0 {
                Button::new(label)
                    .size(btn_size)
                    .on_press(move || Msg::ResourceSelected(k.clone()))
            } else {
                Button::new(label)
                    .outline()
                    .size(btn_size)
                    .on_press(move || Msg::ResourceSelected(k.clone()))
            };
            ctx.component(ChildId::from(format!("quick-{kind}")), btn, rect);
            x = x.saturating_add(w).saturating_add(2); // 2 columns of spacing between buttons
        }

        // Also offer direct "Import from Cluster" button on the home screen
        let import_label = "↓ Import from Cluster";
        let import_w = (import_label.chars().count() as u16).saturating_add(4);
        if x.saturating_add(import_w) > area.right() {
            x = area.x.saturating_add(2);
            y = y.saturating_add(btn_h).saturating_add(1);
        }
        if y.saturating_add(btn_h) <= area.bottom() {
            let rect = Rect::new(x, y, import_w, btn_h);
            ctx.component(
                "quick-import-cluster",
                Button::new(import_label)
                    .outline()
                    .size(btn_size)
                    .on_press(|| Msg::OpenClusterImportManifest),
                rect,
            );
        }
    }
}

fn declare_active_workspace(ctx: &mut DeclareCtx<'_, App, Msg>, area: Rect) {
    let use_large_chrome = area.height >= 16;
    let tab_h: u16 = if use_large_chrome { 3 } else { 1 };
    let btn_size = if use_large_chrome {
        ButtonSize::Large
    } else {
        ButtonSize::default()
    };

    // 1. Tab Strip Row (3 rows tall when large)
    let tab_row = Rect::new(area.x, area.y, area.width, tab_h);
    let close_w = 11u16.min(tab_row.width);
    let tabs_w = tab_row.width.saturating_sub(close_w.saturating_add(2));
    let close_rect = Rect::new(
        tab_row.right().saturating_sub(close_w),
        tab_row.y,
        close_w,
        tab_h,
    );

    let active_tab_idx = ctx.state().active_tab_idx;
    let mut cur_x = tab_row.x;
    let max_x = tab_row.x.saturating_add(tabs_w);

    for (i, t) in ctx.state().tabs.iter().enumerate() {
        if cur_x >= max_x {
            break;
        }
        let title = if t.dirty {
            format!("● {}", t.label)
        } else {
            t.label.clone()
        };
        let is_active = active_tab_idx == Some(i);

        let title_len = title.chars().count() as u16;
        let btn_pad: u16 = if use_large_chrome { 4 } else { 2 };
        let tab_btn_w = title_len.saturating_add(btn_pad).max(6);
        let x_btn_w: u16 = if use_large_chrome { 5 } else { 3 };

        let remaining = max_x.saturating_sub(cur_x);
        if remaining < 4 {
            break;
        }

        let actual_tab_w = tab_btn_w.min(remaining.saturating_sub(x_btn_w));
        let tab_btn_rect = Rect::new(cur_x, tab_row.y, actual_tab_w, tab_h);
        cur_x = cur_x.saturating_add(actual_tab_w);

        let actual_x_w = x_btn_w.min(max_x.saturating_sub(cur_x));
        let x_btn_rect = Rect::new(cur_x, tab_row.y, actual_x_w, tab_h);
        cur_x = cur_x.saturating_add(actual_x_w).saturating_add(1);

        let tab_id = t.id.clone();
        let tab_id_close = t.id.clone();

        let tab_comp = Button::new(title)
            .size(btn_size)
            .on_press(move || Msg::TabSelected(tab_id.clone()));
        let tab_comp = if is_active {
            tab_comp.outline()
        } else {
            tab_comp.ghost()
        };
        ctx.component(
            ChildId::from(format!("tab-btn-{i}")),
            tab_comp,
            tab_btn_rect,
        );

        let close_comp = Button::new("✕")
            .size(btn_size)
            .on_press(move || Msg::CloseTabById(tab_id_close.clone()));
        let close_comp = if is_active {
            close_comp.outline()
        } else {
            close_comp.ghost()
        };
        ctx.component(
            ChildId::from(format!("tab-close-{i}")),
            close_comp,
            x_btn_rect,
        );
    }

    ctx.component(
        "close-tab",
        Button::new("✕ Close")
            .outline()
            .size(btn_size)
            .on_press(|| Msg::CloseActiveTab),
        close_rect,
    );

    // 1 row of vertical spacing after the tab bar
    let toolbar_y = area.y.saturating_add(tab_h).saturating_add(1);
    let toolbar_h: u16 = if use_large_chrome { 3 } else { 1 };
    if area.bottom() <= toolbar_y.saturating_add(toolbar_h) {
        return;
    }

    // 2. Resource Action Toolbar (3 rows tall when large)
    let toolbar_row = Rect::new(area.x, toolbar_y, area.width, toolbar_h);
    declare_toolbar(ctx, toolbar_row, btn_size);

    // 3. Divider Row with spacing
    let div_y = toolbar_y.saturating_add(toolbar_h);
    if div_y >= area.bottom() {
        return;
    }
    let div_row = Rect::new(area.x, div_y, area.width, 1);
    ctx.paint(move |pctx| {
        let rule = "─".repeat(div_row.width as usize);
        pctx.widget(
            Paragraph::new(Line::styled(rule, Style::default().fg(pctx.theme.border))),
            div_row,
        );
    });

    // 4. Schema-Driven Form inside ScrollArea (1 row below divider)
    let form_y = div_y.saturating_add(1);
    let form_area = Rect::new(
        area.x,
        form_y,
        area.width,
        area.bottom().saturating_sub(form_y),
    );
    if form_area.height > 0 && form_area.width > 0 {
        declare_form(ctx, form_area);
    }
}

fn declare_toolbar(ctx: &mut DeclareCtx<'_, App, Msg>, area: Rect, btn_size: ButtonSize) {
    let Some(idx) = ctx.state().active_tab_idx else {
        return;
    };
    let Some(tab) = ctx.state().tabs.get(idx) else {
        return;
    };
    let kind = tab.kind.clone();
    let api_version = tab.form.api_version.clone();
    let field_count = tab
        .form
        .rows
        .iter()
        .filter(|r| matches!(r.kind, Row::Field { .. }))
        .count();

    let btn_import_label = "↓ Import";
    let btn_fields_label = "+ Fields";
    let btn_validate_label = "✓ Validate";
    let btn_preview_label = "⎙ Preview & Save";

    let w_import = (btn_import_label.chars().count() as u16).saturating_add(4);
    let w_fields = (btn_fields_label.chars().count() as u16).saturating_add(4);
    let w_validate = (btn_validate_label.chars().count() as u16).saturating_add(4);
    let w_preview = (btn_preview_label.chars().count() as u16).saturating_add(4);
    let btn_gap = 2u16;
    let total_btns = w_import
        .saturating_add(w_fields)
        .saturating_add(w_validate)
        .saturating_add(w_preview)
        .saturating_add(btn_gap * 3);

    let text_max_w = if area.width > total_btns.saturating_add(10) {
        area.width.saturating_sub(total_btns.saturating_add(2))
    } else {
        area.width
    };

    ctx.paint(move |pctx| {
        let line = Line::from(vec![
            Span::styled(
                format!(" {kind}"),
                Style::default()
                    .fg(pctx.theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({api_version} • {field_count} fields)"),
                Style::default().fg(pctx.theme.muted_foreground),
            ),
        ]);
        let text_y = area.y.saturating_add(area.height / 2);
        let text_rect = Rect::new(area.x, text_y, text_max_w, 1);
        pctx.widget(Paragraph::new(line), text_rect);
    });

    if area.width > total_btns.saturating_add(10) {
        let btn_h = btn_size.height();
        let start_x = area.right().saturating_sub(total_btns);
        let r_import = Rect::new(start_x, area.y, w_import, btn_h);
        let r_fields = Rect::new(
            start_x.saturating_add(w_import).saturating_add(btn_gap),
            area.y,
            w_fields,
            btn_h,
        );
        let r_validate = Rect::new(
            r_fields.right().saturating_add(btn_gap),
            area.y,
            w_validate,
            btn_h,
        );
        let r_preview = Rect::new(
            r_validate.right().saturating_add(btn_gap),
            area.y,
            w_preview,
            btn_h,
        );

        ctx.component(
            "btn-import-cluster",
            Button::new(btn_import_label)
                .outline()
                .size(btn_size)
                .on_press(|| Msg::OpenClusterImportManifest),
            r_import,
        );
        ctx.component(
            "btn-fields",
            Button::new(btn_fields_label)
                .outline()
                .size(btn_size)
                .on_press(|| Msg::OpenFieldPicker),
            r_fields,
        );
        ctx.component(
            "btn-validate",
            Button::new(btn_validate_label)
                .outline()
                .size(btn_size)
                .on_press(|| Msg::ValidateForm),
            r_validate,
        );
        ctx.component(
            "btn-preview",
            Button::new(btn_preview_label)
                .size(btn_size)
                .on_press(|| Msg::OpenPreview),
            r_preview,
        );
    }
}

fn declare_form(ctx: &mut DeclareCtx<'_, App, Msg>, area: Rect) {
    let Some(idx) = ctx.state().active_tab_idx else {
        return;
    };
    let Some(tab) = ctx.state().tabs.get(idx) else {
        return;
    };

    let rows_snapshot = tab.form.rows.clone();
    let row_vis = tab.form.row_visibility();
    let collapsed_flags: Vec<bool> = (0..rows_snapshot.len())
        .map(|i| tab.form.is_category_collapsed(i))
        .collect();
    let added_field_flags: Vec<bool> = rows_snapshot
        .iter()
        .map(|r| match &r.kind {
            Row::Field { field, .. } => tab.form.is_added_field(&field.path),
            _ => false,
        })
        .collect();
    let added_category_flags: Vec<bool> = (0..rows_snapshot.len())
        .map(|i| tab.form.is_added_category_at(i))
        .collect();
    let focused_row_idx = tab.form.focus;
    let total_lines = (tab.form.total_lines() as u16).saturating_add(5);

    let scroll_area = ScrollArea::new(total_lines)
        .scroll(
            |app: &App| app.active_form().map(|f| f.scroll() as u16).unwrap_or(0),
            Msg::FormScrollChanged,
        )
        .content(move |ctx| {
            let content_rect = ctx.area();
            let mut y = content_rect.y;
            let mut seen_section = false;

            for (row_idx, row_view) in rows_snapshot.iter().enumerate() {
                if !row_vis.get(row_idx).copied().unwrap_or(true) {
                    continue;
                }
                let is_collapsed = collapsed_flags.get(row_idx).copied().unwrap_or(false);
                let is_added_cat = added_category_flags.get(row_idx).copied().unwrap_or(false);

                match &row_view.kind {
                    Row::Section(sec_name) => {
                        // Matches FormState::block_heights:
                        // 1st section = 2 rows (1 header + 1 blank below)
                        // subsequent sections = 3 rows (1 blank above + 1 header + 1 blank below)
                        if seen_section {
                            y = y.saturating_add(1);
                        }
                        seen_section = true;
                        let sec_rect =
                            Rect::new(content_rect.x, y, content_rect.width.saturating_sub(1), 1);
                        let title = sec_name.to_uppercase();
                        let arrow = if is_collapsed { "▸" } else { "▾" };
                        let state_tag = if is_collapsed { " (collapsed) " } else { " " };
                        let prefix = format!("{arrow} ── {title}{state_tag}");
                        let target_w = (sec_rect.width as usize).saturating_sub(2);
                        let rem = target_w.saturating_sub(prefix.chars().count());
                        let sec_label = format!("{prefix}{}", "─".repeat(rem));
                        ctx.component(
                            ChildId::from(format!("cat-sec-{row_idx}")),
                            Button::new(sec_label)
                                .ghost()
                                .on_press(move || Msg::ToggleFormCategory(row_idx)),
                            sec_rect,
                        );
                        y = y.saturating_add(2); // 1 header row + 1 blank spacing row below
                    }
                    Row::Group { label, depth } => {
                        // Matches FormState::block_heights: 2 rows (1 header + 1 blank below)
                        let max_indent = content_rect.width.saturating_sub(32);
                        let indent_cols = (*depth as u16).saturating_mul(4).min(max_indent);
                        let total_w = content_rect
                            .width
                            .saturating_sub(indent_cols)
                            .saturating_sub(1);
                        let rm_w = if is_added_cat && total_w > 28 {
                            12u16
                        } else {
                            0u16
                        };
                        let grp_w = if rm_w > 0 {
                            total_w.saturating_sub(rm_w.saturating_add(1))
                        } else {
                            total_w
                        };
                        let grp_rect =
                            Rect::new(content_rect.x.saturating_add(indent_cols), y, grp_w, 1);
                        let arrow = if is_collapsed { "▸" } else { "▾" };
                        let state_tag = if is_collapsed { " (collapsed) " } else { " " };
                        let prefix = format!("{arrow} ── {label}{state_tag}");
                        let target_w = (grp_rect.width as usize).saturating_sub(2);
                        let rem = target_w.saturating_sub(prefix.chars().count());
                        let grp_label = format!("{prefix}{}", "─".repeat(rem));
                        ctx.component(
                            ChildId::from(format!("cat-grp-{row_idx}")),
                            Button::new(grp_label)
                                .ghost()
                                .on_press(move || Msg::ToggleFormCategory(row_idx)),
                            grp_rect,
                        );
                        if rm_w > 0 {
                            let rm_rect = Rect::new(grp_rect.right().saturating_add(1), y, rm_w, 1);
                            ctx.component(
                                ChildId::from(format!("rm-cat-{row_idx}")),
                                Button::new("✕ Remove")
                                    .ghost()
                                    .on_press(move || Msg::RemoveCategory(row_idx)),
                                rm_rect,
                            );
                        }
                        y = y.saturating_add(2); // 1 group row + 1 blank spacing row below
                    }
                    Row::Array {
                        key,
                        path,
                        depth,
                        count,
                    } => {
                        // Matches FormState::block_heights: 4 rows (3-row strip + 1 blank row below)
                        let max_indent = content_rect.width.saturating_sub(32);
                        let indent_cols = (*depth as u16).saturating_mul(4).min(max_indent);
                        let arr_rect = Rect::new(
                            content_rect.x.saturating_add(indent_cols),
                            y,
                            content_rect
                                .width
                                .saturating_sub(indent_cols)
                                .saturating_sub(1),
                            3,
                        );
                        let key_str = key.clone();
                        let cnt = *count;
                        let add_label = "+ Add item";
                        let add_btn_w = (add_label.chars().count() as u16)
                            .saturating_add(4)
                            .min(arr_rect.width);

                        let rm_btn_label = if cnt > 1 {
                            Some("- Remove item")
                        } else if is_added_cat {
                            Some("✕ Remove")
                        } else {
                            None
                        };
                        let rm_btn_w = rm_btn_label
                            .map(|l| (l.chars().count() as u16).saturating_add(4))
                            .unwrap_or(0);
                        let total_btns_w = if rm_btn_w > 0 {
                            add_btn_w.saturating_add(rm_btn_w).saturating_add(1)
                        } else {
                            add_btn_w
                        };

                        let label_w = arr_rect
                            .width
                            .saturating_sub(total_btns_w.saturating_add(2));
                        let label_rect =
                            Rect::new(arr_rect.x, arr_rect.y.saturating_add(1), label_w, 1);
                        let add_btn_rect = Rect::new(
                            arr_rect.right().saturating_sub(total_btns_w),
                            arr_rect.y,
                            add_btn_w,
                            3,
                        );

                        let arrow = if is_collapsed { "▸" } else { "▾" };
                        let state_tag = if is_collapsed { " (collapsed) " } else { " " };
                        let item_word = if cnt == 1 { "item" } else { "items" };
                        let prefix = format!("{arrow} ── {key_str} [{cnt} {item_word}]{state_tag}");
                        let target_w = (label_w as usize).saturating_sub(2);
                        let rem = target_w.saturating_sub(prefix.chars().count());
                        let arr_label = format!("{prefix}{}", "─".repeat(rem));
                        ctx.component(
                            ChildId::from(format!("cat-arr-{row_idx}")),
                            Button::new(arr_label)
                                .ghost()
                                .on_press(move || Msg::ToggleFormCategory(row_idx)),
                            label_rect,
                        );

                        let arr_path = path.clone();
                        ctx.component(
                            ChildId::from(format!("arr-add-{row_idx}")),
                            Button::new(add_label)
                                .outline()
                                .size(ButtonSize::Large)
                                .on_press(move || Msg::AddArrayItemAt(arr_path.clone())),
                            add_btn_rect,
                        );

                        if let Some(rm_label) = rm_btn_label {
                            let rm_rect = Rect::new(
                                arr_rect.right().saturating_sub(rm_btn_w),
                                arr_rect.y,
                                rm_btn_w,
                                3,
                            );
                            if cnt > 1 {
                                let arr_path_rm = path.clone();
                                ctx.component(
                                    ChildId::from(format!("arr-rm-item-{row_idx}")),
                                    Button::new(rm_label)
                                        .outline()
                                        .size(ButtonSize::Large)
                                        .on_press(move || {
                                            Msg::RemoveArrayItemAt(arr_path_rm.clone())
                                        }),
                                    rm_rect,
                                );
                            } else {
                                ctx.component(
                                    ChildId::from(format!("rm-cat-{row_idx}")),
                                    Button::new(rm_label)
                                        .outline()
                                        .size(ButtonSize::Large)
                                        .on_press(move || Msg::RemoveCategory(row_idx)),
                                    rm_rect,
                                );
                            }
                        }
                        y = y.saturating_add(4); // 3 rows + 1 blank spacing row below
                    }
                    Row::Field { field, depth } => {
                        // Matches FormState::block_heights: 4 rows (3-row field box + 1 blank spacing row below)
                        let max_indent = content_rect.width.saturating_sub(32);
                        let indent_cols = (*depth as u16).saturating_mul(4).min(max_indent);
                        let field_x = content_rect.x.saturating_add(indent_cols);
                        let field_w = content_rect
                            .width
                            .saturating_sub(indent_cols)
                            .saturating_sub(1);
                        let is_added = added_field_flags.get(row_idx).copied().unwrap_or(false);

                        let rm_w = if is_added && field_w > 24 { 7u16 } else { 0u16 };
                        let box_w = if rm_w > 0 {
                            field_w.saturating_sub(rm_w.saturating_add(1))
                        } else {
                            field_w
                        };

                        let field_box_rect = Rect::new(field_x, y, box_w, 3);
                        let inner_control_rect = Rect::new(
                            field_x.saturating_add(1),
                            y.saturating_add(1),
                            box_w.saturating_sub(2),
                            1,
                        );

                        let label_text = field.label.clone();
                        let type_badge = field.kind.name();
                        let is_req = field.required;
                        let err_msg = row_view.error.clone();
                        let is_row_focused = row_idx == focused_row_idx;

                        let field_id = ChildId::from(format!("field-{row_idx}"));
                        match &field.kind {
                            FieldKind::Text | FieldKind::Integer | FieldKind::StringList => {
                                paint_field_container(
                                    ctx,
                                    field_box_rect,
                                    label_text,
                                    is_req,
                                    type_badge,
                                    err_msg,
                                    is_row_focused,
                                );
                                let path_read = field.path.clone();
                                let placeholder = match field.kind {
                                    FieldKind::Integer => "Enter integer...",
                                    FieldKind::StringList => "Comma-separated values...",
                                    _ => "Enter value...",
                                };
                                ctx.component(
                                    field_id,
                                    Input::new()
                                        .prefix("  ")
                                        .placeholder(placeholder)
                                        .invalid(row_view.error.is_some())
                                        .value(
                                            move |app: &App| {
                                                app.active_form()
                                                    .map(|f| f.input_state(&path_read))
                                                    .unwrap_or(&app.empty_input)
                                            },
                                            move |state| Msg::FormInputChanged { row_idx, state },
                                        )
                                        .on_submit(move || Msg::FormInputSubmit { row_idx }),
                                    inner_control_rect,
                                );
                            }
                            FieldKind::Boolean => {
                                paint_field_container(
                                    ctx,
                                    field_box_rect,
                                    label_text,
                                    is_req,
                                    type_badge,
                                    err_msg,
                                    is_row_focused,
                                );
                                let path_read = field.path.clone();
                                let checked_now = row_view.display == "true";
                                ctx.component(
                                    field_id,
                                    Checkbox::new(if checked_now {
                                        "true (enabled)"
                                    } else {
                                        "false (disabled)"
                                    })
                                    .checked_marker("[ON] ")
                                    .unchecked_marker("[off]")
                                    .checked(
                                        move |app: &App| {
                                            app.active_form()
                                                .and_then(|f| f.field_value(&path_read))
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false)
                                        },
                                        move |checked| Msg::FormBooleanChanged { row_idx, checked },
                                    ),
                                    inner_control_rect,
                                );
                            }
                            FieldKind::Map => {
                                paint_field_container(
                                    ctx,
                                    field_box_rect,
                                    label_text,
                                    is_req,
                                    type_badge,
                                    err_msg,
                                    is_row_focused,
                                );
                                let import_label = " ↓ Import from Cluster ";
                                let import_w = (import_label.chars().count() as u16 + 2)
                                    .min(inner_control_rect.width / 2);
                                let gap = 2u16;
                                let edit_w = inner_control_rect
                                    .width
                                    .saturating_sub(import_w.saturating_add(gap));
                                let edit_rect = Rect::new(
                                    inner_control_rect.x,
                                    inner_control_rect.y,
                                    edit_w,
                                    1,
                                );
                                let import_rect = Rect::new(
                                    inner_control_rect.right().saturating_sub(import_w),
                                    inner_control_rect.y,
                                    import_w,
                                    1,
                                );

                                let path_open = field.path.clone();
                                ctx.component(
                                    field_id,
                                    Button::new(format!(" ✎ Edit Map  {} ", row_view.display))
                                        .secondary()
                                        .on_press(move || Msg::OpenMapEditor(path_open.clone())),
                                    edit_rect,
                                );
                                let path_import = field.path.clone();
                                ctx.component(
                                    ChildId::from(format!("import-map-{row_idx}")),
                                    Button::new(import_label).secondary().on_press(move || {
                                        Msg::OpenClusterImport(path_import.clone())
                                    }),
                                    import_rect,
                                );
                            }
                            FieldKind::Enum(_) | FieldKind::Namespace => {
                                paint_field_container(
                                    ctx,
                                    field_box_rect,
                                    label_text.clone(),
                                    is_req,
                                    type_badge,
                                    err_msg,
                                    is_row_focused,
                                );
                                let dropdown_label = format_dropdown_label(
                                    inner_control_rect.width,
                                    &label_text,
                                    &row_view.display,
                                );
                                let path_pick = field.path.clone();
                                ctx.component(
                                    field_id,
                                    Button::new(dropdown_label)
                                        .secondary()
                                        .on_press(move || Msg::OpenSelectPicker(path_pick.clone())),
                                    inner_control_rect,
                                );
                            }
                        }

                        if rm_w > 0 {
                            let rm_rect =
                                Rect::new(field_box_rect.right().saturating_add(1), y, rm_w, 3);
                            let path_rm = field.path.clone();
                            ctx.component(
                                ChildId::from(format!("rm-field-{row_idx}")),
                                Button::new("✕")
                                    .outline()
                                    .size(ButtonSize::Large)
                                    .on_press(move || Msg::RemoveField(path_rm.clone())),
                                rm_rect,
                            );
                        }

                        y = y.saturating_add(4); // 3 rows for field box + 1 blank spacing row
                    }
                }
            }

            // Bottom "+ Add Optional Schema Fields" large 3-row button inside the form scroll area
            y = y.saturating_add(1);
            let add_fields_label = "+ Add Optional Schema Fields (Ctrl+F)";
            let add_w = (add_fields_label.chars().count() as u16)
                .saturating_add(4)
                .min(content_rect.width);
            let add_rect = Rect::new(content_rect.x, y, add_w, 3);
            ctx.component(
                "add-fields-bottom",
                Button::new(add_fields_label)
                    .outline()
                    .size(ButtonSize::Large)
                    .on_press(|| Msg::OpenFieldPicker),
                add_rect,
            );
        });

    ctx.component("form", scroll_area, area);
}

/// Formats a full-width dropdown trigger button label with a right-aligned `▾` chevron.
fn format_dropdown_label(width: u16, field_label: &str, display_val: &str) -> String {
    let val_str = if display_val.is_empty() {
        format!("Select {field_label}... (unset)")
    } else {
        display_val.to_string()
    };
    let target_w = (width as usize).saturating_sub(2);
    let arrow = " ▾";
    let max_val_len = target_w.saturating_sub(arrow.chars().count());
    let clipped: String = val_str.chars().take(max_val_len).collect();
    let pad = max_val_len.saturating_sub(clipped.chars().count());
    format!("{clipped}{}{arrow}", " ".repeat(pad))
}

/// Paints a 3-row rounded titled field container matching `ratcn::Input::title(...)`.
fn paint_field_container(
    ctx: &mut DeclareCtx<'_, App, Msg>,
    rect: Rect,
    label_text: String,
    is_req: bool,
    type_badge: &'static str,
    err_msg: Option<String>,
    is_row_focused: bool,
) {
    ctx.paint(move |pctx| {
        let border_color = if err_msg.is_some() {
            pctx.theme.destructive
        } else if is_row_focused {
            pctx.theme.ring
        } else {
            pctx.theme.border
        };
        let label_style = if is_row_focused {
            Style::default()
                .fg(pctx.theme.primary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(pctx.theme.foreground)
        };
        let mut spans = vec![Span::styled(format!(" {label_text}"), label_style)];
        if is_req {
            spans.push(Span::styled(
                " *",
                Style::default()
                    .fg(pctx.theme.destructive)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        spans.push(Span::styled(
            format!(" [{type_badge}] "),
            Style::default().fg(pctx.theme.muted_foreground),
        ));
        if let Some(err) = err_msg {
            spans.push(Span::styled(
                format!("⚠ {err} "),
                Style::default()
                    .fg(pctx.theme.destructive)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(pctx.theme.field))
            .title(Line::from(spans));
        pctx.widget(block, rect);
    });
}

// ---------------------------------------------------------------------------
// Footer Status & Clickable Keybindings Bar
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum FooterAction {
    ModalNext,
    ModalMoveDown,
    ModalSubmit,
    CloseModal,
    NextField,
    FocusSidebarFilter,
    OpenFieldPicker,
    RemoveFocusedField,
    OpenContextSwitcher,
    ValidateForm,
    OpenPreview,
    CloseActiveTab,
    CycleTheme,
    RequestQuit,
    NextResource,
    OpenSelectedResource,
    SwitchPane,
}

fn footer_items(
    has_modal: bool,
    has_tab: bool,
) -> &'static [(&'static str, &'static str, FooterAction)] {
    if has_modal {
        &[
            ("Tab", "Next", FooterAction::ModalNext),
            ("↑↓", "Move", FooterAction::ModalMoveDown),
            ("Enter", "Select/Submit", FooterAction::ModalSubmit),
            ("Esc", "Close Modal", FooterAction::CloseModal),
        ]
    } else if has_tab {
        &[
            ("Tab/↑↓", "Fields", FooterAction::NextField),
            ("/", "Filter", FooterAction::FocusSidebarFilter),
            ("+/^F", "Add Fields", FooterAction::OpenFieldPicker),
            ("^D", "Remove", FooterAction::RemoveFocusedField),
            ("c", "Context", FooterAction::OpenContextSwitcher),
            ("^V", "Validate", FooterAction::ValidateForm),
            ("p/^S", "Preview & Save", FooterAction::OpenPreview),
            ("^W", "Close", FooterAction::CloseActiveTab),
            ("t", "Theme", FooterAction::CycleTheme),
            ("q", "Quit", FooterAction::RequestQuit),
        ]
    } else {
        &[
            ("↑↓/jk", "Navigate", FooterAction::NextResource),
            ("Enter", "Open Resource", FooterAction::OpenSelectedResource),
            ("/", "Filter Kinds", FooterAction::FocusSidebarFilter),
            ("c / ^K", "Context", FooterAction::OpenContextSwitcher),
            ("Tab", "Switch Pane", FooterAction::SwitchPane),
            ("t", "Theme", FooterAction::CycleTheme),
            ("q", "Quit", FooterAction::RequestQuit),
        ]
    }
}

fn is_on_footer_row(app: &App, row: u16) -> bool {
    let (_, fy, _) = app.footer_rect.get();
    fy != u16::MAX && row == fy
}

fn handle_footer_click(ratcn: &mut Ratcn<App, Msg>, app: &mut App, col: u16, row: u16) -> bool {
    let (fx, fy, fw) = app.footer_rect.get();
    if fy == u16::MAX || row != fy || col < fx || col >= fx.saturating_add(fw) {
        return false;
    }

    let has_modal = app.modals.top().is_some();
    let has_tab = app.active_tab_idx.is_some();
    let items = footer_items(has_modal, has_tab);

    let mut x = fx.saturating_add(1); // leading " "
    for (i, (k, desc, action)) in items.iter().enumerate() {
        if i > 0 {
            x = x.saturating_add(2); // "  " separator
        }
        let item_w = (k.chars().count() + 2 + 1 + desc.chars().count()) as u16;
        if col >= x && col < x.saturating_add(item_w) {
            trigger_footer_action(ratcn, app, *action);
            return true;
        }
        x = x.saturating_add(item_w);
    }
    false
}

fn trigger_footer_action(ratcn: &mut Ratcn<App, Msg>, app: &mut App, action: FooterAction) {
    match action {
        FooterAction::ModalNext | FooterAction::SwitchPane => {
            handle_event(ratcn, app, Event::Key(KeyEvent::new(KeyCode::Tab)));
        }
        FooterAction::ModalMoveDown => {
            handle_event(ratcn, app, Event::Key(KeyEvent::new(KeyCode::Down)));
        }
        FooterAction::ModalSubmit => {
            handle_event(ratcn, app, Event::Key(KeyEvent::new(KeyCode::Enter)));
        }
        FooterAction::CloseModal => app.update(Msg::CloseModal),
        FooterAction::NextField => {
            if let Some(form) = app.active_form_mut() {
                form.focus_next_field(true);
                let idx = form.focus;
                app.focus = FocusState::intent([
                    ChildId::Static("workspace"),
                    ChildId::Static("form"),
                    ChildId::from(format!("field-{idx}")),
                ]);
                app.ui_dirty = true;
            }
        }
        FooterAction::FocusSidebarFilter => {
            app.focus = FocusState::intent(["sidebar", "filter"]);
            app.ui_dirty = true;
        }
        FooterAction::OpenFieldPicker => app.update(Msg::OpenFieldPicker),
        FooterAction::RemoveFocusedField => app.update(Msg::RemoveFocusedField),
        FooterAction::OpenContextSwitcher => app.update(Msg::OpenContextSwitcher),
        FooterAction::ValidateForm => app.update(Msg::ValidateForm),
        FooterAction::OpenPreview => app.update(Msg::OpenPreview),
        FooterAction::CloseActiveTab => app.update(Msg::CloseActiveTab),
        FooterAction::CycleTheme => app.update(Msg::CycleTheme),
        FooterAction::RequestQuit => app.update(Msg::RequestQuit),
        FooterAction::NextResource => {
            app.select_next_resource();
            app.focus = FocusState::intent(["sidebar", "resources"]);
        }
        FooterAction::OpenSelectedResource => {
            app.open_selected_resource();
        }
    }
}

fn declare_footer(ctx: &mut DeclareCtx<'_, App, Msg>, area: Rect) {
    ctx.state().footer_rect.set((area.x, area.y, area.width));

    let has_modal = ctx.state().modals.top().is_some();
    let has_tab = ctx.state().active_tab_idx.is_some();

    ctx.paint(move |pctx| {
        let surface = pctx.theme.surface;
        let key_bg = pctx.theme.secondary;
        let key_fg = pctx.theme.secondary_foreground;
        let muted = pctx.theme.muted_foreground;

        let pairs = footer_items(has_modal, has_tab);

        let mut spans = Vec::new();
        spans.push(Span::raw(" "));
        for (i, (k, desc, _)) in pairs.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(
                format!(" {k} "),
                Style::default()
                    .bg(key_bg)
                    .fg(key_fg)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(format!(" {desc}"), Style::default().fg(muted)));
        }

        pctx.with_buffer(area, |rect, buf| {
            buf.set_style(rect, Style::default().bg(surface));
            Paragraph::new(Line::from(spans)).render(rect, buf);
        });
    });
}

// ---------------------------------------------------------------------------
// Modal Layer 1: Confirm Dialog
// ---------------------------------------------------------------------------

fn declare_confirm_modal(ctx: &mut DeclareCtx<'_, App, Msg>) {
    let cm = ctx.state().confirm_modal.clone();
    let dialog = Dialog::new()
        .title(cm.title)
        .description(cm.message)
        .outer_width(60)
        .on_dismiss(|| Msg::CloseModal)
        .action(
            "cancel",
            Button::new("Cancel")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::CloseModal),
        )
        .action(
            "confirm",
            Button::new(cm.confirm_label)
                .destructive()
                .size(ButtonSize::Large)
                .on_press(|| Msg::ConfirmAccepted),
        );
    ctx.modal(MODAL_CONFIRM, dialog, ctx.area());
}

// ---------------------------------------------------------------------------
// Modal Layer 2: Field Picker Dialog
// ---------------------------------------------------------------------------

fn declare_field_picker_modal(ctx: &mut DeclareCtx<'_, App, Msg>) {
    let fp = &ctx.state().field_picker_modal;
    let rows = fp.tree_rows();
    let selected_count = fp.selected.len();
    let kind_name = fp.kind.clone();

    let items: Vec<ListItem<String>> = rows
        .iter()
        .map(|r| ListItem::new(r.id().to_string(), r.id().to_string()))
        .collect();

    let row_map: HashMap<String, FieldPickerRow> =
        rows.into_iter().map(|r| (r.id().to_string(), r)).collect();

    let primary_fg = ctx.state().theme().primary;
    let muted_fg = ctx.state().theme().muted_foreground;
    let fg = ctx.state().theme().foreground;
    let destructive_fg = ctx.state().theme().destructive;

    let add_label = if selected_count > 0 {
        format!("Add Selected ({selected_count})")
    } else if let Some(cur) = &fp.cursor {
        if let Some(group_path) = cur.strip_prefix(FIELD_PICKER_GROUP_PREFIX) {
            let prefix = format!("{group_path}.");
            let count = fp
                .available
                .iter()
                .filter(|l| l.path == group_path || l.path.starts_with(&prefix))
                .count();
            format!("Add Category ({count})")
        } else {
            "Add Focused Field".to_string()
        }
    } else {
        "Add Field".to_string()
    };

    let outer_w = (ctx.area().width.saturating_sub(4)).clamp(84, 112);
    let dialog = Dialog::new()
        .title(format!("Add Optional Fields — {kind_name}"))
        .outer_width(outer_w)
        .on_dismiss(|| Msg::CloseModal)
        .content(18, move |ctx| {
            let body = ctx.area();
            if body.height < 5 {
                return;
            }
            let filter_rect = Rect::new(body.x, body.y, body.width, 3);
            let list_rect = Rect::new(
                body.x,
                body.y.saturating_add(4), // 1 blank row of spacing below the 3-row search input
                body.width,
                body.height.saturating_sub(4),
            );
            ctx.component(
                "filter",
                Input::new()
                    .prefix("  ")
                    .title(" Search schema fields or categories ")
                    .placeholder("Filter by field path or type...")
                    .value(
                        |app: &App| &app.field_picker_modal.filter,
                        Msg::FieldPickerFilterChanged,
                    )
                    .on_submit(|| Msg::FieldPickerCommit),
                filter_rect,
            );
            ctx.component(
                "list",
                List::new(items)
                    .focus_symbol("▸ ")
                    .item_focus(
                        |app: &App| app.field_picker_modal.cursor.clone(),
                        Msg::FieldPickerCursorChanged,
                    )
                    .multi_selection(
                        |app: &App, id: &String| {
                            if id.starts_with(FIELD_PICKER_GROUP_PREFIX) {
                                false
                            } else {
                                app.field_picker_modal.selected.contains(id)
                            }
                        },
                        Msg::FieldPickerToggle,
                    )
                    .paint_item(move |_app: &App, row| {
                        if let Some(item) = row_map.get(row.value) {
                            match item {
                                FieldPickerRow::Category {
                                    label,
                                    depth,
                                    expanded,
                                    is_array,
                                    total_leaves,
                                    selected_leaves,
                                    ..
                                } => {
                                    let indent = "  ".repeat(*depth);
                                    let arrow = if *expanded { "▾ " } else { "▸ " };
                                    let icon = if *is_array { "📦 " } else { "📁 " };
                                    let count_str = if *selected_leaves > 0 {
                                        format!("  ({selected_leaves}/{total_leaves} selected)")
                                    } else {
                                        format!("  ({total_leaves} fields)")
                                    };
                                    Text::from(Line::from(vec![
                                        Span::raw(indent),
                                        Span::styled(arrow, Style::default().fg(muted_fg)),
                                        Span::styled(icon, Style::default().fg(primary_fg)),
                                        Span::styled(
                                            label.clone(),
                                            Style::default().fg(fg).add_modifier(Modifier::BOLD),
                                        ),
                                        Span::styled(count_str, Style::default().fg(muted_fg)),
                                    ]))
                                }
                                FieldPickerRow::Leaf {
                                    label,
                                    field_type,
                                    required,
                                    depth,
                                    selected,
                                    ..
                                } => {
                                    let indent = "  ".repeat(*depth);
                                    let check = if *selected { "[✓] " } else { "[ ] " };
                                    let check_style = if *selected {
                                        Style::default().fg(primary_fg).add_modifier(Modifier::BOLD)
                                    } else {
                                        Style::default().fg(muted_fg)
                                    };
                                    let req_span = if *required {
                                        Span::styled(" *", Style::default().fg(destructive_fg))
                                    } else {
                                        Span::raw("")
                                    };
                                    Text::from(Line::from(vec![
                                        Span::raw(indent),
                                        Span::styled(check, check_style),
                                        Span::styled(label.clone(), Style::default().fg(fg)),
                                        req_span,
                                        Span::styled(
                                            format!("  [{field_type}]"),
                                            Style::default().fg(muted_fg),
                                        ),
                                    ]))
                                }
                            }
                        } else {
                            Text::from(Line::from(Span::raw(row.label.to_string())))
                        }
                    }),
                list_rect,
            );
        })
        .action(
            "cancel",
            Button::new("Cancel")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::CloseModal),
        )
        .action(
            "toggle_group",
            Button::new("± Select Group (a)")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::FieldPickerToggleGroup),
        )
        .action(
            "add",
            Button::new(add_label)
                .size(ButtonSize::Large)
                .on_press(|| Msg::FieldPickerCommit),
        );

    ctx.modal(MODAL_FIELD_PICKER, dialog, ctx.area());
}

// ---------------------------------------------------------------------------
// Modal Layer 3: YAML Preview & Save Dialog
// ---------------------------------------------------------------------------

fn declare_preview_modal(ctx: &mut DeclareCtx<'_, App, Msg>) {
    let yaml = ctx.state().preview_modal.yaml.clone();
    let line_count = (yaml.lines().count() as u16).max(1);

    let outer_w = (ctx.area().width.saturating_sub(4)).clamp(88, 116);
    let dialog = Dialog::new()
        .title("YAML Preview & Save Manifest")
        .outer_width(outer_w)
        .on_dismiss(|| Msg::CloseModal)
        .content(18, move |ctx| {
            let body = ctx.area();
            if body.height < 6 {
                return;
            }
            let scroll_h = body.height.saturating_sub(4);
            let yaml_rect = Rect::new(body.x, body.y, body.width, scroll_h);
            // 1 blank row of spacing between YAML scroll area and path Input
            let path_rect = Rect::new(
                body.x,
                body.y.saturating_add(scroll_h).saturating_add(1),
                body.width,
                3,
            );

            let yaml_for_paint = yaml.clone();
            ctx.component(
                "yaml-scroll",
                ScrollArea::new(line_count)
                    .scroll(
                        |app: &App| app.preview_modal.scroll,
                        Msg::PreviewScrollChanged,
                    )
                    .content(move |ctx| {
                        let content_area = ctx.area();
                        let yaml_owned = yaml_for_paint.clone();
                        ctx.paint(move |pctx| {
                            paint_highlighted_yaml(pctx, content_area, &yaml_owned);
                        });
                    }),
                yaml_rect,
            );

            ctx.component(
                "path-input",
                Input::new()
                    .prefix("  ")
                    .title(" Destination file path ")
                    .placeholder("~/path/to/manifest.yaml")
                    .value(
                        |app: &App| &app.preview_modal.path_input,
                        Msg::PreviewPathChanged,
                    )
                    .on_submit(|| Msg::PreviewSave),
                path_rect,
            );
        })
        .action(
            "close",
            Button::new("Close")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::CloseModal),
        )
        .action(
            "browse",
            Button::new("Browse…")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::OpenDirPicker),
        )
        .action(
            "save",
            Button::new("Save YAML")
                .size(ButtonSize::Large)
                .on_press(|| Msg::PreviewSave),
        );

    ctx.modal(MODAL_PREVIEW, dialog, ctx.area());
}

fn paint_highlighted_yaml(pctx: &mut PaintCtx<'_, App>, area: Rect, yaml: &str) {
    let key_color = pctx.theme.primary;
    let val_color = pctx.theme.accent;
    let num_color = pctx.theme.warning;
    let muted = pctx.theme.muted_foreground;
    let well_bg = pctx.theme.field;

    let lines: Vec<Line<'static>> = yaml
        .lines()
        .enumerate()
        .map(|(i, raw_line)| {
            let line_no = format!("{:>3} │ ", i + 1);
            let mut spans = vec![Span::styled(line_no, Style::default().fg(muted))];
            if let Some(colon_pos) = raw_line.find(':') {
                let (k, rest) = raw_line.split_at(colon_pos + 1);
                spans.push(Span::styled(
                    k.to_string(),
                    Style::default().fg(key_color).add_modifier(Modifier::BOLD),
                ));
                let trimmed = rest.trim();
                let v_color =
                    if trimmed == "true" || trimmed == "false" || trimmed.parse::<i64>().is_ok() {
                        num_color
                    } else {
                        val_color
                    };
                spans.push(Span::styled(rest.to_string(), Style::default().fg(v_color)));
            } else {
                spans.push(Span::styled(
                    raw_line.to_string(),
                    Style::default().fg(val_color),
                ));
            }
            Line::from(spans)
        })
        .collect();

    pctx.with_buffer(area, |rect, buf| {
        buf.set_style(rect, Style::default().bg(well_bg));
        Paragraph::new(lines).render(rect, buf);
    });
}

// ---------------------------------------------------------------------------
// Modal Layer 4: Directory Picker Dialog
// ---------------------------------------------------------------------------

fn declare_dir_picker_modal(ctx: &mut DeclareCtx<'_, App, Msg>) {
    let dp = &ctx.state().dir_picker_modal;
    let dir_display = crate::modals::shorten_home_path(&dp.current_dir);
    let mut items: Vec<ListItem<String>> = vec![ListItem::new(
        crate::modals::DIR_PICKER_CURRENT_TOKEN.to_string(),
        format!("📍 .  (Save in {dir_display})"),
    )];
    if dp.has_parent() {
        items.push(ListItem::new(
            crate::modals::DIR_PICKER_PARENT_TOKEN.to_string(),
            "📁 .. (Parent directory)".to_string(),
        ));
    }
    for p in &dp.entries {
        if let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_string()) {
            items.push(ListItem::new(name.clone(), format!("📁 {name}/")));
        }
    }

    let outer_w = (ctx.area().width.saturating_sub(8)).clamp(72, 104);
    let dialog = Dialog::new()
        .title(format!("Browse Directory — {dir_display}"))
        .outer_width(outer_w)
        .on_dismiss(|| Msg::CloseModal)
        .content(12, move |ctx| {
            let body = ctx.area();
            ctx.component(
                "dirs",
                List::new(items)
                    .focus_symbol("▸ ")
                    .selected_marker("")
                    .unselected_marker("")
                    .item_focus(
                        |app: &App| app.dir_picker_modal.cursor.clone(),
                        Msg::DirPickerCursorChanged,
                    )
                    .selection(|_app: &App| None, Msg::DirPickerEnterDir),
                body,
            );
        })
        .action(
            "cancel",
            Button::new("Cancel")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::CloseModal),
        )
        .action(
            "up",
            Button::new("↑ Parent Dir")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::DirPickerUp),
        )
        .action(
            "select",
            Button::new("Select Directory")
                .size(ButtonSize::Large)
                .on_press(|| Msg::DirPickerSelectCurrent),
        );

    ctx.modal(MODAL_DIR_PICKER, dialog, ctx.area());
}

// ---------------------------------------------------------------------------
// Modal Layer 5: Map Editor Dialog (Large Key/Value Inputs + Spacing)
// ---------------------------------------------------------------------------

fn declare_map_editor_modal(ctx: &mut DeclareCtx<'_, App, Msg>) {
    let me = &ctx.state().map_editor_modal;
    let title = me.title.clone();
    let row_ids: Vec<(usize, ChildId)> = me
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| (i, r.id.clone()))
        .collect();
    // Each row is 3 rows tall + 1 row of vertical spacing (stride = 4)
    let content_h = (row_ids.len() as u16).saturating_mul(4).max(4);

    let outer_w = (ctx.area().width.saturating_sub(4)).clamp(92, 120);
    let dialog = Dialog::new()
        .title(title)
        .outer_width(outer_w)
        .on_dismiss(|| Msg::CloseModal)
        .content(13, move |ctx| {
            let body = ctx.area();
            if body.height < 4 {
                return;
            }

            ctx.component(
                "scroll",
                ScrollArea::new(content_h)
                    .scroll(
                        |app: &App| app.map_editor_modal.scroll,
                        Msg::MapEditorScrollChanged,
                    )
                    .content(move |ctx| {
                        let area = ctx.area();
                        let del_w = 5u16.min(area.width);
                        let gap = 2u16;
                        let col_w = area.width.saturating_sub(del_w.saturating_add(gap * 2)) / 2;
                        for (row_idx, row_id) in row_ids {
                            let y = area.y.saturating_add((row_idx as u16).saturating_mul(4));
                            let row_rect = Rect::new(area.x, y, area.width, 3);
                            ctx.scope(
                                row_id,
                                row_rect,
                                ScopeOptions::default().tab_wrap(TabWrap::Escape),
                                move |ctx| {
                                    let r = ctx.area();
                                    let key_rect = Rect::new(r.x, r.y, col_w, 3);
                                    let val_rect = Rect::new(
                                        r.x.saturating_add(col_w).saturating_add(gap),
                                        r.y,
                                        col_w,
                                        3,
                                    );
                                    let del_rect =
                                        Rect::new(r.right().saturating_sub(del_w), r.y, del_w, 3);

                                    ctx.component(
                                        "key",
                                        Input::new()
                                            .prefix("  ")
                                            .title(format!(" Key #{} ", row_idx + 1))
                                            .placeholder("key")
                                            .value(
                                                move |app: &App| {
                                                    app.map_editor_modal
                                                        .rows
                                                        .get(row_idx)
                                                        .map(|e| &e.key)
                                                        .unwrap_or(&app.empty_input)
                                                },
                                                move |state| Msg::MapEditorKeyChanged {
                                                    row_idx,
                                                    state,
                                                },
                                            ),
                                        key_rect,
                                    );
                                    ctx.component(
                                        "val",
                                        Input::new()
                                            .prefix("  ")
                                            .title(format!(" Value #{} ", row_idx + 1))
                                            .placeholder("value")
                                            .value(
                                                move |app: &App| {
                                                    app.map_editor_modal
                                                        .rows
                                                        .get(row_idx)
                                                        .map(|e| &e.value)
                                                        .unwrap_or(&app.empty_input)
                                                },
                                                move |state| Msg::MapEditorValueChanged {
                                                    row_idx,
                                                    state,
                                                },
                                            ),
                                        val_rect,
                                    );
                                    ctx.component(
                                        "del",
                                        Button::new("✕")
                                            .destructive()
                                            .size(ButtonSize::Large)
                                            .on_press(move || Msg::MapEditorRemoveRow(row_idx)),
                                        del_rect,
                                    );
                                },
                            );
                        }
                    }),
                body,
            );
        })
        .action(
            "cancel",
            Button::new("Cancel")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::CloseModal),
        )
        .action(
            "import",
            Button::new("↓ Import from Cluster")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::OpenClusterImportFromMapEditor),
        )
        .action(
            "add",
            Button::new("+ Add Entry")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::MapEditorAddRow),
        )
        .action(
            "save",
            Button::new("Apply Map")
                .size(ButtonSize::Large)
                .on_press(|| Msg::MapEditorSave),
        );

    ctx.modal(MODAL_MAP_EDITOR, dialog, ctx.area());
}

// ---------------------------------------------------------------------------
// Modal Layer 6: Select Option Picker Dialog (Dropdown List)
// ---------------------------------------------------------------------------

fn declare_select_picker_modal(ctx: &mut DeclareCtx<'_, App, Msg>) {
    let sp = &ctx.state().select_picker_modal;
    let title = format!("▾ {}", sp.title);
    let mut items = vec![ListItem::new(
        SELECT_UNSET_VALUE.to_string(),
        "(unset / clear)".to_string(),
    )];
    items.extend(
        sp.options
            .iter()
            .map(|opt| ListItem::new(opt.clone(), opt.clone())),
    );

    let dialog = Dialog::new()
        .title(title)
        .outer_width(58)
        .on_dismiss(|| Msg::CloseModal)
        .content(10, move |ctx| {
            let body = ctx.area();
            ctx.component(
                "options",
                List::new(items)
                    .focus_symbol("▸ ")
                    .item_focus(
                        |app: &App| app.select_picker_modal.cursor.clone(),
                        Msg::SelectPickerCursorChanged,
                    )
                    .selection(
                        |app: &App| app.select_picker_modal.selected.clone(),
                        Msg::SelectPickerCommit,
                    ),
                body,
            );
        })
        .action(
            "cancel",
            Button::new("Cancel")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::CloseModal),
        )
        .action(
            "clear",
            Button::new("Clear Value")
                .destructive()
                .size(ButtonSize::Large)
                .on_press(|| Msg::SelectPickerClear),
        );

    ctx.modal(MODAL_SELECT_PICKER, dialog, ctx.area());
}

// ---------------------------------------------------------------------------
// Modal Layer 7: Import from Cluster Dialog (ktui ClusterImportScreen)
// ---------------------------------------------------------------------------

fn declare_cluster_import_modal(ctx: &mut DeclareCtx<'_, App, Msg>) {
    let ci = &ctx.state().cluster_import_modal;
    let dialog_title = ci.dialog_title();
    let status_msg = ci.status_message.clone();
    let is_loading = ci.is_fetching;
    let target_path = ci.target_path.clone().unwrap_or_default();
    let muted_fg = ctx.state().theme().muted_foreground;

    let kind_tabs: Vec<Tab<String>> = ClusterImportModalState::KINDS
        .iter()
        .map(|k| Tab::new((*k).to_string(), (*k).to_string()))
        .collect();

    let filtered_indices = ci.filtered_indices();
    let mut preview_map: HashMap<String, String> = HashMap::new();
    let items: Vec<ListItem<String>> = filtered_indices
        .iter()
        .filter_map(|&i| {
            let item = ci.items.get(i)?;
            let name = item
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("unnamed");
            let ns = item
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            let id = i.to_string();
            let label = format!("{ns}/{name}");
            let extracted = extract_map_from_cluster_item(item, &target_path);
            let summary = if extracted.is_empty() {
                "(no labels)".to_string()
            } else {
                let pairs: Vec<String> = extracted
                    .iter()
                    .take(3)
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect();
                let extra = if extracted.len() > 3 {
                    format!(", +{}", extracted.len() - 3)
                } else {
                    String::new()
                };
                format!("{{{pairs_str}{extra}}}", pairs_str = pairs.join(", "))
            };
            preview_map.insert(id.clone(), summary);
            Some(ListItem::new(id, label))
        })
        .collect();

    let outer_w = (ctx.area().width.saturating_sub(4)).clamp(90, 118);
    let dialog = Dialog::new()
        .title(dialog_title)
        .outer_width(outer_w)
        .on_dismiss(|| Msg::CloseModal)
        .content(18, move |ctx| {
            let body = ctx.area();
            if body.height < 8 {
                return;
            }
            let tabs_rect = Rect::new(body.x, body.y, body.width, 3);
            let filter_rect = Rect::new(body.x, body.y.saturating_add(3), body.width, 3);
            let status_rect = Rect::new(body.x, body.y.saturating_add(6), body.width, 1);
            let list_rect = Rect::new(
                body.x,
                body.y.saturating_add(7),
                body.width,
                body.height.saturating_sub(7),
            );

            ctx.component(
                "filter",
                Input::new()
                    .prefix("  ")
                    .title(" Search cluster resources ")
                    .placeholder("Filter by name or namespace...")
                    .value(
                        |app: &App| &app.cluster_import_modal.filter,
                        Msg::ClusterImportFilterChanged,
                    )
                    .on_submit(|| Msg::ClusterImportCommitSelected),
                filter_rect,
            );

            ctx.component(
                "kinds",
                Tabs::new(kind_tabs)
                    .size(TabsSize::Large)
                    .selection(
                        |app: &App| Some(app.cluster_import_modal.selected_kind.clone()),
                        Msg::ClusterImportKindSelected,
                    )
                    .activation(TabsActivation::Automatic),
                tabs_rect,
            );

            let status_text = status_msg.clone();
            ctx.paint(move |pctx| {
                let color = if is_loading {
                    pctx.theme.warning
                } else {
                    pctx.theme.muted_foreground
                };
                pctx.widget(
                    Paragraph::new(Line::styled(status_text, Style::default().fg(color))),
                    status_rect,
                );
            });

            if !items.is_empty() && list_rect.height > 0 {
                ctx.component(
                    "resources",
                    List::new(items)
                        .focus_symbol("▸ ")
                        .selected_marker("● ")
                        .unselected_marker("  ")
                        .item_focus(
                            |app: &App| app.cluster_import_modal.cursor.clone(),
                            Msg::ClusterImportCursorChanged,
                        )
                        .selection(|_app: &App| None, Msg::ClusterImportCommit)
                        .paint_item(move |_app: &App, row| {
                            let summary =
                                preview_map.get(row.value).map(String::as_str).unwrap_or("");
                            Text::from(Line::from(vec![
                                Span::styled(
                                    format!("{:<32}", row.label),
                                    Style::default().add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(format!("  {summary}"), Style::default().fg(muted_fg)),
                            ]))
                        }),
                    list_rect,
                );
            }
        })
        .action(
            "cancel",
            Button::new("Cancel")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::CloseModal),
        )
        .action(
            "refresh",
            Button::new("⟳ Refresh")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::ClusterImportRefresh),
        )
        .action(
            "import",
            Button::new("↓ Import Selected")
                .size(ButtonSize::Large)
                .on_press(|| Msg::ClusterImportCommitSelected),
        );

    ctx.modal(MODAL_CLUSTER_IMPORT, dialog, ctx.area());
}

// ---------------------------------------------------------------------------
// Modal Layer 8: Kubernetes Context Switcher Dialog
// ---------------------------------------------------------------------------

fn declare_context_switcher_modal(ctx: &mut DeclareCtx<'_, App, Msg>) {
    let cs = &ctx.state().context_switcher_modal;
    let filtered = cs.filtered_contexts();
    let current_ctx = cs.current_context.clone();
    let outer_w = (ctx.area().width.saturating_sub(4)).clamp(76, 102);
    let muted_fg = ctx.state().theme().muted_foreground;

    let items: Vec<ListItem<String>> = filtered
        .iter()
        .map(|name| {
            let label = if Some(name) == current_ctx.as_ref() {
                format!("● {name} (current)")
            } else {
                format!("○ {name}")
            };
            ListItem::new(name.clone(), label)
        })
        .collect();

    let cur_ctx_for_paint = current_ctx.clone();

    let dialog = Dialog::new()
        .title("Switch Kubernetes Context")
        .outer_width(outer_w)
        .on_dismiss(|| Msg::CloseModal)
        .content(14, move |ctx| {
            let body = ctx.area();
            if body.height < 5 {
                return;
            }
            let filter_rect = Rect::new(body.x, body.y, body.width, 3);
            let list_rect = Rect::new(
                body.x,
                body.y.saturating_add(4),
                body.width,
                body.height.saturating_sub(4),
            );

            ctx.component(
                "filter",
                Input::new()
                    .prefix("  ")
                    .title(" Search contexts ")
                    .placeholder("Filter kube context name...")
                    .value(
                        |app: &App| &app.context_switcher_modal.filter,
                        Msg::ContextSwitcherFilterChanged,
                    )
                    .on_submit(|| Msg::ContextSwitcherCommitSelected),
                filter_rect,
            );

            let cur_ctx_ref = cur_ctx_for_paint.clone();
            ctx.component(
                "list",
                List::new(items)
                    .focus_symbol("▸ ")
                    .item_focus(
                        |app: &App| app.context_switcher_modal.cursor.clone(),
                        Msg::ContextSwitcherCursorChanged,
                    )
                    .selection(|_app: &App| None, Msg::ContextSwitcherCommit)
                    .paint_item(move |_app: &App, row| {
                        let is_current = cur_ctx_ref.as_deref() == Some(row.value);
                        if is_current {
                            Text::from(Line::from(vec![
                                Span::styled(
                                    "● ",
                                    Style::default()
                                        .fg(Color::Rgb(74, 222, 128))
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(
                                    row.value.to_string(),
                                    Style::default().add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(" (current)", Style::default().fg(muted_fg)),
                            ]))
                        } else {
                            Text::from(Line::from(vec![
                                Span::styled("○ ", Style::default().fg(muted_fg)),
                                Span::styled(row.value.to_string(), Style::default()),
                            ]))
                        }
                    }),
                list_rect,
            );
        })
        .action(
            "cancel",
            Button::new("Cancel")
                .outline()
                .size(ButtonSize::Large)
                .on_press(|| Msg::CloseModal),
        )
        .action(
            "switch",
            Button::new("Switch Context")
                .size(ButtonSize::Large)
                .on_press(|| Msg::ContextSwitcherCommitSelected),
        );

    ctx.modal(MODAL_CONTEXT_SWITCHER, dialog, ctx.area());
}
