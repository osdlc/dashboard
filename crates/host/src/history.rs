//! Host-kept history for `Series` widgets that set an `id`.

use std::collections::{HashMap, VecDeque};

use dashboard_contract::{View, Widget};

const DEFAULT_HISTORY: usize = 300;

#[derive(Default)]
pub struct History {
    /// Keyed by (tile ID, widget ID, series name).
    buffers: HashMap<(String, String, String), VecDeque<(i64, f64)>>,
}

impl History {
    /// Drop every buffer for a tile, e.g. when its module stops.
    pub fn forget(&mut self, tile_id: &str) {
        self.buffers.retain(|(tile, _, _), _| tile != tile_id);
    }

    /// Append new samples from `view` and replace them with the full history.
    pub fn apply(&mut self, tile_id: &str, view: &mut View) {
        self.walk(tile_id, &mut view.root);
    }

    fn walk(&mut self, tile_id: &str, widget: &mut Widget) {
        match widget {
            Widget::Series {
                id: Some(id),
                history,
                series,
                ..
            } => {
                let keep = history
                    .map(|h| h as usize)
                    .unwrap_or(DEFAULT_HISTORY)
                    .max(1);
                for s in series {
                    let key = (tile_id.to_string(), id.clone(), s.name.clone());
                    let buf = self.buffers.entry(key).or_default();
                    for point in s.points.drain(..) {
                        // Ignore samples that would go back in time (e.g. a replayed update).
                        if buf.back().is_none_or(|last| point.0 > last.0) {
                            buf.push_back(point);
                        }
                    }
                    while buf.len() > keep {
                        buf.pop_front();
                    }
                    s.points = buf.iter().copied().collect();
                }
            }
            Widget::Row { children } | Widget::Column { children } => {
                for child in children {
                    self.walk(tile_id, child);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashboard_contract::{SeriesData, Status};

    fn view(points: Vec<(i64, f64)>) -> View {
        View {
            status: Status::Ok,
            summary: String::new(),
            root: Widget::Column {
                children: vec![Widget::Series {
                    label: "CPU".into(),
                    unit: None,
                    y_max: None,
                    id: Some("cpu".into()),
                    history: Some(3),
                    style: Default::default(),
                    color: Default::default(),
                    mirrored: false,
                    format: Default::default(),
                    series: vec![SeriesData {
                        name: "total".into(),
                        points,
                    }],
                }],
            },
        }
    }

    fn points(v: &View) -> Vec<(i64, f64)> {
        let Widget::Column { children } = &v.root else {
            unreachable!()
        };
        let Widget::Series { series, .. } = &children[0] else {
            unreachable!()
        };
        series[0].points.clone()
    }

    #[test]
    fn appends_and_trims() {
        let mut h = History::default();
        for t in 1..=4 {
            let mut v = view(vec![(t, t as f64)]);
            h.apply("sysperf", &mut v);
            if t == 4 {
                assert_eq!(points(&v), vec![(2, 2.0), (3, 3.0), (4, 4.0)]);
            }
        }
        // Same widget ID in another tile is separate.
        let mut v = view(vec![(10, 1.0)]);
        h.apply("other", &mut v);
        assert_eq!(points(&v), vec![(10, 1.0)]);
        // Old timestamps are dropped.
        let mut v = view(vec![(1, 9.0)]);
        h.apply("sysperf", &mut v);
        assert_eq!(points(&v), vec![(2, 2.0), (3, 3.0), (4, 4.0)]);
    }
}
