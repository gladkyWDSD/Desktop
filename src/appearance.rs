use gtk4::{prelude::*, Align, Orientation, PolicyType};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    Right,
    #[default]
    Top,
    Bottom,
}

impl Edge {
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];
    pub fn vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub dock: Edge,
    pub settings: Edge,
    pub transparent: bool,
    pub background_opacity: u8,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            dock: Edge::Top,
            settings: Edge::Right,
            transparent: false,
            background_opacity: 24,
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Appearance> = RefCell::new(Appearance::default());
    static LISTENERS: RefCell<Vec<Rc<dyn Fn(Appearance)>>> = RefCell::new(Vec::new());
}
pub fn current() -> Appearance {
    CURRENT.with(|value| *value.borrow())
}
pub fn apply(value: Appearance) {
    let value = Appearance {
        background_opacity: value.background_opacity.clamp(5, 100),
        ..value
    };
    CURRENT.with(|current| *current.borrow_mut() = value);
    let listeners = LISTENERS.with(|listeners| listeners.borrow().clone());
    for listener in listeners {
        listener(value);
    }
}
pub fn subscribe(callback: Rc<dyn Fn(Appearance)>) {
    callback(current());
    LISTENERS.with(|listeners| listeners.borrow_mut().push(callback));
}

/// Both local and remote bars use the same orientation and scrolling rules.
/// Actions remain outside the scroll region, including on a short display.
pub fn bind_content(
    content: &gtk4::Box,
    row: &gtk4::CenterBox,
    viewport: &gtk4::ScrolledWindow,
    left: &gtk4::Widget,
    launchers: &gtk4::Widget,
    right: &gtk4::Widget,
) {
    let (content, row, viewport) = (content.downgrade(), row.downgrade(), viewport.downgrade());
    let (left, launchers, right) = (left.downgrade(), launchers.downgrade(), right.downgrade());
    let entries =
        RefCell::new(Vec::<(gtk4::glib::WeakRef<gtk4::Entry>, i32, i32, i32, bool)>::new());
    subscribe(Rc::new(move |value| {
        let (Some(content), Some(row), Some(viewport), Some(left), Some(launchers), Some(right)) = (
            content.upgrade(),
            row.upgrade(),
            viewport.upgrade(),
            left.upgrade(),
            launchers.upgrade(),
            right.upgrade(),
        ) else {
            return;
        };
        let vertical = value.dock.vertical();
        if vertical && entries.borrow().is_empty() {
            fn collect(widget: &gtk4::Widget, found: &mut Vec<gtk4::Entry>) {
                if let Some(entry) = widget.downcast_ref::<gtk4::Entry>() {
                    found.push(entry.clone());
                }
                let mut child = widget.first_child();
                while let Some(widget) = child {
                    collect(&widget, found);
                    child = widget.next_sibling();
                }
            }
            let mut found = Vec::new();
            collect(&left, &mut found);
            for entry in found {
                entries.borrow_mut().push((
                    entry.downgrade(),
                    entry.width_chars(),
                    entry.max_width_chars(),
                    entry.width_request(),
                    entry.hexpands(),
                ));
                entry.set_width_chars(3);
                entry.set_max_width_chars(14);
                entry.set_width_request(-1);
                entry.set_hexpand(true);
            }
        } else if !vertical {
            for (entry, min, max, width, expand) in entries.borrow_mut().drain(..) {
                if let Some(entry) = entry.upgrade() {
                    entry.set_width_chars(min);
                    entry.set_max_width_chars(max);
                    entry.set_width_request(width);
                    entry.set_hexpand(expand);
                }
            }
        }
        let direction = if vertical {
            Orientation::Vertical
        } else {
            Orientation::Horizontal
        };
        content.set_orientation(direction);
        row.set_orientation(direction);
        for widget in [&left, &launchers, &right] {
            if let Some(group) = widget.downcast_ref::<gtk4::Box>() {
                group.set_orientation(direction);
            }
            widget.set_halign(if vertical { Align::Fill } else { Align::Center });
            widget.set_valign(if vertical {
                Align::Start
            } else {
                Align::Center
            });
        }
        let policy = if vertical {
            (PolicyType::External, PolicyType::Automatic)
        } else {
            (PolicyType::External, PolicyType::Never)
        };
        viewport.set_policy(policy.0, policy.1);
        right.set_vexpand(false);
        right.set_valign(if vertical { Align::End } else { Align::Center });
        for child in [row.start_widget(), row.center_widget()]
            .into_iter()
            .flatten()
        {
            if let Some(scroll) = child.downcast_ref::<gtk4::ScrolledWindow>() {
                scroll.set_policy(PolicyType::External, PolicyType::Never);
                scroll.set_propagate_natural_width(!vertical);
                scroll.set_propagate_natural_height(vertical);
            }
        }
        if vertical {
            content.add_css_class("cosmic-vertical");
        } else {
            content.remove_css_class("cosmic-vertical");
        }
    }));
}

pub fn dock_rect(edge: Edge, screen: (i32, i32), thickness: i32) -> (i32, i32, i32, i32) {
    let (w, h) = (screen.0.max(1), screen.1.max(1));
    let side = 224.min((w / 2).max(1));
    let thickness = thickness.min(h);
    match edge {
        Edge::Top => (0, 0, w, thickness),
        Edge::Bottom => (0, h - thickness, w, thickness),
        Edge::Left => (0, 0, side, h),
        Edge::Right => (w - side, 0, side, h),
    }
}

pub fn settings_controls(parent: &gtk4::Box, state: Rc<RefCell<crate::state::AppState>>) {
    let (_, body) = crate::launcher_settings::section_card(parent, "", "Make it yours");
    for (title, is_dock) in [
        ("Control dock position", true),
        ("Settings panel position", false),
    ] {
        let row = gtk4::Box::new(Orientation::Horizontal, 12);
        let label = gtk4::Label::new(Some(title));
        label.set_hexpand(true);
        label.set_xalign(0.0);
        row.append(&label);
        let picker = gtk4::DropDown::from_strings(&["Left", "Right", "Top", "Bottom"]);
        let value = state.borrow().appearance;
        picker.set_selected(
            Edge::ALL
                .iter()
                .position(|edge| *edge == if is_dock { value.dock } else { value.settings })
                .unwrap() as u32,
        );
        let state = Rc::clone(&state);
        picker.connect_selected_notify(move |picker| {
            let Some(edge) = Edge::ALL.get(picker.selected() as usize) else {
                return;
            };
            let snapshot = {
                let mut state = state.borrow_mut();
                if is_dock {
                    state.appearance.dock = *edge;
                } else {
                    state.appearance.settings = *edge;
                    state.settings_panel_pos = None;
                }
                state.clone()
            };
            apply(snapshot.appearance);
            crate::state::save_state_async(snapshot);
        });
        row.append(&picker);
        body.append(&row);
    }
    let row = gtk4::Box::new(Orientation::Horizontal, 12);
    let label = gtk4::Label::new(Some("Transparent desktop"));
    label.set_hexpand(true);
    label.set_xalign(0.0);
    row.append(&label);
    let toggle = gtk4::Switch::new();
    toggle.set_active(state.borrow().appearance.transparent);
    let opacity = gtk4::Scale::with_range(Orientation::Horizontal, 5.0, 100.0, 1.0);
    opacity.set_hexpand(true);
    opacity.set_digits(0);
    opacity.set_value(state.borrow().appearance.background_opacity.clamp(5, 100) as f64);
    opacity.set_sensitive(toggle.is_active());
    opacity.set_format_value_func(|_, value| format!("{value:.0}%"));
    opacity.update_property(&[gtk4::accessible::Property::Label(
        "Desktop background opacity",
    )]);
    toggle.connect_active_notify({
        let state = Rc::clone(&state);
        let opacity = opacity.clone();
        move |toggle| {
            let snapshot = {
                let mut state = state.borrow_mut();
                state.appearance.transparent = toggle.is_active();
                state.clone()
            };
            apply(snapshot.appearance);
            crate::state::save_state_async(snapshot);
            opacity.set_sensitive(toggle.is_active());
        }
    });
    row.append(&toggle);
    body.append(&row);
    let label = gtk4::Label::new(Some("Background opacity"));
    label.set_xalign(0.0);
    body.append(&label);
    opacity.connect_value_changed(move |slider| {
        let snapshot = {
            let mut state = state.borrow_mut();
            state.appearance.background_opacity = slider.value().round() as u8;
            state.clone()
        };
        apply(snapshot.appearance);
        crate::state::save_state_async(snapshot);
    });
    body.append(&opacity);
    let hint = gtk4::Label::new(Some(
        "See your wallpaper through the desktop. Controls and terminals stay readable.",
    ));
    hint.add_css_class("launcher-hint");
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    body.append(&hint);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opacity_migrates_existing_preferences_and_clamps_invalid_values() {
        let old: Appearance =
            serde_json::from_str(r#"{"transparent":true,"dock":"bottom"}"#).unwrap();
        assert_eq!(old.background_opacity, 24);
        assert!(old.transparent);
        assert_eq!(old.dock, Edge::Bottom);
        apply(Appearance {
            background_opacity: 255,
            ..old
        });
        assert_eq!(current().background_opacity, 100);
        apply(Appearance {
            background_opacity: 0,
            ..old
        });
        assert_eq!(current().background_opacity, 5);
        apply(Appearance::default());
    }
    #[test]
    fn toolbar_edges_fit_logical_screen() {
        for screen in [
            (320, 480),
            (1024, 768),
            (2560, 1440),
            (400, 300),
            (1920, 1080),
        ] {
            for thickness in [36, 46, 56] {
                for edge in Edge::ALL {
                    let (x, y, w, h) = dock_rect(edge, screen, thickness);
                    assert!(x >= 0 && y >= 0 && x + w <= screen.0 && y + h <= screen.1);
                }
            }
        }
        let value = Appearance {
            dock: Edge::Left,
            settings: Edge::Bottom,
            transparent: true,
            background_opacity: 42,
        };
        assert_eq!(
            serde_json::from_str::<Appearance>(&serde_json::to_string(&value).unwrap()).unwrap(),
            value
        );
        assert_eq!(
            serde_json::from_str::<Appearance>("{}").unwrap(),
            Appearance::default()
        );
    }
}
