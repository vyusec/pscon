use super::*;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, Gauge, Paragraph, Row, Table, TableState, Tabs,
        Wrap,
    },
};
use unicode_width::UnicodeWidthChar;

const BG: Color = Color::Rgb(13, 19, 30);
const PANEL: Color = Color::Rgb(19, 28, 43);
const LINE: Color = Color::Rgb(44, 61, 82);
const TEXT: Color = Color::Rgb(220, 230, 241);
const MUTED: Color = Color::Rgb(142, 160, 182);
const CYAN: Color = Color::Rgb(75, 219, 217);
const SELECTED: Color = Color::Rgb(27, 60, 78);
const AMBER: Color = Color::Rgb(247, 193, 94);
const RED: Color = Color::Rgb(251, 115, 133);
const LOGO: &str = r" ____  ____   ____ ___  _   _
|  _ \/ ___| / ___/ _ \| \ | |
| |_) \___ \| |  | | | |  \| |
|  __/ ___) | |__| |_| | |\  |
|_|   |____/ \____\___/|_| \_|";
fn block(title: impl Into<String>) -> Block<'static> {
    Block::default()
        .title(format!(" {} ", title.into()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(LINE))
        .style(Style::default().fg(TEXT).bg(PANEL))
}
fn accent() -> Style {
    Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
}
fn muted() -> Style {
    Style::default().fg(MUTED)
}
fn risk(value: f32) -> Style {
    Style::default().fg(if value >= 0.8 {
        RED
    } else if value >= 0.5 {
        AMBER
    } else {
        TEXT
    })
}
fn rows(area: Rect, sizes: impl IntoIterator<Item = Constraint>) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints(sizes)
        .split(area)
}
fn cols(area: Rect, sizes: impl IntoIterator<Item = Constraint>) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(sizes)
        .split(area)
}
fn inset(area: Rect, x: u16, y: u16) -> Rect {
    Rect {
        x: area.x + x.min(area.width),
        y: area.y + y.min(area.height),
        width: area.width.saturating_sub(x * 2),
        height: area.height.saturating_sub(y * 2),
    }
}
fn popup(area: Rect, width: u16, height: u16) -> Rect {
    if area.width < 80 || area.height < 24 {
        return area;
    }
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}
fn paragraph(frame: &mut Frame, text: impl Into<String>, area: Rect, title: &str) {
    frame.render_widget(
        Paragraph::new(text.into())
            .block(block(title))
            .wrap(Wrap { trim: false }),
        area,
    );
}
pub(super) fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BG).fg(TEXT)),
        area,
    );
    if area.width < 60 || area.height < 18 {
        frame.render_widget(
            Paragraph::new("PSCON\n\nResize terminal to at least 60 × 18.\nCtrl+C to exit.")
                .style(accent())
                .wrap(Wrap { trim: true }),
            inset(area, 1, 1),
        );
        return;
    }
    let layout = rows(
        area,
        [
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(3),
        ],
    );
    let header = block("PSCON / RECONNAISSANCE CONSOLE");
    let target = if app.scan_running {
        app.target.as_ref().map(ToString::to_string)
    } else {
        app.scan.as_ref().map(|s| s.target.clone())
    };
    let state = if app.scan_running {
        "SCANNING"
    } else {
        "READY"
    };
    let bridge = if app.bridge_connected {
        "JSner connected"
    } else {
        &app.bridge_status
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {state} "), accent()),
            Span::raw(target.unwrap_or_else(|| "Browser-assisted recon".into())),
            Span::styled(format!("   • {bridge}"), muted()),
        ]))
        .block(header),
        layout[0],
    );
    if matches!(app.screen, Screen::Home | Screen::History) {
        frame.render_widget(
            Paragraph::new(format!(
                "  {}   /   h Home   H History   Ctrl+P Commands",
                app.screen.title()
            ))
            .style(muted()),
            layout[1],
        );
    } else if area.width < 100 {
        frame.render_widget(
            Paragraph::new(format!(
                "  {} {}   /   1–6 views · Tab next · Shift+Tab previous",
                app.screen.index() + 1,
                app.screen.title()
            ))
            .style(accent()),
            layout[1],
        );
    } else {
        let titles: Vec<_> = Screen::ALL[..6]
            .iter()
            .enumerate()
            .map(|(i, s)| Line::from(format!("{} {}", i + 1, s.title())))
            .collect();
        frame.render_widget(
            Tabs::new(titles)
                .select(app.screen.index())
                .style(muted())
                .highlight_style(accent().bg(SELECTED))
                .divider(" "),
            layout[1],
        );
    }
    match app.screen {
        Screen::Home => home(frame, app, layout[2]),
        Screen::Overview => overview(frame, app, layout[2]),
        Screen::Report => report(frame, app, layout[2]),
        _ => table(frame, app, layout[2]),
    }
    let foot = rows(
        layout[3],
        [
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    );
    frame.render_widget(
        Paragraph::new(app.status.as_str()).style(Style::default().fg(
            if app.status.to_lowercase().contains("fail") {
                RED
            } else {
                MUTED
            },
        )),
        foot[0],
    );
    if let Some(search) = &app.search {
        let area = foot[1];
        frame.render_widget(Paragraph::new("/ ").style(accent()), area);
        input(
            frame,
            search,
            Rect::new(area.x + 2, area.y, area.width.saturating_sub(2), 1),
            true,
        );
        frame.render_widget(
            Paragraph::new("Enter apply  Esc restore  Ctrl+U clear").style(muted()),
            foot[2],
        );
    } else {
        let hints = match app.screen {
            Screen::Home => "↑↓ choose  Enter open  s New scan  H History  ? Help  q Quit",
            Screen::Report => "e Export all formats  s New scan  h Home  Ctrl+P Commands  q Quit",
            Screen::History => {
                "Enter open  n Scan this target  / Search  o Sort  O Reverse  r Refresh"
            }
            _ => "Enter details  / Search  o Sort  O Reverse  s New scan  e Export  ? Help",
        };
        frame.render_widget(Paragraph::new(hints).style(muted()), foot[1]);
        frame.render_widget(
            Paragraph::new("1–6 views   Tab / Shift+Tab   h Home   H History   Ctrl+P Commands")
                .style(muted()),
            foot[2],
        );
    }
    modal(frame, app, area);
}
fn home(frame: &mut Frame, app: &App, area: Rect) {
    let compact = area.height < 14;
    let content = inset(area, if area.width >= 100 { 6 } else { 1 }, 0);
    let sections = rows(
        content,
        [
            Constraint::Length(if compact {
                2
            } else if area.height < 20 {
                6
            } else {
                8
            }),
            Constraint::Min(6),
            Constraint::Length(if compact { 0 } else { 3 }),
        ],
    );
    let logo = if compact {
        "PSCON  /  BROWSER-ASSISTED RECON"
    } else {
        LOGO
    };
    frame.render_widget(Paragraph::new(logo).style(accent()), sections[0]);
    let panes = if area.width >= 100 {
        cols(
            sections[1],
            [Constraint::Percentage(45), Constraint::Percentage(55)],
        )
    } else {
        rows(
            sections[1],
            [
                Constraint::Min(6),
                Constraint::Length(if area.height < 22 { 0 } else { 5 }),
            ],
        )
    };
    let actions = [
        "New scan",
        "Recent scans",
        "Resume latest scan",
        "Help and shortcuts",
    ];
    let menu: Vec<_> = actions
        .iter()
        .enumerate()
        .map(|(i, label)| {
            Line::from(Span::styled(
                format!(
                    " {}  {}",
                    if app.home_selected == i { "›" } else { " " },
                    label
                ),
                if app.home_selected == i {
                    accent().bg(SELECTED)
                } else {
                    Style::default().fg(TEXT)
                },
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(menu).block(block("START HERE")), panes[0]);
    let latest = app.history.first().map_or(
        "No saved scans yet.\nEnter a target to build your first asset inventory.".into(),
        |s| {
            format!(
                "{}\n{} · {}\n{} endpoints  /  {} secrets  /  {} assets",
                s.target,
                s.status.as_str(),
                s.started_at.format("%Y-%m-%d %H:%M UTC"),
                s.summary.endpoints,
                s.summary.secrets,
                s.summary.assets
            )
        },
    );
    paragraph(frame, latest, panes[1], "LATEST SCAN");
    paragraph(
        frame,
        "Connect JSner to send browser snapshots, or press s to enter a target URL.",
        sections[2],
        "GET CONNECTED",
    );
}
fn overview(frame: &mut Frame, app: &mut App, area: Rect) {
    let compact = area.height < 15;
    let parts = rows(
        area,
        [
            Constraint::Length(if compact { 1 } else { 3 }),
            Constraint::Length(if compact { 1 } else { 3 }),
            Constraint::Min(1),
        ],
    );
    if compact {
        frame.render_widget(
            Paragraph::new(format!(
                " Assets {}  Endpoints {}  Secrets {}  Checks {}  AI {}",
                app.assets.len(),
                app.endpoints.len(),
                app.secrets.len(),
                app.verifications.len(),
                app.decisions.len()
            ))
            .style(accent()),
            parts[0],
        );
    } else {
        let cards = cols(parts[0], [Constraint::Ratio(1, 5); 5]);
        for (i, (title, count)) in [
            ("ASSETS", app.assets.len()),
            ("ENDPOINTS", app.endpoints.len()),
            ("SECRETS", app.secrets.len()),
            ("VERIFIED", app.verifications.len()),
            ("DECISIONS", app.decisions.len()),
        ]
        .into_iter()
        .enumerate()
        {
            frame.render_widget(
                Paragraph::new(count.to_string())
                    .style(if i == 2 && count > 0 {
                        Style::default().fg(RED)
                    } else {
                        accent()
                    })
                    .block(block(title)),
                cards[i],
            );
        }
    }
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(CYAN).bg(LINE))
        .ratio(if app.progress.is_finite() {
            app.progress.clamp(0., 1.) as f64
        } else {
            0.
        })
        .label(format!("{} · {:.0}%", app.stage, app.progress * 100.));
    frame.render_widget(
        if compact {
            gauge
        } else {
            gauge.block(block(format!("SCAN / {}", app.stage)))
        },
        parts[1],
    );
    let panels = if area.width >= 110 {
        cols(
            parts[2],
            [Constraint::Percentage(65), Constraint::Percentage(35)],
        )
    } else {
        rows(
            parts[2],
            [
                Constraint::Min(3),
                Constraint::Length(if area.height >= 18 { 6 } else { 0 }),
            ],
        )
    };
    table(frame, app, panels[0]);
    if panels[1].height > 0 {
        let height = panels[1].height.saturating_sub(2) as usize;
        let end = app.logs.len().saturating_sub(app.log_scroll);
        let start = end.saturating_sub(height);
        frame.render_widget(
            Paragraph::new(app.logs[start..end].join("\n"))
                .style(muted())
                .block(block("ACTIVITY / [ older  ] newer")),
            panels[1],
        );
    }
}
fn table(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height == 0 {
        return;
    }
    let records = app.records();
    let headers = app.screen.headers();
    let state = app.view();
    let filter = app
        .search
        .as_ref()
        .map_or(state.filter.as_str(), |i| &i.text);
    let title = format!(
        "{} / {} results · {} {}{}",
        app.screen.title(),
        records.len(),
        headers.get(state.sort).unwrap_or(&""),
        if state.descending { "↓" } else { "↑" },
        if filter.is_empty() {
            String::new()
        } else {
            format!(" · filter: {filter}")
        }
    );
    if records.is_empty() {
        let message = if filter.is_empty() {
            "No results yet.\nPress s to start a scan, H for saved scans, or send a JSner snapshot."
        } else {
            "No matching results.\nPress / to edit the filter or Esc to clear it."
        };
        paragraph(frame, message, area, &title);
        return;
    }
    let wide = area.width >= 105;
    let columns: Vec<usize> = match app.screen {
        Screen::Overview | Screen::Endpoints => {
            if wide {
                vec![0, 1, 2, 3, 4, 5]
            } else {
                vec![0, 1, 2]
            }
        }
        Screen::Secrets => {
            if wide {
                vec![0, 1, 2, 3, 4, 5]
            } else {
                vec![0, 1, 2, 5]
            }
        }
        Screen::Assets => vec![0, 1, 2, 3],
        Screen::Decisions => {
            if wide {
                vec![0, 1, 2, 3, 4, 5]
            } else {
                vec![0, 2, 3]
            }
        }
        Screen::History => {
            if wide {
                vec![0, 1, 2, 3, 4, 5]
            } else {
                vec![1, 2, 3]
            }
        }
        _ => vec![],
    };
    let flexible = match app.screen {
        Screen::Assets => 3,
        Screen::Secrets if !wide => 5,
        _ => 2,
    };
    let widths: Vec<_> = columns
        .iter()
        .map(|&i| {
            if i == flexible {
                Constraint::Min(12)
            } else {
                Constraint::Length(match (app.screen, i) {
                    (Screen::History, 0) => 23,
                    (Screen::Secrets, 2) => 20,
                    (Screen::Secrets, 4) => 16,
                    (_, 0)
                        if matches!(
                            app.screen,
                            Screen::Overview | Screen::Endpoints | Screen::Secrets
                        ) =>
                    {
                        8
                    }
                    (_, 1) => 10,
                    _ => 11,
                })
            }
        })
        .collect();
    let table_rows = records.iter().map(|record| {
        Row::new(
            columns
                .iter()
                .map(|&i| Cell::from(record.cells[i].clone()))
                .collect::<Vec<_>>(),
        )
        .style(risk(record.risk))
    });
    let table = Table::new(table_rows, widths)
        .header(
            Row::new(
                columns
                    .iter()
                    .map(|&i| Cell::from(headers[i]))
                    .collect::<Vec<_>>(),
            )
            .style(accent())
            .bottom_margin(1),
        )
        .block(block(title))
        .column_spacing(1)
        .highlight_symbol("› ")
        .row_highlight_style(Style::default().bg(SELECTED).add_modifier(Modifier::BOLD));
    let mut selection = TableState::default()
        .with_offset(state.offset)
        .with_selected(Some(state.position.min(records.len() - 1)));
    frame.render_stateful_widget(table, area, &mut selection);
    app.view_mut().offset = selection.offset();
}
fn report(frame: &mut Frame, app: &App, area: Rect) {
    let body = match &app.scan {
        Some(scan) => {
            let dir = app.config.scans_dir().join(scan.id.to_string());
            format!(
                "SCAN {}\n{}\nStatus: {}\n\nPress e to regenerate all four reports.\n\nJSON       {}\nMarkdown   {}\nHTML       {}\nSARIF      {}\n\n{}",
                scan.id,
                scan.target,
                scan.status.as_str(),
                dir.join("report.json").display(),
                dir.join("report.md").display(),
                dir.join("report.html").display(),
                dir.join("report.sarif").display(),
                app.export_message
            )
        }
        None => "No scan available.\nPress s to start a scan or H to open saved results.".into(),
    };
    paragraph(frame, body, area, "REPORTS / e export");
}
fn input(frame: &mut Frame, value: &Input, area: Rect, focused: bool) {
    let (text, cursor) = value.visible(area.width);
    frame.render_widget(
        Paragraph::new(text).style(if focused {
            accent().bg(SELECTED)
        } else {
            Style::default().fg(TEXT)
        }),
        area,
    );
    if focused && area.width > 0 && area.height > 0 {
        frame.set_cursor_position((area.x + cursor, area.y));
    }
}
/// Wrap by terminal cell width so scroll offsets and cursor placement agree with rendering.
fn wrap_lines(text: &str, width: u16) -> Vec<Line<'static>> {
    let mut result = vec![];
    for line in text.lines() {
        let mut buf = String::new();
        let mut used = 0;
        for c in line.chars().filter(|c| !c.is_control()) {
            let w = c.width().unwrap_or(0);
            if used + w > width as usize && !buf.is_empty() {
                result.push(Line::from(std::mem::take(&mut buf)));
                used = 0;
            }
            buf.push(c);
            used += w;
        }
        result.push(Line::from(buf));
    }
    result
}
fn modal(frame: &mut Frame, app: &mut App, area: Rect) {
    match &mut app.modal {
        Modal::None => {}
        Modal::Form(form) => {
            let outer = popup(area, 84, 31);
            frame.render_widget(Clear, outer);
            frame.render_widget(block("NEW SCAN / session settings"), outer);
            let content = inset(outer, 2, 1);
            // Compact terminals show focused field plus all toggles and the submit action.
            let compact = content.height < 25;
            let indices: Vec<usize> = if compact {
                vec![if form.focus < 4 { form.focus } else { 0 }]
            } else {
                (0..4).collect()
            };
            let mut y = content.y;
            for i in indices {
                let label = [
                    "Target URL",
                    "Crawl depth",
                    "Maximum pages",
                    "Maximum assets",
                ][i];
                frame.render_widget(
                    Paragraph::new(format!(
                        "{} {}",
                        if form.focus == i { "›" } else { " " },
                        label
                    ))
                    .style(if form.focus == i { accent() } else { muted() }),
                    Rect::new(content.x, y, content.width, 1),
                );
                input(
                    frame,
                    &form.fields[i],
                    Rect::new(content.x + 2, y + 1, content.width.saturating_sub(2), 1),
                    form.focus == i,
                );
                frame.render_widget(
                    Paragraph::new(form.errors[i].as_str()).style(Style::default().fg(RED)),
                    Rect::new(content.x + 2, y + 2, content.width.saturating_sub(2), 1),
                );
                y += if compact { 3 } else { 4 };
            }
            for (i, label) in [
                "Safe endpoint verification",
                "Decision providers",
                "Notifications",
            ]
            .iter()
            .enumerate()
            {
                frame.render_widget(
                    Paragraph::new(format!(
                        "{} [{}] {}",
                        if form.focus == i + 4 { "›" } else { " " },
                        if form.toggles[i] { "x" } else { " " },
                        label
                    ))
                    .style(if form.focus == i + 4 {
                        accent().bg(SELECTED)
                    } else {
                        muted()
                    }),
                    Rect::new(content.x, y, content.width, 1),
                );
                y += 1;
            }
            y += 1;
            frame.render_widget(
                Paragraph::new(if app.scan_running {
                    "Scan in progress"
                } else {
                    "  [ Start scan ]"
                })
                .style(if form.focus == 7 {
                    accent().bg(SELECTED)
                } else {
                    accent()
                }),
                Rect::new(content.x, y, content.width, 1),
            );
            frame.render_widget(Paragraph::new("Tab/Shift+Tab focus · Space toggle · Ctrl+Enter start\nEsc cancel · Ctrl+U clear · HTTP/HTTPS targets").style(muted()), Rect::new(content.x, y + 2, content.width, 2));
        }
        Modal::Palette {
            input: editor,
            selected,
        } => {
            let outer = popup(area, 70, 19);
            frame.render_widget(Clear, outer);
            frame.render_widget(block("COMMANDS / Esc close"), outer);
            let content = inset(outer, 2, 1);
            input(
                frame,
                editor,
                Rect::new(content.x, content.y, content.width, 1),
                true,
            );
            let actions = App::palette_actions(editor);
            let lines: Vec<_> = actions
                .iter()
                .enumerate()
                .skip(selected.saturating_sub(content.height.saturating_sub(4) as usize))
                .map(|(i, a)| {
                    Line::from(Span::styled(
                        format!("{} {}", if i == *selected { "›" } else { " " }, a.label()),
                        if i == *selected {
                            accent().bg(SELECTED)
                        } else {
                            muted()
                        },
                    ))
                })
                .collect();
            frame.render_widget(
                Paragraph::new(if actions.is_empty() {
                    vec![Line::from("No matching commands")]
                } else {
                    lines
                }),
                Rect::new(
                    content.x,
                    content.y + 2,
                    content.width,
                    content.height.saturating_sub(2),
                ),
            );
        }
        Modal::Details {
            title,
            body,
            scroll,
        } => {
            let outer = popup(area, 110, area.height.saturating_sub(2));
            frame.render_widget(Clear, outer);
            let content = inset(outer, 2, 1);
            let lines = wrap_lines(body, content.width);
            *scroll = (*scroll).min(
                lines
                    .len()
                    .saturating_sub(content.height as usize)
                    .min(u16::MAX as usize) as u16,
            );
            frame.render_widget(
                block(title.clone()).title_bottom(" ↑↓ PgUp PgDn scroll · Esc close "),
                outer,
            );
            frame.render_widget(Paragraph::new(lines).scroll((*scroll, 0)), content);
        }
        Modal::Sort { selected } => {
            let outer = popup(area, 46, 12);
            frame.render_widget(Clear, outer);
            let mut lines = vec![
                Line::from("Enter choose · O reverses in workspace"),
                Line::from(""),
            ];
            lines.extend(app.screen.headers().iter().enumerate().map(|(i, label)| {
                Line::from(Span::styled(
                    format!("{} {label}", if *selected == i { "›" } else { " " }),
                    if *selected == i { accent() } else { muted() },
                ))
            }));
            frame.render_widget(
                Paragraph::new(lines).block(block("SORT BY / Esc cancel")),
                outer,
            );
        }
        Modal::Help { scroll } => {
            let outer = popup(area, 85, 26);
            frame.render_widget(Clear, outer);
            let content = inset(outer, 2, 1);
            let lines = wrap_lines(
                "WORKFLOW\ns         New scan form       h / H      Home / Recent scans\n1–6       Switch view         Tab        Next view (Shift reverses)\nCtrl+P    Command palette     Enter      Inspect / open selection\n\nRESULTS\n/         Live search         Esc        Cancel search / clear filter\no / O     Sort column / reverse order\nj/k, ↑↓   Move                g/G        First / last\nPgUp/Dn   Move ten rows       [ / ]      Older / newer activity\nr         Reload / refresh    e          Export four report formats\nn         New scan from selected history target\n\nINPUT\nTab       Next field          Shift+Tab  Previous field\nSpace     Toggle option       Ctrl+Enter Start scan\n←→ Home End Delete Backspace · Ctrl+U clear · Paste supported\n\nq / Ctrl+C quit · Esc closes dialogs",
                content.width,
            );
            *scroll = (*scroll).min(lines.len().saturating_sub(content.height as usize) as u16);
            frame.render_widget(
                block("HELP").title_bottom(" ↑↓ PgUp PgDn scroll · Esc close "),
                outer,
            );
            frame.render_widget(Paragraph::new(lines).scroll((*scroll, 0)), content);
        }
    }
}
