//! Crypto prices: the price and 24-hour change of a few coins, from
//! CoinGecko's public API, as a table. Clicking a coin opens its CoinGecko
//! page. No account or key.
//!
//! ```yaml
//! modules:
//!   crypto:
//!     config:
//!       coins: [bitcoin, ethereum, solana]   # CoinGecko ids
//!       currency: usd
//!       warn_drop: 5      # warn when a coin fell at least 5% in 24 hours
//! ```

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, Cell, Column, Manifest, Requirements, Row, Size, Status, View, Widget,
};
use dashboard_host::Module;
use dashboard_modules_common::http::{check, client};
use dashboard_modules_common::manifest;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::thousands;

const API: &str = "https://api.coingecko.com/api/v3/coins/markets";

/// Most coins shown.
const MAX_COINS: usize = 25;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// CoinGecko coin ids (the name in the coin page's URL), e.g. "bitcoin",
    /// "ethereum". Shown in this order.
    coins: Vec<String>,
    /// The currency prices are shown in, e.g. "usd", "eur", "btc".
    currency: String,
    /// Warn when a coin fell at least this many percent in 24 hours. Above 0.
    #[schemars(range(min = 0.01))]
    warn_drop: f64,
    /// Read this file instead of calling the API: a saved
    /// `/api/v3/coins/markets` reply. For tests and demos.
    sample: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            coins: crate::strings(&["bitcoin", "ethereum", "solana"]),
            currency: "usd".into(),
            warn_drop: 5.0,
            sample: None,
        }
    }
}

#[derive(Default)]
pub struct Crypto {
    config: Config,
}

#[async_trait]
impl Module for Crypto {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let c = &mut self.config;
        if c.coins.is_empty() || c.coins.len() > MAX_COINS {
            bail!("coins: 1 to {MAX_COINS} CoinGecko ids, e.g. [bitcoin, ethereum]");
        }
        let id_ok = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        };
        if let Some(bad) = c.coins.iter().find(|s| !id_ok(s)) {
            bail!("coins: {bad:?} is not a CoinGecko id (lowercase, like \"bitcoin\")");
        }
        if c.warn_drop <= 0.0 {
            bail!("warn_drop: a percent above 0, e.g. 5");
        }
        c.currency = c.currency.to_lowercase();
        if c.currency.is_empty() || !c.currency.chars().all(|c| c.is_ascii_lowercase()) {
            bail!("currency: a currency code such as usd or eur");
        }
        Ok(manifest(
            "crypto",
            "Crypto prices",
            120_000,
            30_000,
            Size::Medium,
            30,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let markets: Vec<Market> = match &c.sample {
            Some(path) => crate::read_sample(path)?,
            None => fetch(c).await?,
        };
        Ok(view(&markets, c))
    }
}

/// One entry of `/api/v3/coins/markets`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Market {
    id: String,
    symbol: String,
    name: String,
    current_price: Option<f64>,
    price_change_percentage_24h: Option<f64>,
}

async fn fetch(c: &Config) -> Result<Vec<Market>> {
    let resp = client()?
        .get(API)
        .query(&[
            ("vs_currency", c.currency.as_str()),
            ("ids", &c.coins.join(",")),
            ("price_change_percentage", "24h"),
        ])
        .send()
        .await?;
    if resp.status().as_u16() == 429 {
        bail!("CoinGecko's rate limit was hit (HTTP 429); try a longer refresh");
    }
    Ok(check(resp).await?.json().await?)
}

/// A price in `currency`: "$79,764", "$182.90", "$0.1235", "0.0421 BTC".
fn price(value: f64, currency: &str) -> String {
    let number = if value.abs() >= 1000.0 {
        thousands(value.round() as i64)
    } else if value.abs() >= 1.0 {
        format!("{value:.2}")
    } else {
        format!("{value:.4}")
    };
    match currency {
        "usd" => format!("${number}"),
        "eur" => format!("€{number}"),
        "gbp" => format!("£{number}"),
        "jpy" => format!("¥{number}"),
        other => format!("{number} {}", other.to_uppercase()),
    }
}

/// "+1.34%", "-0.53%", or "-" when CoinGecko has no change.
fn change(pct: Option<f64>) -> String {
    match pct {
        Some(p) => format!("{p:+.2}%"),
        None => "-".into(),
    }
}

/// The tile: one row per configured coin, in the config's order.
fn view(markets: &[Market], c: &Config) -> View {
    let mut warned = false;
    let mut rows = vec![];
    let mut summary = None;
    for id in &c.coins {
        let link = Action::OpenUrl {
            url: format!("https://www.coingecko.com/en/coins/{id}"),
        };
        let Some(m) = markets.iter().find(|m| &m.id == id) else {
            // Likely a typo in `coins`: warn, so it shows on a collapsed tile.
            warned = true;
            rows.push(Row {
                cells: [
                    ("coin".to_string(), Cell::Text(id.clone())),
                    ("price".to_string(), Cell::Text("not found".into())),
                    ("change".to_string(), Cell::Text("-".into())),
                ]
                .into(),
                status: Some(Status::Error),
                action: None,
            });
            continue;
        };
        let pct = m.price_change_percentage_24h;
        let fell = pct.is_some_and(|p| p <= -c.warn_drop);
        warned |= fell;
        let symbol = m.symbol.to_uppercase();
        let shown = m
            .current_price
            .map_or("-".into(), |p| price(p, &c.currency));
        summary.get_or_insert_with(|| format!("{symbol} {shown} {}", change(pct)));
        rows.push(Row {
            cells: [
                (
                    "coin".to_string(),
                    Cell::Text(format!("{} {symbol}", m.name)),
                ),
                ("price".to_string(), Cell::Text(shown)),
                ("change".to_string(), Cell::Text(change(pct))),
            ]
            .into(),
            status: fell.then_some(Status::Warn),
            action: Some(link),
        });
    }
    let col = |key: &str, label: &str| Column {
        key: key.into(),
        label: label.into(),
    };
    View {
        status: if warned { Status::Warn } else { Status::Ok },
        summary: summary.unwrap_or_else(|| "no coins found".into()),
        root: Widget::Table {
            columns: vec![
                col("coin", "Coin"),
                col("price", "Price"),
                col("change", "24h"),
            ],
            rows,
        },
    }
}

pub fn catalog() -> CatalogEntry {
    let markets: Vec<Market> =
        serde_json::from_str(include_str!("../testdata/crypto/markets.json"))
            .expect("demo data parses");
    let demo = Config {
        coins: crate::strings(&["bitcoin", "ethereum", "solana", "dogecoin"]),
        ..Config::default()
    };
    CatalogEntry {
        description: "Prices and 24-hour change of the coins you pick, from CoinGecko. Warns \
                      when a coin falls at least a set percent, or a coin id is not found. Click a coin to open its \
                      CoinGecko page. Public API, no key."
            .into(),
        tags: crate::strings(&["crypto", "finance", "prices", "public-api"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: vec![],
            hosts: crate::strings(&["api.coingecko.com"]),
        },
        ..crate::entry(
            "crypto",
            "Crypto prices",
            Size::Medium,
            view(&markets, &demo),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Market> {
        let path = format!(
            "{}/testdata/crypto/markets.json",
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
        let c = Config {
            coins: crate::strings(&["bitcoin", "ethereum", "solana", "dogecoin", "nocoin"]),
            ..Config::default()
        };
        let v = view(&sample(), &c);
        // Dogecoin fell 7%.
        assert_eq!(v.status, Status::Warn);
        assert_eq!(v.summary, "BTC $79,764 -0.53%");
        let Widget::Table { rows, .. } = &v.root else {
            panic!("not a table: {:?}", v.root);
        };
        // The config's order, not the API's.
        assert_eq!(text(&rows[0], "coin"), "Bitcoin BTC");
        assert_eq!(text(&rows[1], "price"), "$3,120");
        assert_eq!(text(&rows[1], "change"), "+1.34%");
        assert_eq!(rows[1].status, None);
        assert_eq!(text(&rows[2], "price"), "$182.90");
        assert_eq!(text(&rows[2], "change"), "-");
        assert_eq!(text(&rows[3], "price"), "$0.1235");
        assert_eq!(rows[3].status, Some(Status::Warn));
        assert_eq!(
            rows[3].action,
            Some(Action::OpenUrl {
                url: "https://www.coingecko.com/en/coins/dogecoin".into()
            })
        );
        // An id CoinGecko doesn't know.
        assert_eq!(text(&rows[4], "price"), "not found");
        assert_eq!(rows[4].status, Some(Status::Error));
    }

    #[test]
    fn no_warning_under_the_threshold() {
        let c = Config {
            warn_drop: 10.0,
            ..Config::default()
        };
        let mut c2 = c;
        c2.coins.push("dogecoin".into());
        assert_eq!(view(&sample(), &c2).status, Status::Ok);
        // A coin CoinGecko doesn't know warns by itself.
        c2.coins.push("nocoin".into());
        assert_eq!(view(&sample(), &c2).status, Status::Warn);
    }

    #[test]
    fn formats_prices() {
        assert_eq!(price(79_764.4, "usd"), "$79,764");
        assert_eq!(price(12.5, "eur"), "€12.50");
        assert_eq!(price(0.042_1, "btc"), "0.0421 BTC");
    }

    #[tokio::test]
    async fn checks_the_config() {
        let mut m = Crypto::default();
        for bad in [
            serde_json::json!({ "coins": [] }),
            serde_json::json!({ "coins": ["Bitcoin"] }),
            serde_json::json!({ "currency": "us$" }),
            serde_json::json!({ "warn_drop": 0 }),
            serde_json::json!({ "warn_drop": -5 }),
        ] {
            assert!(m.init(bad.clone()).await.is_err(), "{bad}");
        }
        m.init(serde_json::json!({ "coins": ["bitcoin"], "currency": "EUR" }))
            .await
            .unwrap();
        assert_eq!(m.config.currency, "eur");
    }
}
