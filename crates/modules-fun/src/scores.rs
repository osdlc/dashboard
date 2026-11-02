//! Sports scores: today's games of a league, live ones first, from ESPN's
//! public scoreboard API (no key; ESPN does not document it, so it can
//! change). Clicking a game opens its ESPN page.
//!
//! ```yaml
//! modules:
//!   scores:
//!     config:
//!       league: basketball/nba   # or football/nfl, hockey/nhl, baseball/mlb,
//!                                # soccer/eng.1, soccer/usa.1, ...
//!       teams: [BOS, LAL]        # only these teams' games; default all
//! ```

use anyhow::{Result, bail};
use async_trait::async_trait;
use dashboard_contract::{
    Action, CatalogEntry, ListItem, Manifest, Requirements, Size, Status, View, Widget,
};
use dashboard_host::Module;
use dashboard_modules_common::http::{check, client};
use dashboard_modules_common::manifest;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

const API: &str = "https://site.api.espn.com/apis/site/v2/sports";

/// Most games shown.
const MAX_LIMIT: usize = 50;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Config {
    /// Sport and league as ESPN names them, e.g. "basketball/nba",
    /// "football/nfl", "hockey/nhl", "baseball/mlb", "soccer/eng.1".
    league: String,
    /// Only games with one of these teams: abbreviations ("BOS") or whole
    /// words of the name ("Celtics", "Los Angeles"), ignoring case. Default:
    /// every game.
    teams: Vec<String>,
    /// Games shown, at most 50.
    #[schemars(range(min = 1, max = 50))]
    limit: usize,
    /// Read this file instead of calling ESPN: a saved scoreboard reply. For
    /// tests and demos.
    #[schemars(extend("x-path" = true))]
    sample: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            league: "basketball/nba".into(),
            teams: vec![],
            limit: 15,
            sample: None,
        }
    }
}

#[derive(Default)]
pub struct Scores {
    config: Config,
}

#[async_trait]
impl Module for Scores {
    async fn init(&mut self, config: Value) -> Result<Manifest> {
        if !config.is_null() {
            self.config = serde_json::from_value(config)?;
        }
        let c = &mut self.config;
        if c.limit == 0 || c.limit > MAX_LIMIT {
            bail!("limit: 1 to {MAX_LIMIT}");
        }
        c.league = c.league.to_lowercase();
        let part_ok = |p: &str| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        };
        match c.league.split_once('/') {
            Some((sport, league)) if part_ok(sport) && part_ok(league) => {}
            _ => bail!("league: sport/league, e.g. basketball/nba or soccer/eng.1"),
        }
        Ok(manifest(
            "scores",
            "Scores",
            60_000,
            15_000,
            Size::Medium,
            30,
        ))
    }

    async fn render(&mut self) -> Result<View> {
        let c = &self.config;
        let board: Scoreboard = match &c.sample {
            Some(path) => crate::read_sample(path)?,
            None => {
                let url = format!("{API}/{}/scoreboard", c.league);
                let resp = client()?.get(&url).send().await?;
                if resp.status().as_u16() == 404 {
                    bail!("ESPN has no league {:?}", c.league);
                }
                check(resp).await?.json().await?
            }
        };
        Ok(view(&board, c))
    }
}

/// ESPN's scoreboard reply, with only the fields the tile shows.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Scoreboard {
    leagues: Vec<League>,
    events: Vec<Event>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct League {
    abbreviation: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Event {
    short_name: String,
    status: EventStatus,
    competitions: Vec<Competition>,
    links: Vec<Link>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct EventStatus {
    #[serde(rename = "type")]
    kind: StatusType,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StatusType {
    /// "pre", "in" or "post".
    state: String,
    short_detail: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Competition {
    competitors: Vec<Competitor>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Competitor {
    home_away: String,
    score: String,
    team: Team,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Team {
    abbreviation: String,
    display_name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Link {
    rel: Vec<String>,
    href: String,
}

impl Event {
    /// The away team, then the home team.
    fn sides(&self) -> Option<(&Competitor, &Competitor)> {
        let teams = &self.competitions.first()?.competitors;
        let away = teams.iter().find(|t| t.home_away == "away")?;
        let home = teams.iter().find(|t| t.home_away == "home")?;
        Some((away, home))
    }

    fn has_team(&self, wanted: &[String]) -> bool {
        let Some(comp) = self.competitions.first() else {
            return false;
        };
        comp.competitors.iter().any(|c| {
            wanted.iter().any(|w| {
                c.team.abbreviation.eq_ignore_ascii_case(w) || has_words(&c.team.display_name, w)
            })
        })
    }

    /// Live games first, then upcoming, then finished.
    fn rank(&self) -> u8 {
        match self.status.kind.state.as_str() {
            "in" => 0,
            "pre" => 1,
            _ => 2,
        }
    }

    /// The game's ESPN page: the "summary" link, else ESPN's front page.
    fn link(&self) -> String {
        self.links
            .iter()
            .find(|l| l.rel.iter().any(|r| r == "summary"))
            .or(self.links.first())
            .map(|l| l.href.clone())
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "https://www.espn.com/".into())
    }
}

/// Whether `name` holds the words of `wanted`, in order and whole, ignoring
/// case: "Golden State Warriors" has "state warriors" but not "den".
fn has_words(name: &str, wanted: &str) -> bool {
    let words = |s: &str| -> Vec<String> { s.split_whitespace().map(str::to_lowercase).collect() };
    let (name, wanted) = (words(name), words(wanted));
    !wanted.is_empty() && name.windows(wanted.len()).any(|w| w == wanted.as_slice())
}

/// One game: "BOS 78 – NY 71" (away first), or "DEN @ LAL" before it starts.
/// Football (soccer) puts the home team first: "CRY 0 – NEW 2", "CRY v NEW".
fn game(e: &Event, home_first: bool) -> ListItem {
    let state = e.status.kind.state.as_str();
    let title = match e.sides() {
        Some((away, home)) => {
            let (a, b) = if home_first {
                (home, away)
            } else {
                (away, home)
            };
            let (a_name, b_name) = (&a.team.abbreviation, &b.team.abbreviation);
            match (state, home_first) {
                ("pre", false) => format!("{a_name} @ {b_name}"),
                ("pre", true) => format!("{a_name} v {b_name}"),
                _ => format!("{a_name} {} – {b_name} {}", a.score, b.score),
            }
        }
        None => e.short_name.clone(),
    };
    let live = state == "in";
    ListItem {
        tone: None,
        title,
        subtitle: Some(e.status.kind.short_detail.clone()).filter(|s| !s.is_empty()),
        status: live.then_some(Status::Ok),
        tags: if live { vec!["live".into()] } else { vec![] },
        action: Some(Action::OpenUrl { url: e.link() }),
    }
}

fn view(board: &Scoreboard, c: &Config) -> View {
    let mut events: Vec<&Event> = board
        .events
        .iter()
        .filter(|e| c.teams.is_empty() || e.has_team(&c.teams))
        .collect();
    events.sort_by_key(|e| e.rank());
    let live = events.iter().filter(|e| e.rank() == 0).count();
    let home_first = c.league.starts_with("soccer/");
    let items: Vec<ListItem> = events
        .into_iter()
        .take(c.limit)
        .map(|e| game(e, home_first))
        .collect();
    let league = board
        .leagues
        .first()
        .map(|l| l.abbreviation.clone())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| c.league.to_uppercase());
    let mut summary = league;
    if live > 0 {
        summary.push_str(&format!(" · {live} live"));
    }
    summary.push_str(&match items.len() {
        0 => " · no games".to_string(),
        1 => " · 1 game".to_string(),
        n => format!(" · {n} games"),
    });
    View {
        status: Status::Ok,
        summary,
        root: Widget::List {
            items,
            empty_text: Some("no games today".into()),
        },
    }
}

pub fn catalog() -> CatalogEntry {
    let board: Scoreboard = serde_json::from_str(include_str!("../testdata/scores/nba.json"))
        .expect("demo data parses");
    CatalogEntry {
        description: "Today's games and scores for a league (NBA, NFL, NHL, MLB, football \
                      leagues and more), live games first, optionally only your teams. Click \
                      a game to open it on ESPN. ESPN's public scoreboard API: no key, but \
                      undocumented."
            .into(),
        tags: crate::strings(&["sports", "scores", "public-api"]),
        config_schema: crate::schema::<Config>(),
        requirements: Requirements {
            binaries: vec![],
            env: vec![],
            hosts: crate::strings(&["site.api.espn.com"]),
        },
        ..crate::entry(
            "scores",
            "Scores",
            Size::Medium,
            view(&board, &Config::default()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Scoreboard {
        let path = format!("{}/testdata/scores/nba.json", env!("CARGO_MANIFEST_DIR"));
        crate::read_sample(&path).unwrap()
    }

    #[test]
    fn parses_the_sample() {
        let v = view(&sample(), &Config::default());
        assert_eq!(v.summary, "NBA · 1 live · 4 games");
        let Widget::List { items, .. } = &v.root else {
            panic!("not a list: {:?}", v.root);
        };
        // Live first, then upcoming, then finished in the API's order.
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "BOS 78 – NY 71",
                "DEN @ LAL",
                "WSH 103 – CHA 110",
                "MIN 125 – CLE 127"
            ]
        );
        assert_eq!(items[0].subtitle.as_deref(), Some("5:12 - 3rd"));
        assert_eq!(items[0].status, Some(Status::Ok));
        assert_eq!(items[0].tags, ["live"]);
        assert_eq!(items[3].subtitle.as_deref(), Some("Final/OT"));
        assert_eq!(items[3].status, None);
        // The summary link; no links at all: ESPN's front page.
        assert_eq!(
            items[2].action,
            Some(Action::OpenUrl {
                url: "https://www.espn.com/nba/game/_/gameId/401900001/wizards-hornets".into()
            })
        );
        assert_eq!(
            items[1].action,
            Some(Action::OpenUrl {
                url: "https://www.espn.com/".into()
            })
        );
    }

    #[test]
    fn football_puts_the_home_team_first() {
        let c = Config {
            league: "soccer/eng.1".into(),
            ..Config::default()
        };
        let v = view(&sample(), &c);
        let Widget::List { items, .. } = &v.root else {
            panic!()
        };
        assert_eq!(items[0].title, "NY 71 – BOS 78");
        assert_eq!(items[1].title, "LAL v DEN");
    }

    #[test]
    fn filters_by_team() {
        let c = Config {
            teams: crate::strings(&["lal", "Cavaliers"]),
            ..Config::default()
        };
        let v = view(&sample(), &c);
        assert_eq!(v.summary, "NBA · 2 games");
        let c = Config {
            teams: crate::strings(&["XYZ"]),
            ..Config::default()
        };
        assert_eq!(view(&sample(), &c).summary, "NBA · no games");
    }

    #[test]
    fn team_names_match_whole_words() {
        assert!(has_words("Golden State Warriors", "warriors"));
        assert!(has_words("Golden State Warriors", "Golden State"));
        assert!(has_words("Los Angeles Lakers", "los angeles"));
        assert!(!has_words("Golden State Warriors", "DEN"));
        assert!(!has_words("Sacramento Kings", "SA"));
        assert!(!has_words("Atlanta Hawks", "LA"));
        assert!(!has_words("Golden State Warriors", "State Golden"));
        assert!(!has_words("Golden State Warriors", " "));
        // An abbreviation matches only itself.
        let c = Config {
            teams: crate::strings(&["DEN"]),
            ..Config::default()
        };
        let v = view(&sample(), &c);
        let Widget::List { items, .. } = &v.root else {
            panic!()
        };
        assert_eq!(items.len(), 1);
        assert!(items[0].title.contains("DEN"));
    }

    #[tokio::test]
    async fn checks_the_config() {
        let mut m = Scores::default();
        for bad in [
            serde_json::json!({ "league": "nba" }),
            serde_json::json!({ "league": "basketball/../nba" }),
            serde_json::json!({ "limit": 0 }),
        ] {
            assert!(m.init(bad.clone()).await.is_err(), "{bad}");
        }
        m.init(serde_json::json!({ "league": "Soccer/eng.1" }))
            .await
            .unwrap();
        assert_eq!(m.config.league, "soccer/eng.1");
    }
}
