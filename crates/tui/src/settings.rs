//! The `s` overlay: config file, theme, rotation, and per-module settings.

use dashboard_contract::{Theme, UiSettings};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::help::{centered, format_ms};
use crate::mouse::{self, Click, Hits};
use crate::theme::Palette;

/// Theme comes first and the key line is pinned to the bottom, so both sit at
/// known rows for mouse clicks whatever wraps in between.
pub fn draw(f: &mut Frame, p: &Palette, s: &UiSettings, hits: &mut Hits) {
    let area = centered(f.area(), 75, 85);
    f.render_widget(Clear, area);
    let block = Block::default().borders(Borders::ALL).title(" Settings ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    hits.add(area, Click::Inside);
    let [body, keys] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);

    let heading = |t: &str| Line::styled(t.to_string(), Style::new().add_modifier(Modifier::BOLD));
    let muted = Style::new().fg(p.muted);
    let accent = Style::new().fg(p.accent);

    // Row 0: heading. Row 1: "  [auto]  light   dark " - each choice clickable.
    let mut lines = vec![heading("Theme")];
    let mut choices = vec![("  ".to_string(), None)];
    let mut theme = vec![Span::raw("  ")];
    for t in [Theme::Auto, Theme::Light, Theme::Dark] {
        let name = theme_name(t);
        let (text, style) = if t == s.theme {
            (format!("[{name}]"), accent.add_modifier(Modifier::BOLD))
        } else {
            (format!(" {name} "), muted)
        };
        theme.push(Span::styled(text.clone(), style));
        theme.push(Span::raw(" "));
        choices.push((text, Some(Click::Theme(t))));
        choices.push((" ".to_string(), None));
    }
    lines.push(Line::from(theme));
    mouse::spans(
        Rect {
            y: body.y + 1,
            height: 1,
            ..body
        },
        &choices,
        hits,
    );

    lines.push(Line::raw(""));
    lines.push(heading("Config file"));
    lines.push(Line::raw(format!("  {}", s.config.path)));
    if !s.config.exists {
        lines.push(Line::styled(
            "  not created yet; showing the built-in demo",
            muted,
        ));
    }
    if let Some(err) = &s.config.error {
        lines.push(Line::styled(
            "  Not applied (previous config still in effect):",
            Style::new().fg(p.error),
        ));
        for l in err.lines() {
            lines.push(Line::styled(format!("    {l}"), Style::new().fg(p.error)));
        }
    }

    lines.push(Line::raw(""));
    lines.push(heading("Rotation"));
    match &s.rotate {
        Some(r) => lines.push(Line::raw(format!(
            "  every {} through {}, resumes {} after input",
            format_ms(r.interval_ms),
            r.screens.join(", "),
            format_ms(r.resume_after_ms)
        ))),
        None => lines.push(Line::styled(
            "  not configured (add `rotate:` to the config)",
            muted,
        )),
    }

    lines.push(Line::raw(""));
    lines.push(heading("Modules"));
    for m in &s.modules {
        let refresh = m.refresh_ms.map(format_ms).unwrap_or_else(|| "push".into());
        let source = if m.refresh_overridden {
            "from config"
        } else {
            "module default"
        };
        lines.push(Line::from(vec![
            Span::raw(format!("  {:<22}", format!("{} ({})", m.name, m.tile_id))),
            Span::raw(format!("every {refresh:<6}")),
            Span::styled(format!(" {source}"), muted),
        ]));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);

    let parts = [
        (
            "  t change theme".to_string(),
            Some(Click::Theme(next_theme(s.theme))),
        ),
        ("   ".to_string(), None),
        ("e edit config file".to_string(), Some(Click::EditConfig)),
        ("   ".to_string(), None),
        ("Esc close".to_string(), Some(Click::Close)),
    ];
    mouse::spans(keys, &parts, hits);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  t", accent),
            Span::raw(" change theme   "),
            Span::styled("e", accent),
            Span::raw(" edit config file   "),
            Span::styled("Esc", accent),
            Span::raw(" close"),
        ])),
        keys,
    );
}

pub fn theme_name(t: Theme) -> &'static str {
    match t {
        Theme::Auto => "auto",
        Theme::Light => "light",
        Theme::Dark => "dark",
    }
}

pub fn next_theme(t: Theme) -> Theme {
    match t {
        Theme::Auto => Theme::Light,
        Theme::Light => Theme::Dark,
        Theme::Dark => Theme::Auto,
    }
}
