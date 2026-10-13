//! The harness launch bar: one implementation for the local workspace and for a
//! remote PC's workspace. Only the source differs.
//!
//! The bar owns the buttons, their labels, logos and tooltips, the order they
//! appear in, and the one line under them that explains why a click was refused.
//! Whoever owns the harnesses supplies the list, what a click means (launch
//! here, or ask that PC to launch), and whether a click is accepted at all.
//! Nothing here decides how a harness starts: the machine that runs it does.
use crate::desktop_protocol::WorkspaceSnapshot;
use crate::peer_client::Peer;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// Space kept free on both sides of the launchers, the same as between two of
/// them, so a launcher never touches the button before or after the group.
const LAUNCHER_GAP: i32 = 10;

/// Display name and icon of one harness. One source of truth so the local HUD
/// and a remote host's bar cannot label the same harness differently.
pub fn harness_label(key: &str) -> (&'static str, &'static str) {
    match key {
        "antigravity" => ("Antigravity", "🌌"),
        "claude" => ("Claude", "⚡"),
        "codex" => ("Codex", "🤖"),
        "opencode" => ("OpenCode", "🔮"),
        "grok" => ("Grok", "🚀"),
        "reasonix" => ("Reasonix", "🧭"),
        "dsh" => ("DeepSeek", "🐋"),
        "aider" => ("Aider", "🧠"),
        "gemini" => ("Gemini", "✦"),
        "hermes" => ("Hermes", "🪽"),
        "pi" => ("Pi", "🥧"),
        "openclaw" => ("OpenClaw", "🦞"),
        "goose" => ("Goose", "🪿"),
        "qwen" => ("Qwen", "🌟"),
        "crush" => ("Crush", "💘"),
        "kimi" => ("Kimi", "🌙"),
        "kiro" => ("Kiro", "🧰"),
        "cursor" => ("Cursor", "🎯"),
        "herder" => ("Herder", "🐑"),
        _ => ("Shell", "💻"),
    }
}

/// What launching this harness does. The flags are applied by the machine that
/// runs the harness, so the same wording is true on either side.
pub fn harness_tooltip(key: &str) -> &'static str {
    match key {
        "antigravity" => "Launch Antigravity CLI (--dangerously-skip-permissions)",
        "claude" => "Launch Claude Code (--dangerously-skip-permissions)",
        "codex" => "Launch OpenAI Codex (--dangerously-bypass-approvals-and-sandbox)",
        "opencode" => "Launch OpenCode (--auto)",
        "grok" => "Launch Grok CLI (--dangerously-skip-permissions)",
        "reasonix" => "Launch Reasonix (reasonix code, else npx -y reasonix code)",
        "dsh" => "Launch DeepSeek Harness (dsh-tui)",
        "aider" => "Launch Aider (--yes-always)",
        "gemini" => "Launch Gemini CLI (no longer maintained; Antigravity replaces it)",
        "hermes" => "Launch Hermes Agent",
        "pi" => "Launch Pi coding agent",
        "openclaw" => "Launch OpenClaw TUI connected to its gateway",
        "goose" => "Launch Goose interactive session",
        "qwen" => "Launch Qwen Code",
        "crush" => "Launch Crush coding agent",
        "kimi" => "Launch Kimi Code",
        "kiro" => "Launch Kiro CLI",
        "cursor" => "Launch Cursor Agent CLI",
        "herder" => "Launch Herder job worker (not an interactive chat)",
        _ => "Launch Terminal Shell",
    }
}

/// Launch one harness on whichever machine owns it.
type LaunchAction = Rc<dyn Fn(&str)>;
/// Attach a hover card to one harness button, and report whether it did: a
/// button with one keeps the native tooltip off, so the two never render on top
/// of each other.
type HoverCard = Rc<dyn Fn(&gtk4::Button, &str) -> bool>;

/// One launch target's state: the keys to offer, in the order to offer them,
/// and whether a click is accepted right now.
#[derive(Clone, Debug, PartialEq)]
pub struct HarnessState {
    pub keys: Vec<String>,
    pub custom: Vec<CustomButton>,
    pub ready: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CustomButton { pub id: String, pub name: String, pub icon: String }

impl From<&crate::custom_harness::CustomHarness> for CustomButton {
    fn from(value: &crate::custom_harness::CustomHarness) -> Self {
        Self { id: value.id.clone(), name: value.name.clone(), icon: value.icon.clone() }
    }
}

impl HarnessState {
    /// Nothing to launch: used before a host has said what it can run.
    pub fn none() -> Self {
        Self {
            keys: Vec::new(),
            custom: Vec::new(),
            ready: false,
        }
    }

    /// The keys a host's snapshot reports it can run, in `HARNESS_KEYS` order.
    pub fn of_snapshot(snapshot: &WorkspaceSnapshot, ready: bool) -> Self {
        Self {
            keys: crate::tmux::HARNESS_KEYS.iter()
                .filter(|key| snapshot.local.visible_harnesses.iter().any(|offered| offered == **key))
                .map(|key| (*key).to_string())
                .chain(snapshot.local.visible_harnesses.iter()
                    .filter(|key| key.starts_with("custom-")).cloned())
                .collect(),
            custom: snapshot.local.harness_types.iter()
                .filter(|item| item.id.starts_with("custom-") && snapshot.local.visible_harnesses.contains(&item.id))
                .map(|item| CustomButton { id: item.id.clone(), name: item.name.clone(),
                    icon: item.icon.clone().unwrap_or_else(|| "💻".into()) }).collect(),
            ready,
        }
    }
}

/// How a launcher shows its name, so a crowded bar can drop it and keep the
/// icon.
enum LauncherName {
    /// A logo button: the name is its own label beside the image.
    Label(gtk4::Label),
    /// An emoji button: the whole button text, and the emoji alone.
    Text { full: String, short: String },
}

/// The order in which launchers lose their names when the bar is too wide:
/// the least launched first, and among equals the one further right. The most
/// used keep their names longest.
pub fn collapse_order(visible: &[String], usage: &BTreeMap<String, u64>) -> Vec<String> {
    let mut order: Vec<(usize, &String)> = visible.iter().enumerate().collect();
    order.sort_by_key(|(index, key)| {
        (usage.get(*key).copied().unwrap_or(0), std::cmp::Reverse(*index))
    });
    order.into_iter().map(|(_, key)| key.clone()).collect()
}

/// Harness buttons for one workspace, local or remote.
pub struct HarnessBar {
    /// The launch group, centered in a top bar exactly like the local one.
    pub group: gtk4::Box,
    /// One line under the group. Empty and hidden unless it has something to
    /// say: a launch in flight, a refusal, or a host that cannot be launched
    /// into yet.
    pub note: gtk4::Label,
    buttons: RefCell<Vec<(String, gtk4::Button)>>,
    /// Logo images, so a theme switch can swap them like the local bar does.
    images: Rc<RefCell<Vec<(gtk4::Image, String)>>>,
    on_launch: LaunchAction,
    /// A hover card for one harness, when this source has one (this PC's usage
    /// numbers only: another PC's usage is not this machine's business).
    on_hover: HoverCard,
    /// Whether this workspace can be launched into right now.
    ready: Cell<bool>,
    busy: Cell<bool>,
    /// A refusal is explained until the user tries again.
    failed: Cell<bool>,
    /// Bumped by every new line, so a self-dismissing notice only clears the
    /// line it put there.
    note_generation: Cell<u64>,
    names: RefCell<HashMap<String, LauncherName>>,
    /// Launches per harness on this PC; empty for another PC's bar.
    usage: RefCell<BTreeMap<String, u64>>,
    /// The width, launchers and resulting width of the last fit, so a bar
    /// whose inputs did not change is not re-measured on every frame.
    fitted: RefCell<Option<(i32, Vec<String>, i32)>>,
    fit_handlers: RefCell<Option<(glib::WeakRef<gtk4::gdk::FrameClock>, Vec<glib::SignalHandlerId>)>>,
}

impl HarnessBar {
    /// `on_hover` returns whether it attached a hover card of its own: a
    /// harness with one keeps the native tooltip off, so the two never render
    /// on top of each other.
    pub fn new(
        on_launch: LaunchAction,
        on_hover: HoverCard,
    ) -> Rc<Self> {
        let group = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        group.add_css_class("hud-launchers");
        group.set_halign(gtk4::Align::Center);
        group.set_valign(gtk4::Align::Center);
        // The top bar centers the group but pushes it against a wide left
        // side (brand, folder field, sticky-note button); the margins keep it
        // apart.
        group.set_margin_start(LAUNCHER_GAP);
        group.set_margin_end(LAUNCHER_GAP);
        let note = gtk4::Label::new(None);
        note.add_css_class("term-preview-text");
        note.set_halign(gtk4::Align::Center);
        note.set_visible(false);

        let light_theme = crate::theme::current_theme().mode == "light";
        let mut buttons = Vec::new();
        let mut names = HashMap::new();
        let images = Rc::new(RefCell::new(Vec::new()));
        for key in crate::tmux::HARNESS_KEYS.iter().copied() {
            let (name, emoji) = harness_label(key);
            let button = gtk4::Button::new();
            button.add_css_class("hud-button");
            if let Some(logo) = crate::brand::logo_path(key, light_theme) {
                let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
                let image = gtk4::Image::from_file(&logo);
                image.set_pixel_size(crate::brand::BRAND_ICON_SIZE);
                row.append(&image);
                let label = gtk4::Label::new(Some(name));
                row.append(&label);
                button.set_child(Some(&row));
                images.borrow_mut().push((image, key.to_string()));
                names.insert(key.to_string(), LauncherName::Label(label));
            } else {
                let full = format!("{emoji} {name}");
                button.set_label(&full);
                names.insert(key.to_string(), LauncherName::Text { full, short: emoji.to_string() });
            }
            // Built once and shown or hidden, so the layout never jumps while
            // the user is aiming at a button.
            button.set_visible(false);
            button.set_sensitive(false);
            buttons.push((key.to_string(), button.clone()));
            group.append(&button);
        }

        let bar = Rc::new(Self {
            group,
            note,
            buttons: RefCell::new(buttons),
            images,
            on_launch,
            on_hover,
            ready: Cell::new(false),
            busy: Cell::new(false),
            failed: Cell::new(false),
            note_generation: Cell::new(0),
            names: RefCell::new(names),
            usage: RefCell::new(BTreeMap::new()),
            fitted: RefCell::new(None),
            fit_handlers: RefCell::new(None),
        });
        for (key, button) in bar.buttons.borrow().iter() {
            let weak = Rc::downgrade(&bar);
            let launched = key.clone();
            button.connect_clicked(move |_| {
                if let Some(bar) = weak.upgrade() {
                    (bar.on_launch)(&launched);
                }
            });
            if !(bar.on_hover)(button, key.as_str()) {
                button.set_tooltip_text(Some(harness_tooltip(key)));
            }
        }
        bar
    }

    /// Offer this set of harnesses. Called by the local workspace whenever the
    /// user changes the setting, and by a remote view on every snapshot.
    pub fn apply(self: &Rc<Self>, state: &HarnessState) {
        let builtins = crate::tmux::HARNESS_KEYS;
        let stale = self.buttons.borrow().iter()
            .filter(|(key, _)| !builtins.contains(&key.as_str()) && !state.custom.iter().any(|item| item.id == *key))
            .map(|(key, _)| key.clone()).collect::<Vec<_>>();
        for key in stale {
            if let Some((_, button)) = self.buttons.borrow_mut().iter().find(|(id, _)| id == &key).cloned() {
                self.group.remove(&button);
            }
            self.buttons.borrow_mut().retain(|(id, _)| id != &key);
            self.names.borrow_mut().remove(&key);
        }
        for item in &state.custom {
            let full = format!("{} {}", item.icon, item.name);
            self.names.borrow_mut().insert(
                item.id.clone(),
                LauncherName::Text { full: full.clone(), short: item.icon.clone() },
            );
            if let Some((_, button)) = self.buttons.borrow().iter().find(|(key, _)| key == &item.id) {
                button.set_label(&full);
                button.set_tooltip_text(Some(&format!("Launch {}", item.name)));
                continue;
            }
            let button = gtk4::Button::with_label(&format!("{} {}", item.icon, item.name));
            button.add_css_class("hud-button");
            if let Some(label) = button.child().and_downcast::<gtk4::Label>() {
                label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                label.set_max_width_chars(18);
            }
            button.set_tooltip_text(Some(&format!("Launch {}", item.name)));
            let key = item.id.clone();
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(bar) = weak.upgrade() { (bar.on_launch)(&key); }
            });
            self.group.append(&button);
            self.buttons.borrow_mut().push((item.id.clone(), button));
        }
        for (key, button) in self.buttons.borrow().iter() {
            button.set_visible(state.keys.iter().any(|offered| offered == key));
        }
        // Renamed custom launchers must be measured again.
        self.fitted.replace(None);
        self.ready.set(state.ready);
        self.paint_sensitivity();
    }

    /// Launch counts by harness key, which decide whose names stay when the
    /// bar is crowded.
    pub fn set_usage(&self, usage: &BTreeMap<String, u64>) {
        if *self.usage.borrow() != *usage {
            *self.usage.borrow_mut() = usage.clone();
            self.fitted.replace(None);
        }
    }

    /// Fit the launchers into `available` logical pixels: with every name if
    /// they fit, otherwise the least used show only their icon (see
    /// [`collapse_order`]). Their tooltips still name them.
    pub fn fit(&self, available: i32) {
        // No allocation yet: keep the names until there is a real width.
        if available <= 0 {
            return;
        }
        let visible: Vec<String> = self.buttons.borrow().iter()
            .filter(|(_, button)| button.is_visible())
            .map(|(key, _)| key.clone())
            .collect();
        let width = || self.group.measure(gtk4::Orientation::Horizontal, -1).1;
        // The resulting width is part of the key, so a change of toolbar size
        // or theme, which resizes the same buttons, is fitted again.
        if let Some((fitted_for, fitted_keys, fitted_width)) = self.fitted.borrow().as_ref() {
            if *fitted_for == available && *fitted_keys == visible && *fitted_width == width() {
                return;
            }
        }
        for key in &visible {
            self.show_name(key, true);
        }
        for key in collapse_order(&visible, &self.usage.borrow()) {
            if width() <= available {
                break;
            }
            self.show_name(&key, false);
        }
        *self.fitted.borrow_mut() = Some((available, visible, width()));
    }

    /// Keep the launchers clear of `left` (the brand and the folder field) in
    /// the top bar built by `window::top_bar_content`, on every layout of the
    /// bar: after output, scale, window size or toolbar size changes, and when
    /// launchers are added or removed. Event-driven like the toolbar's own
    /// width (`window::bind_top_bar_width`): no tick callback.
    pub fn bind_fit(self: &Rc<Self>, left: &impl IsA<gtk4::Widget>) {
        let fit: Rc<dyn Fn()> = {
            let bar = Rc::downgrade(self);
            let left = left.as_ref().downgrade();
            Rc::new(move || {
                let (Some(bar), Some(left)) = (bar.upgrade(), left.upgrade()) else {
                    return;
                };
                let Some(row) = bar.group.ancestor(gtk4::CenterBox::static_type()) else {
                    return;
                };
                if row.width() <= 0 {
                    return;
                }
                let left_width = if left.is_visible() {
                    left.measure(gtk4::Orientation::Horizontal, -1).1
                } else {
                    0
                };
                // The group's measure includes its LAUNCHER_GAP margins.
                bar.fit(row.width() - left_width);
            })
        };
        let connect = {
            let bar = Rc::downgrade(self);
            move |group: &gtk4::Box| {
                let Some(bar) = bar.upgrade() else { return };
                bar.unbind_fit();
                let Some(clock) = group.frame_clock() else { return };
                let on_layout = Rc::clone(&fit);
                let on_paint = Rc::clone(&fit);
                let handlers = vec![
                    clock.connect_layout(move |_| on_layout()),
                    clock.connect_after_paint(move |_| on_paint()),
                ];
                *bar.fit_handlers.borrow_mut() = Some((clock.downgrade(), handlers));
                fit();
            }
        };
        if self.group.is_realized() {
            connect(&self.group);
        }
        self.group.connect_realize(connect);
        let bar = Rc::downgrade(self);
        self.group.connect_unrealize(move |_| {
            if let Some(bar) = bar.upgrade() {
                bar.unbind_fit();
            }
        });
    }

    fn unbind_fit(&self) {
        if let Some((clock, handlers)) = self.fit_handlers.borrow_mut().take() {
            if let Some(clock) = clock.upgrade() {
                for handler in handlers {
                    clock.disconnect(handler);
                }
            }
        }
    }

    fn show_name(&self, key: &str, shown: bool) {
        match self.names.borrow().get(key) {
            Some(LauncherName::Label(label)) => label.set_visible(shown),
            Some(LauncherName::Text { full, short }) => {
                if let Some((_, button)) = self.buttons.borrow().iter().find(|(id, _)| id == key) {
                    let text = if shown { full } else { short };
                    if button.label().as_deref() != Some(text.as_str()) {
                        button.set_label(text);
                    }
                }
            }
            None => {}
        }
    }

    /// Say something under the bar, or clear it with an empty string. A launch
    /// in flight, or a refusal the user has not answered, keeps its own line.
    pub fn note(&self, text: &str) {
        if self.busy.get() || self.failed.get() {
            return;
        }
        self.set_note(text);
    }

    /// One launch is in flight. Another click waits for it instead of making a
    /// second card on a host that is only slow to answer.
    pub fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.paint_sensitivity();
        if !busy {
            return;
        }
        self.set_note("Launching…");
    }

    /// Report one command's result under the bar and let it go by itself.
    ///
    /// Like [`Self::fail`], it holds the line against routine snapshot notes
    /// while shown; when it expires the line clears (unless something newer has
    /// replaced it) and the next snapshot says what it normally says.
    pub fn flash(self: &Rc<Self>, notice: crate::command_feedback::Notice) {
        self.failed.set(true);
        self.set_note(notice.text);
        for class in crate::command_feedback::TONE_CLASSES {
            self.note.remove_css_class(class);
        }
        self.note.add_css_class(notice.tone.css_class());
        let generation = self.note_generation.get();
        let weak = Rc::downgrade(self);
        gtk4::glib::timeout_add_local_once(notice.duration, move || {
            let Some(bar) = weak.upgrade() else {
                return;
            };
            if bar.note_generation.get() == generation && !bar.busy.get() {
                bar.failed.set(false);
                bar.set_note("");
            }
        });
    }

    /// A launch is starting: its own message replaces an older refusal.
    pub fn starting(&self, message: &str) {
        self.failed.set(false);
        self.set_note(message);
    }

    /// Logo images for the theme-swap pass, like the local toolbar's.
    pub fn brand_images(&self) -> Vec<(gtk4::Image, String)> {
        self.images.borrow().clone()
    }

    /// The launch buttons, keyed by harness. Used by tests to click one.
    #[cfg(test)]
    pub fn buttons(&self) -> Vec<(String, gtk4::Button)> {
        self.buttons.borrow().clone()
    }

    fn set_note(&self, text: &str) {
        self.note_generation.set(self.note_generation.get().wrapping_add(1));
        for class in crate::command_feedback::TONE_CLASSES {
            self.note.remove_css_class(class);
        }
        self.note.set_text(text);
        self.note.set_visible(!text.is_empty());
    }

    fn paint_sensitivity(&self) {
        let ready = self.ready.get() && !self.busy.get();
        for (_, button) in self.buttons.borrow().iter() {
            button.set_sensitive(ready && button.is_visible());
        }
    }

    /// Whether this workspace currently offers `key`.
    #[cfg(test)]
    pub fn offered(&self, key: &str) -> bool {
        self.buttons.borrow()
            .iter()
            .find(|(button_key, _)| button_key == key)
            .is_some_and(|(_, button)| button.is_visible())
    }

    #[cfg(test)]
    pub fn sensitive(&self, key: &str) -> bool {
        self.buttons.borrow()
            .iter()
            .find(|(button_key, _)| button_key == key)
            .is_some_and(|(_, button)| button.is_sensitive())
    }
}

/// Everything a remote launch needs from the last snapshot: the peer, the epoch
/// the command must name, and the folder that host publishes for new cards.
#[derive(Clone)]
pub struct RemoteTarget {
    pub peer: Peer,
    pub epoch: String,
    pub workspace: String,
}

impl RemoteTarget {
    pub fn of(peer: &Peer, snapshot: &WorkspaceSnapshot) -> Self {
        Self {
            peer: peer.clone(),
            epoch: snapshot.local.epoch.clone(),
            workspace: snapshot.local.workspace.clone(),
        }
    }
}

/// Turn one harness key into the command that launches it on the host. Only a
/// harness this host offers and the folder this host published are ever named.
pub fn create_request(
    target: &RemoteTarget,
    agent_key: &str,
) -> crate::desktop_protocol::CommandRequest {
    crate::peer_client::request(
        &target.peer,
        &target.epoch,
        crate::desktop_protocol::WorkspaceCommand::CreateTerminal {
            agent_type: agent_key.to_string(),
            workspace: target.workspace.clone(),
        },
    )
}

/// One place for the viewer-visible wording of a refused launch. Codes are the
/// protocol's own, so no host text is ever shown here.
pub fn launch_error(error: &str) -> &'static str {
    match error {
        "unsupported_harness" => "That PC does not offer this harness",
        "invalid_workspace" => "That PC's folder changed · try launching again",
        "unsupported_command" | "invalid_command" | "invalid_layout" => {
            "Update SUPER DESKTOP on that PC to launch harnesses there"
        }
        "epoch_changed" | "peer_identity_changed" => "That PC restarted · try again",
        "desktop_unavailable" | "desktop_not_ready" | "remote_desktop_unavailable" => {
            "That PC's desktop is not running"
        }
        "unknown_outcome"
        | "desktop_timeout"
        | "command_outcome_unknown"
        | "peer_response_incomplete"
        | "invalid_desktop_response"
        | "invalid_peer_response" => "Launch status unknown · check that PC and refresh",
        "connection_failed_or_pin_mismatch" | "peer_endpoint_unavailable" => {
            "Cannot reach that PC · nothing launched"
        }
        "peer_revoked_or_expired" => "Pairing required · nothing launched",
        "terminal_unavailable" => "That PC could not start it · check its workspace",
        _ => "Could not launch on that PC",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_harness_the_protocol_can_carry_has_a_label_and_a_tooltip() {
        assert_eq!(harness_label("claude"), ("Claude", "⚡"));
        for key in crate::tmux::HARNESS_KEYS {
            let (name, icon) = harness_label(key);
            assert!(!name.is_empty() && !icon.is_empty(), "{key}");
            assert!(harness_tooltip(key).starts_with("Launch"), "{key}");
        }
        // A key from nowhere is still labelled rather than left blank.
        assert_eq!(harness_label("shell"), ("Shell", "💻"));
        // Only the shell may fall through to the shell's label.
        for key in crate::tmux::HARNESS_KEYS.iter().filter(|key| **key != "shell") {
            assert_ne!(harness_label(key), harness_label("shell"), "{key}");
            assert_ne!(harness_tooltip(key), harness_tooltip("shell"), "{key}");
        }
        assert_eq!(harness_label("dsh"), ("DeepSeek", "🐋"));
        assert_eq!(harness_tooltip("dsh"), "Launch DeepSeek Harness (dsh-tui)");
    }

    #[test]
    fn refusals_are_explained_in_the_viewer_s_own_words() {
        assert!(launch_error("unsupported_harness").contains("does not offer"));
        assert!(launch_error("invalid_workspace").contains("folder changed"));
        assert!(launch_error("unknown_outcome").contains("unknown"));
        // A code from nowhere still gets a sentence, never the raw code.
        assert_eq!(launch_error("shadowy_code"), "Could not launch on that PC");
        assert!(!launch_error("desktop_unavailable").contains("desktop_unavailable"));
    }

    #[test]
    fn one_state_describes_both_a_local_and_a_remote_workspace() {
        let mut snapshot = crate::remote_workspace::fixture();
        snapshot.local.visible_harnesses = vec!["shell".into(), "claude".into()];
        let state = HarnessState::of_snapshot(&snapshot, true);
        // HARNESS_KEYS order, not the order the host happened to list them in.
        assert_eq!(state.keys, vec!["claude".to_string(), "shell".to_string()]);
        assert!(state.ready);
        // A key the host does not offer is not in the state at all.
        assert!(!state.keys.iter().any(|key| key == "aider"));
        // Nor is one this build no longer knows (an older host still offering
        // the removed T3 Code launcher).
        snapshot.local.visible_harnesses = vec!["t3code".into(), "claude".into()];
        assert_eq!(HarnessState::of_snapshot(&snapshot, true).keys, vec!["claude".to_string()]);

        let none = HarnessState::none();
        assert!(none.keys.is_empty() && !none.ready);
    }

    #[test]
    fn toolbar_launchers_lose_names_least_used_first() {
        let keys: Vec<String> = ["claude", "codex", "opencode", "shell"].map(String::from).to_vec();
        // Nothing launched yet: from the right, so the bar's first stay named.
        assert_eq!(collapse_order(&keys, &BTreeMap::new()), ["shell", "opencode", "codex", "claude"]);
        let usage = BTreeMap::from([("codex".to_string(), 9), ("shell".to_string(), 2)]);
        assert_eq!(collapse_order(&keys, &usage), ["opencode", "claude", "shell", "codex"]);
    }

    #[test]
    fn toolbar_crowded_launchers_keep_only_icons() {
        crate::gtk_test::run_in_child_process("harness_bar::tests::crowded_inner");
    }

    #[test]
    fn crowded_inner() {
        if !crate::gtk_test::is_child() {
            return;
        }
        gtk4::init().unwrap();
        crate::styles::apply_styles();
        let bar = HarnessBar::new(Rc::new(|_: &str| {}), Rc::new(|_: &gtk4::Button, _: &str| false));
        let keys: Vec<String> = crate::tmux::HARNESS_KEYS.iter().map(|key| key.to_string()).collect();
        bar.apply(&HarnessState {
            keys: keys.iter().cloned().chain(["custom-a".to_string()]).collect(),
            custom: vec![CustomButton { id: "custom-a".into(), name: "My long CLI".into(), icon: "🧭".into() }],
            ready: true,
        });
        let width = || bar.group.measure(gtk4::Orientation::Horizontal, -1).1;
        let named = |key: &str| match bar.names.borrow().get(key) {
            Some(LauncherName::Label(label)) => label.is_visible(),
            Some(LauncherName::Text { full, .. }) => bar.buttons().iter()
                .any(|(id, button)| id == key && button.label().as_deref() == Some(full.as_str())),
            None => panic!("{key} has no name"),
        };
        bar.fit(10_000);
        let full = width();
        assert!(keys.iter().all(|key| named(key)) && named("custom-a"));

        bar.set_usage(&BTreeMap::from([("codex".to_string(), 5), ("shell".to_string(), 3)]));
        bar.fit(full * 2 / 3);
        assert!(width() <= full * 2 / 3, "the launchers must fit the space they are given");
        // The most used keep their names; the least used, at the right end,
        // are the first to show only their icon. Every launcher stays visible.
        assert!(named("codex") && named("shell"));
        assert!(!named("custom-a"));
        assert_eq!(bar.buttons().iter().filter(|(_, button)| button.is_visible()).count(), keys.len() + 1);
        let custom = bar.buttons().into_iter().find(|(key, _)| key == "custom-a").unwrap().1;
        assert_eq!(custom.label().as_deref(), Some("🧭"));

        // Too narrow for any name: every launcher is still there as an icon.
        bar.fit(1);
        assert!(keys.iter().all(|key| !named(key)));
        // Room again, then a renamed custom launcher: the names come back.
        bar.fit(full);
        assert!(keys.iter().all(|key| named(key)) && named("custom-a"));
        bar.apply(&HarnessState {
            keys: vec!["claude".into(), "custom-a".into()],
            custom: vec![CustomButton { id: "custom-a".into(), name: "Renamed".into(), icon: "🧭".into() }],
            ready: true,
        });
        bar.fit(full);
        assert_eq!(custom.label().as_deref(), Some("🧭 Renamed"));
        assert!(named("claude"));
    }


    #[test]
    fn toolbar_launchers_keep_a_gap_from_the_note_button() {
        crate::gtk_test::run_in_child_process("harness_bar::tests::gap_inner");
    }

    #[test]
    fn gap_inner() {
        if !crate::gtk_test::is_child() {
            return;
        }
        gtk4::init().unwrap();
        crate::styles::apply_styles();
        let bar = HarnessBar::new(Rc::new(|_: &str| {}), Rc::new(|_: &gtk4::Button, _: &str| false));
        bar.apply(&HarnessState {
            keys: vec!["claude".into(), "codex".into(), "shell".into()],
            custom: vec![],
            ready: true,
        });
        // A left side wide enough that the bar pushes the launchers against it.
        let left = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        let folder = gtk4::Entry::new();
        folder.set_width_chars(60);
        let note = gtk4::Button::with_label("📝 +");
        left.append(&folder);
        left.append(&note);
        let right = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        let content = crate::window::top_bar_content(&left, &bar.group, &right);
        let width = |w: &gtk4::Widget| w.measure(gtk4::Orientation::Horizontal, -1).1;
        // Exactly enough room: the bar's 10px spacing before the right side.
        let row_width = width(left.upcast_ref()) + width(bar.group.upcast_ref()) + 10;
        let (_, height, _, _) = content.measure(gtk4::Orientation::Vertical, row_width);
        content.allocate(row_width, height, -1, None);
        let note_end = note.compute_bounds(&content).map(|b| b.x() + b.width()).unwrap();
        let first = bar.buttons().into_iter().find(|(_, button)| button.is_visible()).unwrap().1;
        let first_start = first.compute_bounds(&content).map(|b| b.x()).unwrap();
        assert!(first_start - note_end >= LAUNCHER_GAP as f32 - 0.5,
            "the first launcher touches the note button: {note_end} → {first_start}");
    }

    #[test]
    fn a_remote_bar_offers_exactly_what_the_host_offers() {
        crate::gtk_test::run_in_child_process("harness_bar::tests::bar_inner");
    }

    #[test]
    fn bar_inner() {
        if !crate::gtk_test::is_child() {
            return;
        }
        gtk4::init().unwrap();
        let relaunches = Rc::new(RefCell::new(Vec::new()));
        let bar = HarnessBar::new(
            Rc::new({
                let relaunches = Rc::clone(&relaunches);
                move |key: &str| relaunches.borrow_mut().push(key.to_string())
            }),
            Rc::new(|_: &gtk4::Button, _: &str| false),
        );
        // Nothing is offered before a source has said what it can run.
        assert!(!bar.offered("shell") && !bar.sensitive("shell"));

        bar.apply(&HarnessState {
            keys: vec!["shell".into()],
            custom: vec![],
            ready: true,
        });
        assert!(bar.offered("shell") && bar.sensitive("shell"));
        bar.apply(&HarnessState {
            keys: vec!["shell".into(), "custom-test".into()],
            custom: vec![CustomButton { id: "custom-test".into(), name: "My CLI".into(), icon: "🧭".into() }],
            ready: true,
        });
        assert!(bar.offered("custom-test") && bar.sensitive("custom-test"));
        let custom = bar.buttons().into_iter().find(|(key, _)| key == "custom-test").unwrap().1;
        assert_eq!(custom.label().as_deref(), Some("🧭 My CLI"));
        custom.emit_clicked();
        assert_eq!(relaunches.borrow().last().map(String::as_str), Some("custom-test"));
        bar.apply(&HarnessState { keys: vec!["shell".into()], custom: vec![], ready: true });
        assert!(!bar.offered("custom-test"));
        // A harness the source does not list is not offered at all.
        assert!(!bar.offered("claude"));

        // One click, one launch, through the source's own callback.
        bar.buttons()
            .iter()
            .find(|(key, _)| key == "shell")
            .unwrap()
            .1
            .emit_clicked();
        assert_eq!(relaunches.borrow().as_slice(), ["custom-test".to_string(), "shell".to_string()]);

        // A source that cannot accept a click keeps the bar inert, and an
        // in-flight launch closes the door on a second one.
        bar.apply(&HarnessState {
            keys: vec!["shell".into()],
            custom: vec![],
            ready: false,
        });
        assert!(bar.offered("shell") && !bar.sensitive("shell"));
        bar.apply(&HarnessState {
            keys: vec!["shell".into()],
            custom: vec![],
            ready: true,
        });
        bar.set_busy(true);
        assert!(!bar.sensitive("shell") && bar.note.text() == "Launching…");
        bar.set_busy(false);
        assert!(bar.sensitive("shell"));

        // A refusal stays visible while it is shown, instead of being wiped by
        // whatever the next snapshot says, and carries its tone's theme class.
        let refusal = crate::command_feedback::for_workspace(
            crate::command_feedback::WorkspaceAction::Create,
            crate::command_feedback::Outcome::Rejected("unsupported_harness"),
        )
        .unwrap();
        bar.flash(refusal);
        bar.note("Update SUPER DESKTOP on that PC");
        assert!(bar.note.text().contains("does not offer"));
        assert!(bar.note.has_css_class("term-notice-error"));
        // The next attempt replaces it, and the tone goes with it.
        bar.starting("Launching Claude…");
        assert!(bar.note.text().contains("Launching"));
        assert!(!bar.note.has_css_class("term-notice-error"));

        // A key without a built-in launcher (the removed "t3code", left in an
        // old selection) gets no button, empty or otherwise.
        let before = bar.buttons().len();
        bar.apply(&HarnessState { keys: vec!["t3code".into(), "shell".into()], custom: vec![], ready: true });
        assert_eq!(bar.buttons().len(), before);
        assert!(!bar.offered("t3code"));
        assert_eq!(bar.buttons().iter().filter(|(_, button)| button.is_visible()).count(), 1);

        // An empty state describes a workspace that offers nothing.
        bar.apply(&HarnessState::none());
        assert!(!bar.offered("shell"));
    }
}
