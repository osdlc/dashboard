//! Mempool: Bitcoin fee rates, the mempool's size and the latest blocks, from
//! the public mempool.space API (or your own mempool instance). Clicking a
//! block opens it on the site. No account or key.
//!
//! ```yaml
//! modules:
//!   mempool:
//!     config:
//!       blocks: 6         # latest blocks listed
//!       warn_fee: 50      # warn when the fastest fee is at least this (sat/vB)
//!       # url: https://mempool.space/testnet4
//! ```

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, Cell, Column, Manifest, Requirements, Row, Size, Status, ValueFormat,
    View, Widget, format_value,
};
use dashboard_host::Module;
use dashboard_modules_common::http::{Client, check, client};
use dashboard_modules_common::manifest;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{ago, thousands};

/// Most blocks listed. One API call returns 15.
const MAX_BLOCKS: usize = 15;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// The mempool site, e.g. a self-hosted one or
    /// "https://mempool.space/testnet4".
    url: String,
    /// Latest blocks listed, 0 to 15.
    #[schemars(range(max = 15))]
    blocks: usize,
    /// Warn when the fastest fee rate is at least this many sat/vB. Null:
    /// never.
    warn_fee: Option<f64>,
    /// Read this file instead of calling the API: a JSON object with the
    /// `fees` (`/api/v1/fees/recommended`), `mempool` (`/api/mempool`) and
    /// `blocks` (`/api/v1/blocks`) replies. For tests and demos.
    sample: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: "https://mempool.space".into(),
            blocks: 6,
            warn_fee: Some(50.0),
            sample: None,
        }
    }
}

#[derive(Default)]
pub struct Mempool {
    config: Config,
}

#[async_trait]
impl Module for Mempool {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let c = &mut self.config;
        if c.blocks > MAX_BLOCKS {
            bail!("blocks: at most {MAX_BLOCKS}");
        }
        if !c.url.starts_with("http://") && !c.url.starts_with("https://") {
            bail!("url: must start with http:// or https://");
        }
        c.url = c.url.trim_end_matches('/').to_string();
        Ok(manifest(
            "mempool",
            "Mempool",
            60_000,
            15_000,
            Size::Medium,
            30,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let data: Data = match &c.sample {
            Some(path) => crate::read_sample(path)?,
            None => fetch(&c.url).await?,
        };
        let now = dashboard_host::now_ms() / 1000;
        Ok(view(&data, c, now))
    }
}

/// What the tile shows, from three API calls.
#[derive(Debug, Deserialize)]
struct Data {
    fees: Fees,
    mempool: MempoolInfo,
    #[serde(default)]
    blocks: Vec<Block>,
}

/// `/api/v1/fees/recommended`, in sat/vB.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fees {
    fastest_fee: f64,
    half_hour_fee: f64,
    hour_fee: f64,
}

/// `/api/mempool`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MempoolInfo {
    count: u64,
    /// Virtual bytes waiting: about 1,000,000 fill a block.
    vsize: u64,
}

/// One entry of `/api/v1/blocks`, newest first.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Block {
    id: String,
    height: i64,
    timestamp: i64,
    tx_count: i64,
    extras: Option<Extras>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Extras {
    median_fee: Option<f64>,
    pool: Option<Pool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Pool {
    name: String,
}

async fn fetch(base: &str) -> Result<Data> {
    let http = client()?;
    async fn get<T: DeserializeOwned>(http: &Client, url: String) -> Result<T> {
        Ok(check(http.get(&url).send().await?).await?.json().await?)
    }
    Ok(Data {
        fees: get(&http, format!("{base}/api/v1/fees/recommended")).await?,
        mempool: get(&http, format!("{base}/api/mempool")).await?,
        blocks: get(&http, format!("{base}/api/v1/blocks")).await?,
    })
}

/// A fee rate: whole numbers as they are, small fractions with one decimal.
fn fee(rate: f64) -> String {
    if rate.fract() == 0.0 || rate >= 10.0 {
        format!("{rate:.0}")
    } else {
        format!("{rate:.1}")
    }
}

/// The tile: fee rates and the mempool's size, then the latest blocks.
/// `now` is in unix seconds.
fn view(data: &Data, c: &Config, now: i64) -> View {
    let f = &data.fees;
    let high = c.warn_fee.is_some_and(|warn| f.fastest_fee >= warn);
    let status = if high { Status::Warn } else { Status::Ok };
    let rate = |label: &str, value: f64, status: Option<Status>| Widget::Stat {
        label: label.into(),
        value: fee(value),
        unit: Some("sat/vB".into()),
        delta: None,
        status,
    };
    let blocks_to_clear = data.mempool.vsize.div_ceil(1_000_000);
    let stats = Widget::Row {
        children: vec![
            rate("Fast", f.fastest_fee, high.then_some(Status::Warn)),
            rate("30 min", f.half_hour_fee, None),
            rate("1 hour", f.hour_fee, None),
            Widget::Stat {
                label: "Mempool".into(),
                value: format_value(data.mempool.count as f64, ValueFormat::Count, None),
                unit: Some("tx".into()),
                delta: Some(format!(
                    "~{blocks_to_clear} {}",
                    if blocks_to_clear == 1 {
                        "block"
                    } else {
                        "blocks"
                    }
                )),
                status: None,
            },
        ],
    };
    let rows: Vec<Row> = data
        .blocks
        .iter()
        .take(c.blocks)
        .map(|b| {
            let extras = b.extras.as_ref();
            let median = extras.and_then(|e| e.median_fee);
            let pool = extras
                .and_then(|e| e.pool.as_ref())
                .map(|p| p.name.as_str());
            let cells = [
                ("height", thousands(b.height)),
                ("mined", ago(now - b.timestamp)),
                ("txs", thousands(b.tx_count)),
                (
                    "fee",
                    median.map_or("-".into(), |m| format!("{} sat/vB", fee(m))),
                ),
                (
                    "pool",
                    pool.filter(|p| !p.is_empty()).unwrap_or("-").to_string(),
                ),
            ];
            Row {
                cells: cells
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), Cell::Text(v)))
                    .collect(),
                status: None,
                action: Some(Action::OpenUrl {
                    url: format!("{}/block/{}", c.url, b.id),
                }),
            }
        })
        .collect();
    let mut children = vec![stats];
    if c.blocks > 0 {
        let col = |key: &str, label: &str| Column {
            key: key.into(),
            label: label.into(),
        };
        children.push(Widget::Table {
            columns: vec![
                col("height", "Block"),
                col("mined", "Mined"),
                col("txs", "Txs"),
                col("fee", "Median fee"),
                col("pool", "Pool"),
            ],
            rows,
        });
    }
    let summary = match data.blocks.first() {
        Some(b) => format!(
            "Block {} · {} sat/vB",
            thousands(b.height),
            fee(f.fastest_fee)
        ),
        None => format!("{} sat/vB", fee(f.fastest_fee)),
    };
    View {
        status,
        summary,
        root: Widget::Column { children },
    }
}

/// "Now" for the sample: 2026-10-27T12:00:00Z.
const SAMPLE_NOW: i64 = 1_793_102_400;

pub fn catalog() -> CatalogEntry {
    let data: Data = serde_json::from_str(include_str!("../testdata/mempool/sample.json"))
        .expect("demo data parses");
    CatalogEntry {
        description: "Bitcoin fee rates (fast, 30 minutes, 1 hour), the mempool's size and \
                      the latest blocks with their pool and median fee, from mempool.space or \
                      your own instance. Warns when fees run high. Public API, no key."
            .into(),
        tags: crate::strings(&["crypto", "bitcoin", "finance", "public-api"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: vec![],
            hosts: crate::strings(&["mempool.space"]),
        },
        ..crate::entry(
            "mempool",
            "Mempool",
            Size::Medium,
            view(&data, &Config::default(), SAMPLE_NOW),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Data {
        let path = format!(
            "{}/testdata/mempool/sample.json",
            env!("CARGO_MANIFEST_DIR")
        );
        crate::read_sample(&path).unwrap()
    }

    fn text(row: &Row, key: &str) -> String {
        match &row.cells[key] {
            Cell::Text(s) => s.clone(),
            other => panic!("{key}: {other:?}"),
        }
    }

    #[test]
    fn parses_the_sample() {
        let v = view(&sample(), &Config::default(), SAMPLE_NOW);
        assert_eq!(v.status, Status::Ok);
        assert_eq!(v.summary, "Block 973,386 · 12 sat/vB");
        let Widget::Column { children } = &v.root else {
            panic!("not a column: {:?}", v.root);
        };
        let Widget::Row { children: stats } = &children[0] else {
            panic!("stats");
        };
        let Widget::Stat {
            value, delta, unit, ..
        } = &stats[3]
        else {
            panic!("mempool stat");
        };
        assert_eq!(value, "46.6k");
        assert_eq!(unit.as_deref(), Some("tx"));
        assert_eq!(delta.as_deref(), Some("~20 blocks"));

        let Widget::Table { rows, .. } = &children[1] else {
            panic!("table");
        };
        assert_eq!(rows.len(), 4);
        assert_eq!(text(&rows[0], "height"), "973,386");
        assert_eq!(text(&rows[0], "mined"), "10m ago");
        assert_eq!(text(&rows[0], "txs"), "2,329");
        assert_eq!(text(&rows[0], "fee"), "8.4 sat/vB");
        assert_eq!(text(&rows[0], "pool"), "OCEAN");
        assert_eq!(text(&rows[2], "fee"), "0 sat/vB");
        // No extras: dashes.
        assert_eq!(text(&rows[3], "fee"), "-");
        assert_eq!(text(&rows[3], "pool"), "-");
        assert_eq!(
            rows[0].action,
            Some(Action::OpenUrl {
                url: "https://mempool.space/block/00000000000000000001a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7".into()
            })
        );
    }

    #[test]
    fn warns_on_high_fees() {
        let c = Config {
            warn_fee: Some(12.0),
            blocks: 0,
            ..Config::default()
        };
        let v = view(&sample(), &c, SAMPLE_NOW);
        assert_eq!(v.status, Status::Warn);
        let Widget::Column { children } = &v.root else {
            panic!()
        };
        // No blocks asked for: no table.
        assert_eq!(children.len(), 1);
        let c = Config {
            warn_fee: None,
            ..Config::default()
        };
        assert_eq!(view(&sample(), &c, SAMPLE_NOW).status, Status::Ok);
    }

    #[test]
    fn formats_fee_rates() {
        assert_eq!(fee(1.0), "1");
        assert_eq!(fee(0.5), "0.5");
        assert_eq!(fee(2.26), "2.3");
        assert_eq!(fee(12.6), "13");
    }

    #[tokio::test]
    async fn checks_the_config() {
        let mut m = Mempool::default();
        assert!(m.init(serde_json::json!({ "blocks": 16 })).await.is_err());
        assert!(
            m.init(serde_json::json!({ "url": "mempool.space" }))
                .await
                .is_err()
        );
        m.init(serde_json::json!({ "url": "https://mempool.space/testnet4/" }))
            .await
            .unwrap();
        assert_eq!(m.config.url, "https://mempool.space/testnet4");
    }
}
