//! Checks for things the JSON Schema can't express: values out of range,
//! table cells with no column, links that aren't http(s). Used by
//! `dashboard plugin-check` and the example plugin tests. Each finding is one
//! line of plain text naming where it is.
//!
//! A finding is a problem (the plugin sent something wrong) or a note
//! (allowed, but worth knowing: this host ignores it for now, or it shows
//! nothing useful).

use crate::{Action, CONTRACT_VERSION, Cell, Manifest, Refresh, View, Widget};

/// One thing found, with where it is.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Finding {
    pub problem: bool,
    pub text: String,
}

fn problem(out: &mut Vec<Finding>, text: String) {
    out.push(Finding {
        problem: true,
        text,
    });
}

fn note(out: &mut Vec<Finding>, text: String) {
    out.push(Finding {
        problem: false,
        text,
    });
}

/// Problems with a manifest a plugin returned from `initialize`.
pub fn check_manifest(m: &Manifest) -> Vec<Finding> {
    let mut out = vec![];
    if m.id.trim().is_empty() {
        problem(&mut out, "manifest.id is empty".into());
    }
    if m.name.trim().is_empty() {
        problem(&mut out, "manifest.name is empty".into());
    }
    let major = |v: &str| v.split('.').next().unwrap_or("").to_string();
    if major(&m.contract_version) != major(CONTRACT_VERSION) {
        problem(
            &mut out,
            format!(
                "manifest.contract_version is {:?}; this host speaks {CONTRACT_VERSION}",
                m.contract_version
            ),
        );
    }
    match &m.refresh {
        Refresh::Poll {
            interval_ms,
            min_interval_ms,
        } => {
            if *interval_ms < 100 {
                problem(
                    &mut out,
                    format!("refresh.interval_ms is {interval_ms}; use 100 or more"),
                );
            }
            if min_interval_ms.is_some_and(|min| min > *interval_ms) {
                problem(
                    &mut out,
                    "refresh.min_interval_ms is above interval_ms".into(),
                );
            }
        }
        Refresh::Push => note(
            &mut out,
            "refresh.mode push is not supported yet; use poll".into(),
        ),
    }
    if !m.commands.is_empty() {
        note(
            &mut out,
            "commands are declared but the host does not run commands yet".into(),
        );
    }
    out
}

/// Problems with a view a plugin returned from `view/render`.
pub fn check_view(v: &View) -> Vec<Finding> {
    let mut out = vec![];
    if v.summary.trim().is_empty() {
        problem(
            &mut out,
            "summary is empty (it is shown when the tile is too small)".into(),
        );
    }
    widget(&v.root, "root", &mut out);
    out
}

fn widget(w: &Widget, at: &str, out: &mut Vec<Finding>) {
    let percent = |out: &mut Vec<Finding>, what: &str, p: f64| check_percent(out, at, what, p);
    match w {
        Widget::Gauge {
            percent: p, marker, ..
        } => {
            percent(out, "percent", *p);
            if let Some(m) = marker {
                percent(out, "marker", *m);
            }
        }
        Widget::MeterGrid { meters, .. } => {
            for m in meters {
                percent(out, &format!("meter {:?}", m.label), m.percent);
            }
        }
        Widget::Series { series, .. } => {
            for s in series {
                let times: Vec<i64> = s.points.iter().map(|p| p.0).collect();
                if times.windows(2).any(|w| w[1] < w[0]) {
                    problem(
                        out,
                        format!("{at}: series {:?} points are not in time order", s.name),
                    );
                }
                // Seconds instead of milliseconds is the usual mistake.
                if times.iter().any(|t| *t > 0 && *t < 100_000_000_000) {
                    problem(
                        out,
                        format!(
                            "{at}: series {:?} times look like seconds; use unix milliseconds",
                            s.name
                        ),
                    );
                }
                if s.points.iter().any(|p| !p.1.is_finite()) {
                    problem(
                        out,
                        format!("{at}: series {:?} has a NaN or infinite value", s.name),
                    );
                }
            }
        }
        Widget::Pie { slices, .. } => {
            if slices.is_empty() {
                note(
                    out,
                    format!("{at}: pie has no slices, shown as \"no data\""),
                );
            }
            for s in slices {
                if !s.value.is_finite() || s.value < 0.0 {
                    problem(
                        out,
                        format!(
                            "{at}: slice {:?} value is {}; use a finite number, 0 or more",
                            s.label, s.value
                        ),
                    );
                }
            }
            if !slices.is_empty() && slices.iter().all(|s| s.value == 0.0) {
                note(out, format!("{at}: every slice is 0, shown as \"no data\""));
            }
        }
        Widget::Uptime { checks } => {
            for c in checks {
                if let Some(u) = c.uptime {
                    percent(out, &format!("check {:?} uptime", c.name), u);
                }
                url(c.url.as_deref(), &format!("{at}: check {:?}", c.name), out);
            }
        }
        Widget::List { items, .. } => {
            for (i, item) in items.iter().enumerate() {
                action(item.action.as_ref(), &format!("{at}.items[{i}]"), out);
            }
        }
        Widget::Table { columns, rows } => {
            for (i, row) in rows.iter().enumerate() {
                let here = format!("{at}.rows[{i}]");
                for (key, cell) in &row.cells {
                    if !columns.iter().any(|c| &c.key == key) {
                        problem(
                            out,
                            format!("{here}: cell {key:?} has no column with that key"),
                        );
                    }
                    if let Cell::Widget(inner) = cell {
                        if matches!(
                            inner,
                            Widget::Stat { .. } | Widget::Gauge { .. } | Widget::Pie { .. }
                        ) {
                            widget(inner, &format!("{here}.cells[{key:?}]"), out);
                        } else {
                            problem(
                                out,
                                format!(
                                    "{here}: cell {key:?} holds a widget other than stat, gauge or pie, shown as text"
                                ),
                            );
                        }
                    }
                }
                action(row.action.as_ref(), &here, out);
            }
        }
        Widget::Row { children } | Widget::Column { children } => {
            for (i, child) in children.iter().enumerate() {
                widget(child, &format!("{at}.children[{i}]"), out);
            }
        }
        Widget::Image { path, .. } => {
            if path.trim().is_empty() {
                problem(out, format!("{at}: image path is empty"));
            }
        }
        Widget::Text { .. } | Widget::Stat { .. } | Widget::Bars { .. } | Widget::Log { .. } => {}
    }
}

fn check_percent(out: &mut Vec<Finding>, at: &str, what: &str, p: f64) {
    if !(0.0..=100.0).contains(&p) {
        problem(out, format!("{at}: {what} is {p}; percents are 0-100"));
    }
}

fn action(a: Option<&Action>, at: &str, out: &mut Vec<Finding>) {
    match a {
        Some(Action::OpenUrl { url: u }) => url(Some(u), at, out),
        Some(Action::RunCommand { .. }) => {
            note(out, format!("{at}: run_command actions are not run yet"));
        }
        None => {}
    }
}

fn url(u: Option<&str>, at: &str, out: &mut Vec<Finding>) {
    if let Some(u) = u
        && !(u.starts_with("https://") || u.starts_with("http://"))
    {
        problem(
            out,
            format!("{at}: url {u:?} is not http(s), so it won't open"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Column, Row, SeriesData, Slice, Status};

    #[test]
    fn finds_common_mistakes() {
        let view = View {
            status: Status::Ok,
            summary: "".into(),
            root: Widget::Column {
                children: vec![
                    Widget::Gauge {
                        label: "x".into(),
                        percent: 140.0,
                        marker: None,
                        thresholds: None,
                        caption: None,
                    },
                    Widget::Series {
                        label: "s".into(),
                        unit: None,
                        y_max: None,
                        id: None,
                        history: None,
                        style: Default::default(),
                        color: Default::default(),
                        mirrored: false,
                        format: Default::default(),
                        series: vec![SeriesData {
                            tone: None,
                            name: "a".into(),
                            points: vec![(1_793_000_000, 1.0)],
                        }],
                    },
                    Widget::Table {
                        columns: vec![Column {
                            key: "name".into(),
                            label: "Name".into(),
                            priority: None,
                        }],
                        rows: vec![Row {
                            cells: [("nmae".to_string(), Cell::Text("x".into()))].into(),
                            status: None,
                            action: Some(Action::OpenUrl {
                                url: "file:///etc/passwd".into(),
                            }),
                        }],
                    },
                ],
            },
        };
        let problems: Vec<String> = check_view(&view).into_iter().map(|f| f.text).collect();
        assert_eq!(problems.len(), 5, "{problems:#?}");
        assert!(problems[1].contains("percents are 0-100"));
        assert!(problems[2].contains("look like seconds"));
        assert!(problems[3].contains("\"nmae\" has no column"));
        assert!(problems[4].contains("not http(s)"));
    }

    fn pie(values: &[f64]) -> Widget {
        Widget::Pie {
            label: None,
            slices: values
                .iter()
                .enumerate()
                .map(|(i, v)| Slice {
                    tone: None,
                    label: format!("s{i}"),
                    value: *v,
                    status: None,
                })
                .collect(),
            donut: true,
            format: Default::default(),
            unit: None,
            color: Default::default(),
        }
    }

    fn findings(root: Widget) -> Vec<Finding> {
        check_view(&View {
            status: Status::Ok,
            summary: "x".into(),
            root,
        })
    }

    #[test]
    fn pie_values_must_be_finite_and_not_negative() {
        assert!(findings(pie(&[3.0, 0.0, 1.5])).is_empty());
        let found = findings(pie(&[1.0, -2.0, f64::NAN, f64::INFINITY]));
        assert_eq!(found.len(), 3, "{found:#?}");
        assert!(found.iter().all(|f| f.problem));
        assert!(found[0].text.contains("\"s1\" value is -2"));
    }

    #[test]
    fn empty_or_all_zero_pie_is_a_note() {
        for root in [pie(&[]), pie(&[0.0, 0.0])] {
            let found = findings(root);
            assert_eq!(found.len(), 1, "{found:#?}");
            assert!(!found[0].problem);
            assert!(found[0].text.contains("no data"));
        }
    }

    #[test]
    fn pie_in_a_row_and_a_table_cell_is_checked() {
        let row = Widget::Row {
            children: vec![pie(&[-1.0])],
        };
        let found = findings(row);
        assert_eq!(found.len(), 1);
        assert!(found[0].text.starts_with("root.children[0]: slice"));

        let table = Widget::Table {
            columns: vec![Column {
                key: "use".into(),
                label: "Use".into(),
                priority: None,
            }],
            rows: vec![Row {
                cells: [("use".to_string(), Cell::Widget(pie(&[-1.0])))].into(),
                status: None,
                action: None,
            }],
        };
        let found = findings(table);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(found[0].text.contains("root.rows[0].cells[\"use\"]"));
    }
}
