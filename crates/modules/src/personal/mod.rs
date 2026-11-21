//! The personal bundle: weather, the moon, a simple todo list, your calendar
//! (an iCal feed) and your tasks, from org-mode, Markdown (Obsidian Tasks),
//! todo.txt, Taskwarrior and an osdlc sdlc vault. Each module sits behind a cargo feature named after its id.
//!
//! Everything here is local or a plain URL: no accounts, no OAuth, no tokens.
//! The task modules share one list layout: overdue first, then by date.

use crate::common::Builtin;
#[allow(unused_imports)]
pub(crate) use crate::common::{entry, expand_home, manifest, schema, strings};

#[cfg(feature = "module-lunar")]
pub mod lunar;
#[cfg(feature = "module-todo")]
pub mod todo;
// Dates, the task list and file search, for the calendar and task modules.
// A build with only some of them leaves some helpers unused.
#[cfg(any(
    feature = "module-calendar",
    feature = "module-org-agenda",
    feature = "module-markdown-tasks",
    feature = "module-todo-txt",
    feature = "module-taskwarrior",
    feature = "module-sdlc"
))]
#[cfg_attr(
    not(all(
        feature = "module-calendar",
        feature = "module-org-agenda",
        feature = "module-markdown-tasks",
        feature = "module-todo-txt",
        feature = "module-taskwarrior",
        feature = "module-sdlc"
    )),
    allow(dead_code)
)]
mod shared;
#[cfg(any(
    feature = "module-calendar",
    feature = "module-org-agenda",
    feature = "module-markdown-tasks",
    feature = "module-todo-txt",
    feature = "module-taskwarrior",
    feature = "module-sdlc"
))]
pub(crate) use shared::*;

#[cfg(feature = "module-calendar")]
pub mod calendar;
#[cfg(feature = "module-calendar")]
mod ics;
#[cfg(feature = "module-markdown-tasks")]
pub mod markdown;
#[cfg(feature = "module-org-agenda")]
pub mod org;
#[cfg(feature = "module-sdlc")]
pub mod sdlc;
#[cfg(feature = "module-taskwarrior")]
pub mod taskwarrior;
#[cfg(feature = "module-todo-txt")]
pub mod todotxt;
#[cfg(feature = "module-weather")]
pub mod weather;

/// The modules this build includes, in display order.
pub const MODULES: &[Builtin] = &[
    #[cfg(feature = "module-weather")]
    Builtin {
        id: "weather",
        core: false,
        new: || Box::new(weather::Weather::default()),
        catalog: weather::catalog,
    },
    #[cfg(feature = "module-todo")]
    Builtin {
        id: "todo",
        core: false,
        new: || Box::new(todo::Todo::default()),
        catalog: todo::catalog,
    },
    #[cfg(feature = "module-lunar")]
    Builtin {
        id: "lunar",
        core: false,
        new: || Box::new(lunar::Lunar),
        catalog: lunar::catalog,
    },
    #[cfg(feature = "module-calendar")]
    Builtin {
        id: "calendar",
        core: false,
        new: || Box::new(calendar::Calendar::default()),
        catalog: calendar::catalog,
    },
    #[cfg(feature = "module-org-agenda")]
    Builtin {
        id: "org-agenda",
        core: false,
        new: || Box::new(org::OrgAgenda::default()),
        catalog: org::catalog,
    },
    #[cfg(feature = "module-markdown-tasks")]
    Builtin {
        id: "markdown-tasks",
        core: false,
        new: || Box::new(markdown::MarkdownTasks::default()),
        catalog: markdown::catalog,
    },
    #[cfg(feature = "module-todo-txt")]
    Builtin {
        id: "todo-txt",
        core: false,
        new: || Box::new(todotxt::TodoTxt::default()),
        catalog: todotxt::catalog,
    },
    #[cfg(feature = "module-taskwarrior")]
    Builtin {
        id: "taskwarrior",
        core: false,
        new: || Box::new(taskwarrior::Taskwarrior::default()),
        catalog: taskwarrior::catalog,
    },
    #[cfg(feature = "module-sdlc")]
    Builtin {
        id: "sdlc",
        core: false,
        new: || Box::new(sdlc::Sdlc::default()),
        catalog: sdlc::catalog,
    },
];
