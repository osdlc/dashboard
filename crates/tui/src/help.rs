//! The `?` overlay: keys, screens, and running modules.

use dashboard_contract::UiSettings;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::mouse::{Click, Hits};
use crate::theme::Palette;

/// Every key the TUI handles. The help screen is built from this table.
pub const KEYS: &[(&str, &str)] = &[
    ("?", "show or hide this help"),
    ("1-9", "go to screen"),
    ("Tab / Shift-Tab", "next / previous screen"),
    ("←↑↓→ hjkl", "move between tiles, or items inside one"),
    ("Enter", "go into a tile; on an item, open its link"),
    ("PgUp PgDn", "scroll a tall page (also Space, Home, End)"),
    (
        "s",
        "show or hide settings (t theme, c colors, e edit config)",
    ),
    ("r", "start or stop screen rotation (when configured)"),
    ("Esc", "close help or settings, or step back out"),
    ("mouse", "click tabs, links and buttons; wheel scrolls"),
    (
        "double-click",
        "show a tile fullscreen (Esc or double-click to close)",
    ),
    ("q", "quit"),
];

pub fn draw(f: &mut Frame, p: &Palette, settings: &UiSettings, current: usize, hits: &mut Hits) {
    let area = centered(f.area(), 70, 80);
    f.render_widget(Clear, area);
    hits.add(area, Click::Inside);

    let heading = |s: &str| Line::styled(s.to_string(), Style::new().add_modifier(Modifier::BOLD));
    let mut lines = vec![heading("Keys")];
    for (key, what) in KEYS {
        lines.push(Line::from(vec![
            Span::styled(format!("  {key:<16}"), Style::new().fg(p.accent)),
            Span::raw(*what),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(heading("Screens"));
    for (i, s) in settings.screens.iter().enumerate() {
        let marker = if i == current { "▸" } else { " " };
        lines.push(Line::raw(format!(
            "  {marker} {} {}  ({} tiles)",
            i + 1,
            s.name,
            s.tiles.len()
        )));
    }

    lines.push(Line::raw(""));
    lines.push(heading("Modules"));
    for m in &settings.modules {
        let refresh = m.refresh_ms.map(format_ms).unwrap_or_else(|| "push".into());
        lines.push(Line::from(vec![
            Span::raw(format!("  {} ", m.name)),
            Span::styled(
                format!("({})  every {refresh}", m.tile_id),
                Style::new().fg(p.muted),
            ),
        ]));
        if !m.commands.is_empty() {
            lines.push(Line::styled(
                format!("      commands: {}", m.commands.join(", ")),
                Style::new().fg(p.muted),
            ));
        }
    }

    let block = Block::default().borders(Borders::ALL).title(" Help ");
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn format_ms(ms: u64) -> String {
    match ms {
        ms if ms % 60_000 == 0 => format!("{}m", ms / 60_000),
        ms if ms % 1000 == 0 => format!("{}s", ms / 1000),
        ms => format!("{ms}ms"),
    }
}

pub fn centered(area: Rect, width_pct: u16, height_pct: u16) -> Rect {
    let [area] = Layout::vertical([Constraint::Percentage(height_pct)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::horizontal([Constraint::Percentage(width_pct)])
        .flex(Flex::Center)
        .areas(area);
    area
}
