//! Fresh local discovery feeding the same terminal launcher as the dock.
use gtk4::{glib, prelude::*, Orientation};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
struct Choice {
    key: String,
    name: String,
    icon: String,
    command: String,
}

fn choices(state: &crate::state::AppState) -> Vec<Choice> {
    crate::tmux::detect_harnesses()
        .into_iter()
        // A package that could be downloaded by npx is not an installed agent.
        .filter(|agent| {
            agent.key != "herder"
                && (agent.key == "shell" || crate::tmux::harness_binary(agent.key).is_some())
        })
        .map(|agent| Choice {
            key: agent.key.into(),
            name: agent.name.into(),
            icon: agent.icon.into(),
            command: agent.command,
        })
        .chain(
            state
                .custom_harnesses
                .iter()
                .filter(|agent| agent.validate().is_ok())
                .map(|agent| Choice {
                    key: agent.id.clone(),
                    name: agent.name.clone(),
                    icon: agent.icon.clone(),
                    command: agent.command(),
                }),
        )
        .collect()
}

pub fn button(
    state: Rc<RefCell<crate::state::AppState>>,
    launch: Rc<dyn Fn(&str)>,
) -> gtk4::MenuButton {
    let button = gtk4::MenuButton::new();
    let label = gtk4::Label::new(Some("＋ New Conversation"));
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_xalign(0.0);
    button.set_child(Some(&label));
    button.add_css_class("machine-selector");
    button.add_css_class("cosmic-new-conversation");
    button.set_tooltip_text(Some(
        "Find installed agents and open a real terminal in the selected project folder",
    ));
    let popover = gtk4::Popover::new();
    popover.add_css_class("cosmic-conversations");
    popover.set_has_arrow(false);
    // Layer-shell keyboard mode changes invalidate GTK's modal popup grab.
    popover.set_autohide(false);
    button.set_popover(Some(&popover));
    popover.connect_show(move |popover| {
        let edge = match crate::appearance::current().dock {
            crate::appearance::Edge::Top => gtk4::PositionType::Bottom,
            crate::appearance::Edge::Bottom => gtk4::PositionType::Top,
            crate::appearance::Edge::Left => gtk4::PositionType::Right,
            crate::appearance::Edge::Right => gtk4::PositionType::Left,
        };
        popover.set_position(edge);
        populate(popover, Rc::clone(&state), Rc::clone(&launch));
    });
    let keys = gtk4::EventControllerKey::new();
    keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let weak = popover.downgrade();
        move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                if let Some(popover) = weak.upgrade() {
                    popover.popdown();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    });
    popover.add_controller(keys);
    button
}

fn populate(
    popover: &gtk4::Popover,
    state: Rc<RefCell<crate::state::AppState>>,
    launch: Rc<dyn Fn(&str)>,
) {
    let body = gtk4::Box::new(Orientation::Vertical, 10);
    let heading = gtk4::Box::new(Orientation::Horizontal, 10);
    let title = gtk4::Label::new(Some("New Conversation"));
    title.add_css_class("cosmic-picker-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    heading.append(&title);
    let close = gtk4::Button::from_icon_name("window-close-symbolic");
    close.set_tooltip_text(Some("Close agent chooser"));
    close.connect_clicked({
        let weak = popover.downgrade();
        move |_| {
            if let Some(popover) = weak.upgrade() {
                popover.popdown();
            }
        }
    });
    heading.append(&close);
    body.append(&heading);
    let hint = gtk4::Label::new(Some(
        "Choose an agent. Its interactive terminal opens here.",
    ));
    hint.set_wrap(true);
    hint.set_max_width_chars(38);
    hint.set_xalign(0.0);
    hint.add_css_class("cosmic-picker-hint");
    body.append(&hint);
    let folder = gtk4::Label::new(Some(&crate::state::effective_workspace_dir(
        &state.borrow(),
    )));
    folder.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    folder.set_max_width_chars(36);
    folder.set_xalign(0.0);
    folder.add_css_class("cosmic-picker-folder");
    body.append(&folder);
    let search = gtk4::SearchEntry::new();
    search.set_placeholder_text(Some("Search installed agents…"));
    search.update_property(&[gtk4::accessible::Property::Label("Search installed agents")]);
    body.append(&search);
    let list = gtk4::Box::new(Orientation::Vertical, 6);
    let mut rows = Vec::new();
    let available = choices(&state.borrow());
    for choice in &available {
        let row = gtk4::Button::with_label(&format!("{}  {}", choice.icon, choice.name));
        row.add_css_class("cosmic-agent-choice");
        row.set_tooltip_text(Some(&choice.command));
        if let Some(label) = row.child().and_downcast::<gtk4::Label>() {
            label.set_xalign(0.0);
        }
        let key = choice.key.clone();
        row.connect_clicked({
            let weak = popover.downgrade();
            let launch = Rc::clone(&launch);
            let state = Rc::clone(&state);
            move |button| {
                // Recheck when clicked: an agent can be removed while the chooser is open.
                if !choices(&state.borrow()).iter().any(|item| item.key == key) {
                    button.set_sensitive(false);
                    button.set_tooltip_text(Some(
                        "This agent is no longer installed. Rescan to refresh.",
                    ));
                    return;
                }
                if let Some(popover) = weak.upgrade() {
                    popover.popdown();
                }
                launch(&key);
            }
        });
        rows.push((
            format!("{} {}", choice.name, choice.key).to_lowercase(),
            row.clone(),
        ));
        list.append(&row);
    }
    let empty = gtk4::Label::new(Some(
        "No matching agents. Install a CLI or add a custom launcher in Settings.",
    ));
    empty.set_wrap(true);
    empty.set_max_width_chars(36);
    empty.add_css_class("cosmic-picker-hint");
    empty.set_visible(available.is_empty());
    list.append(&empty);
    search.connect_search_changed(move |search| {
        let query = search.text().trim().to_lowercase();
        let mut matched = false;
        for (name, row) in &rows {
            let visible = name.contains(&query);
            row.set_visible(visible);
            matched |= visible;
        }
        empty.set_visible(!matched);
    });
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_min_content_height(100);
    scroll.set_max_content_height(300);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&list));
    body.append(&scroll);
    let rescan = gtk4::Button::with_label("↻ Rescan installed agents");
    rescan.connect_clicked({
        let weak = popover.downgrade();
        move |_| {
            if let Some(popover) = weak.upgrade() {
                populate(&popover, Rc::clone(&state), Rc::clone(&launch));
            }
        }
    });
    body.append(&rescan);
    popover.set_child(Some(&body));
    search.grab_focus();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_includes_unpinned_custom_agents_and_excludes_missing_executables() {
        let mut state = crate::state::AppState::default();
        state.visible_harnesses = Some(vec![]);
        let custom =
            crate::custom_harness::CustomHarness::create("Test agent", "🤖", "/bin/echo", "")
                .unwrap();
        let id = custom.id.clone();
        state.custom_harnesses.push(custom);
        assert!(choices(&state).iter().any(|item| item.key == id));
        state.custom_harnesses[0].executable = "/cosmic-missing-agent".into();
        assert!(!choices(&state).iter().any(|item| item.key == id));
    }

    #[test]
    fn picker_launches_selected_terminal_and_rescans() {
        crate::gtk_test::run_in_child_process("conversation_picker::tests::picker_child");
    }

    fn widgets(root: &gtk4::Widget) -> Vec<gtk4::Widget> {
        let mut result = vec![root.clone()];
        let mut child = root.first_child();
        while let Some(widget) = child {
            result.extend(widgets(&widget));
            child = widget.next_sibling();
        }
        result
    }

    #[test]
    fn picker_child() {
        if !crate::gtk_test::is_child() {
            return;
        }
        gtk4::init().unwrap();
        gtk4::Settings::default()
            .unwrap()
            .set_gtk_enable_animations(false);
        crate::styles::apply_styles();
        let state = Rc::new(RefCell::new(crate::state::AppState::default()));
        let agent = crate::custom_harness::CustomHarness::create(
            "Test chosen agent",
            "🤖",
            "/bin/echo",
            "",
        )
        .unwrap();
        let id = agent.id.clone();
        state.borrow_mut().custom_harnesses.push(agent);
        let launched = Rc::new(RefCell::new(Vec::new()));
        let button = button(
            Rc::clone(&state),
            Rc::new({
                let launched = Rc::clone(&launched);
                move |key| {
                    launched.borrow_mut().push(key.to_string());
                }
            }),
        );
        let window = gtk4::Window::new();
        window.set_decorated(false);
        window.set_default_size(620, 650);
        window.add_css_class("desktop-workspace-window");
        let root = gtk4::Box::new(Orientation::Vertical, 0);
        button.set_halign(gtk4::Align::Center);
        root.append(&button);
        window.set_child(Some(&root));
        window.present();
        let settle = || {
            let until = std::time::Instant::now() + std::time::Duration::from_millis(600);
            while std::time::Instant::now() < until {
                while glib::MainContext::default().iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        settle();
        button.popup();
        settle();
        let popover = button.popover().unwrap();
        assert!(popover.is_mapped());
        let content = popover.child().unwrap();
        let snapshot = gtk4::Snapshot::new();
        content
            .parent()
            .unwrap()
            .snapshot_child(&content, &snapshot);
        let node = snapshot.to_node().expect("agent chooser should render");
        window
            .renderer()
            .unwrap()
            .render_texture(&node, None)
            .save_to_png("/tmp/cosmic-conversations-preview.png")
            .unwrap();
        let rows = widgets(popover.upcast_ref());
        let search = rows
            .iter()
            .find_map(|w| w.clone().downcast::<gtk4::SearchEntry>().ok())
            .unwrap();
        search.set_text("Test chosen");
        settle();
        let choice = rows
            .iter()
            .find_map(|w| {
                w.clone()
                    .downcast::<gtk4::Button>()
                    .ok()
                    .filter(|b| b.label().is_some_and(|s| s.contains("Test chosen agent")))
            })
            .unwrap();
        assert!(choice.is_visible());
        assert_eq!(
            rows.iter()
                .filter(|w| w.has_css_class("cosmic-agent-choice") && w.is_visible())
                .count(),
            1
        );
        choice.emit_clicked();
        assert_eq!(*launched.borrow(), vec![id]);
        state.borrow_mut().custom_harnesses.clear();
        button.popup();
        settle();
        assert!(!widgets(popover.upcast_ref()).iter().any(|w| w
            .clone()
            .downcast::<gtk4::Button>()
            .ok()
            .is_some_and(|b| b.label().is_some_and(|s| s.contains("Test chosen agent")))));
        window.close();
    }
}
