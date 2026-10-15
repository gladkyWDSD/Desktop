//! A desktop notification when a harness finishes a turn while the overlay is
//! hidden.
//!
//! Completion is the same explicit signal the phone's completion alerts use
//! (`completion::collect_watched`: native adapter metadata or a Codex rollout's
//! `task_complete`), never terminal silence. Each turn has its own completion
//! id, so one turn notifies once. A card's first observation only records its
//! state: restored sessions and turns that finished before the daemon started
//! never notify.
//!
//! The watcher is a background thread polling every [`POLL`]. Clicking the
//! notification's **Show** action opens the overlay on that card.
use crate::completion::Completion;
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const POLL: Duration = Duration::from_secs(3);
/// Longest prompt quoted in a notification body.
const PROMPT_CHARS: usize = 120;

static ENABLED: AtomicBool = AtomicBool::new(true);
static OVERLAY_SHOWN: AtomicBool = AtomicBool::new(false);

/// Settings → Harness launchers → "Notify when a harness finishes".
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// The overlay was shown or hidden. While it is on screen, the cards' own
/// status badges say a harness finished; no notification is sent.
pub fn set_overlay_shown(shown: bool) {
    OVERLAY_SHOWN.store(shown, Ordering::Relaxed);
}

/// Last completion id seen per session.
#[derive(Default)]
struct Tracker {
    seen: HashMap<String, Option<String>>,
}

impl Tracker {
    /// Sessions whose newest completion is one this tracker had not seen yet.
    /// A session's first observation never counts, and sessions that are gone
    /// are forgotten.
    fn observe(&mut self, completions: &[Completion]) -> Vec<String> {
        let mut finished = Vec::new();
        let mut next = HashMap::with_capacity(completions.len());
        for c in completions {
            let id = (c.state == "completed").then(|| c.completion_id.clone()).flatten();
            if let Some(previous) = self.seen.get(&c.id) {
                if id.is_some() && *previous != id {
                    finished.push(c.id.clone());
                }
            }
            next.insert(c.id.clone(), id);
        }
        self.seen = next;
        finished
    }
}

/// Notification text for one finished card: summary and body.
fn message(agent: &str, prompt: Option<&str>, dir: Option<&str>) -> (String, String) {
    let (name, _) = crate::harness_bar::harness_label(agent);
    let name = if name == "Shell" && agent.starts_with("custom-") { "Harness" } else { name };
    let summary = format!("{name} finished");
    let mut lines = Vec::new();
    if let Some(prompt) = prompt.map(str::trim).filter(|p| !p.is_empty()) {
        let line = prompt.lines().next().unwrap_or(prompt);
        let mut short: String = line.chars().take(PROMPT_CHARS).collect();
        if line.chars().count() > PROMPT_CHARS || prompt.lines().nth(1).is_some() {
            short.push('…');
        }
        lines.push(short);
    }
    if let Some(dir) = dir.filter(|d| !d.is_empty()) {
        lines.push(crate::state::display_dir(dir));
    }
    (summary, lines.join("\n"))
}

/// Send one notification and wait for its action on this (worker) thread.
/// `true` when the user picked **Show**. A `notify-send` without `--action`
/// support fails, and then nothing is shown: rare, and never a crash.
fn notify(summary: &str, body: &str) -> bool {
    let out = Command::new("notify-send")
        .args([
            "--app-name=COSMIC DESKTOP",
            "--icon=utilities-terminal",
            "--action=show=Show",
            summary,
            body,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    matches!(out, Ok(out) if String::from_utf8_lossy(&out.stdout).trim() == "show")
}

fn poll_once(tracker: &mut Tracker, on_show: &futures_channel::mpsc::UnboundedSender<String>) {
    let state = crate::state::load_state();
    let ids: Vec<String> = state.terminals.iter().map(|t| t.session_name.clone()).collect();
    if ids.is_empty() {
        tracker.observe(&[]);
        return;
    }
    let (completions, _) = crate::completion::collect_watched(&ids);
    let finished = tracker.observe(&completions);
    if finished.is_empty()
        || !ENABLED.load(Ordering::Relaxed)
        || OVERLAY_SHOWN.load(Ordering::Relaxed)
    {
        return;
    }
    for session in finished {
        let Some(card) = state.terminals.iter().find(|t| t.session_name == session) else { continue };
        // The stored prompt passes `harness_record::is_user_prompt` on read,
        // like a card's title: never an injected turn.
        let prompt = crate::harness_metadata::inspect(&session, &card.agent_type).map(|m| m.prompt);
        let (summary, body) = message(&card.agent_type, prompt.as_deref(), card.workspace_dir.as_deref());
        let on_show = on_show.clone();
        let _ = std::thread::Builder::new()
            .name("desktop-workspace-finish-notice".into())
            .spawn(move || {
                if notify(&summary, &body) {
                    let _ = on_show.unbounded_send(session);
                }
            });
    }
}

/// Start watching. `on_show` runs on the GTK main thread with the session of a
/// notification whose **Show** was clicked.
pub fn start(on_show: impl Fn(String) + 'static) {
    let (tx, mut rx) = futures_channel::mpsc::unbounded::<String>();
    let spawned = std::thread::Builder::new()
        .name("desktop-workspace-finish-watch".into())
        .spawn(move || {
            let mut tracker = Tracker::default();
            loop {
                poll_once(&mut tracker, &tx);
                std::thread::sleep(POLL);
            }
        });
    if let Err(error) = spawned {
        eprintln!("COSMIC DESKTOP: could not start finish notifications: {error}");
        return;
    }
    gtk4::glib::MainContext::default().spawn_local(async move {
        use futures_util::StreamExt;
        while let Some(session) = rx.next().await {
            on_show(session);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, state: &str, completion: Option<&str>) -> Completion {
        Completion {
            id: id.into(),
            supported: true,
            state: state.into(),
            completion_id: completion.map(str::to_string),
        }
    }

    #[test]
    fn finish_notify_fires_once_per_new_completion_and_never_on_first_sight() {
        let mut t = Tracker::default();
        // Restored session already finished: recorded, not announced.
        assert!(t.observe(&[c("a", "completed", Some("1")), c("b", "working", None)]).is_empty());
        // Same completion again: nothing.
        assert!(t.observe(&[c("a", "completed", Some("1")), c("b", "working", None)]).is_empty());
        // b finishes, a starts a new turn.
        assert_eq!(t.observe(&[c("a", "working", None), c("b", "completed", Some("7"))]), ["b"]);
        // a finishes its new turn.
        assert_eq!(t.observe(&[c("a", "completed", Some("2")), c("b", "completed", Some("7"))]), ["a"]);
        // A completion id the tracker missed the working phase of still counts.
        assert_eq!(t.observe(&[c("a", "completed", Some("3")), c("b", "completed", Some("7"))]), ["a"]);
    }

    #[test]
    fn finish_notify_ignores_unknown_states_and_forgets_closed_cards() {
        let mut t = Tracker::default();
        assert!(t.observe(&[c("a", "working", None)]).is_empty());
        // "completed" without an id is not a completion.
        assert!(t.observe(&[c("a", "completed", None)]).is_empty());
        assert!(t.observe(&[c("a", "unknown", None)]).is_empty());
        // Closed: forgotten, so a reused name starts over at first sight.
        assert!(t.observe(&[]).is_empty());
        assert!(t.observe(&[c("a", "completed", Some("1"))]).is_empty());
    }

    #[test]
    fn finish_notify_message_names_the_harness_prompt_and_folder() {
        let home = crate::state::home_dir_string();
        let (summary, body) = message("claude", Some("fix the tests\nand more"), Some(&format!("{home}/proj")));
        assert_eq!(summary, "Claude finished");
        assert_eq!(body, "fix the tests…\n~/proj");
        let (summary, body) = message("codex", None, None);
        assert_eq!(summary, "Codex finished");
        assert_eq!(body, "");
        let long = "x".repeat(200);
        let (_, body) = message("pi", Some(&long), None);
        assert_eq!(body.chars().count(), PROMPT_CHARS + 1);
    }
}
