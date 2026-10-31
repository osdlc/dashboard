//! The fun bundle: the demo module, with made-up data in every widget style,
//! a clock for one or more time zones, Hacker News, the feed reader, and the
//! public-API widgets (subreddit, dev.to, mempool, crypto prices, sports
//! scores). Each module sits behind a cargo feature named after its id.

use dashboard_modules_common::Builtin;
#[allow(unused_imports)]
pub(crate) use dashboard_modules_common::{entry, schema, strings};

#[cfg(feature = "clock")]
pub mod clock;
#[cfg(feature = "crypto")]
pub mod crypto;
#[cfg(feature = "demo")]
pub mod demo;
#[cfg(feature = "devto")]
pub mod devto;
#[cfg(feature = "feedreader")]
pub mod feedreader;
#[cfg(feature = "hackernews")]
pub mod hackernews;
#[cfg(feature = "mempool")]
pub mod mempool;
#[cfg(feature = "scores")]
pub mod scores;
#[cfg(feature = "subreddit")]
pub mod subreddit;

/// The modules this build includes, in display order.
pub const MODULES: &[Builtin] = &[
    #[cfg(feature = "demo")]
    Builtin {
        id: "demo",
        core: false,
        new: || Box::new(demo::Demo::default()),
        catalog: demo::catalog,
    },
    #[cfg(feature = "clock")]
    Builtin {
        id: "clock",
        core: false,
        new: || Box::new(clock::Clock::default()),
        catalog: clock::catalog,
    },
    #[cfg(feature = "hackernews")]
    Builtin {
        id: "hackernews",
        core: false,
        new: || Box::new(hackernews::HackerNews::default()),
        catalog: hackernews::catalog,
    },
    #[cfg(feature = "feedreader")]
    Builtin {
        id: "feedreader",
        core: false,
        new: || Box::new(feedreader::FeedReader::default()),
        catalog: feedreader::catalog,
    },
    #[cfg(feature = "subreddit")]
    Builtin {
        id: "subreddit",
        core: false,
        new: || Box::new(subreddit::Subreddit::default()),
        catalog: subreddit::catalog,
    },
    #[cfg(feature = "devto")]
    Builtin {
        id: "devto",
        core: false,
        new: || Box::new(devto::DevTo::default()),
        catalog: devto::catalog,
    },
    #[cfg(feature = "mempool")]
    Builtin {
        id: "mempool",
        core: false,
        new: || Box::new(mempool::Mempool::default()),
        catalog: mempool::catalog,
    },
    #[cfg(feature = "crypto")]
    Builtin {
        id: "crypto",
        core: false,
        new: || Box::new(crypto::Crypto::default()),
        catalog: crypto::catalog,
    },
    #[cfg(feature = "scores")]
    Builtin {
        id: "scores",
        core: false,
        new: || Box::new(scores::Scores::default()),
        catalog: scores::catalog,
    },
];

/// "5m ago" style age from seconds.
#[allow(dead_code)]
pub(crate) fn ago(secs: i64) -> String {
    match secs.max(0) {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// "1 comment", "12 comments".
#[allow(dead_code)]
pub(crate) fn plural(n: i64, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Reads a saved API reply (a module's `sample:` file) as `T`.
#[allow(dead_code)]
pub(crate) fn read_sample<T: serde::de::DeserializeOwned>(path: &str) -> anyhow::Result<T> {
    use anyhow::Context;
    let path = dashboard_modules_common::expand_home(path);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// 1234567 as "1,234,567".
#[allow(dead_code)]
pub(crate) fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    #[test]
    fn thousands() {
        assert_eq!(super::thousands(0), "0");
        assert_eq!(super::thousands(999), "999");
        assert_eq!(super::thousands(1000), "1,000");
        assert_eq!(super::thousands(973_386), "973,386");
        assert_eq!(super::thousands(-1_234_567), "-1,234,567");
    }
}
