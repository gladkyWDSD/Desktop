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
    /// The desktop's background colour as RGB; `None` keeps the theme's.
    pub background_color: Option<[u8; 3]>,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            dock: Edge::Top,
            settings: Edge::Right,
            transparent: false,
            background_opacity: 24,
            background_color: None,
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
/// Colour used under a transparent desktop when the theme's is kept: the
/// previous fixed tint.
const DEFAULT_TINT: [u8; 3] = [13, 16, 32];

/// Ready-made background colours offered in Settings, before "Custom".
pub const SWATCHES: [(&str, [u8; 3]); 6] = [
    ("Black", [0, 0, 0]),
    ("Midnight", [13, 16, 32]),
    ("Graphite", [30, 31, 36]),
    ("Navy", [15, 27, 51]),
    ("Plum", [33, 20, 46]),
    ("Forest", [15, 32, 24]),
];

/// The window background rule for `value`, or `None` to keep the theme's
/// opaque background. A chosen colour applies with or without transparency;
/// transparency uses the opacity slider.
pub fn background_css(value: &Appearance) -> Option<String> {
    if !value.transparent && value.background_color.is_none() {
        return None;
    }
    let [r, g, b] = value.background_color.unwrap_or(DEFAULT_TINT);
    let alpha = if value.transparent {
        value.background_opacity.clamp(5, 100) as f64 / 100.0
    } else {
        1.0
    };
    Some(format!(
        "window.desktop-workspace-window.cosmic-transparent {{ background-color: rgba({r}, {g}, {b}, {alpha}); background-image: none; }}"
    ))
}

pub fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

pub fn subscribe(callback: Rc<dyn Fn(Appearance)>) {
    callback(current());
    LISTENERS.with(|listeners| listeners.borrow_mut().push(callback));
}

/// A round colour sample, `rgb` read on every draw so one can follow a choice.
fn swatch_area(rgb: Rc<std::cell::Cell<[u8; 3]>>) -> gtk4::DrawingArea {
    let area = gtk4::DrawingArea::new();
    area.set_content_width(22);
    area.set_content_height(22);
    area.set_draw_func(move |_, cr, width, height| {
        let [r, g, b] = rgb.get();
        let (w, h) = (width as f64, height as f64);
        cr.arc(w / 2.0, h / 2.0, w.min(h) / 2.0 - 1.0, 0.0, std::f64::consts::TAU);
        cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
        let _ = cr.fill_preserve();
        // A light rim, so black still reads as a colour on a dark panel.
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.28);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
    });
    area
}

/// "Background colour": the theme's, a ready-made one, or any colour.
fn color_controls(body: &gtk4::Box, state: Rc<RefCell<crate::state::AppState>>) {
    let label = gtk4::Label::new(Some("Background colour"));
    label.set_xalign(0.0);
    body.append(&label);
    let row = gtk4::Box::new(Orientation::Horizontal, 6);
    row.add_css_class("bg-swatches");

    let theme = gtk4::Button::with_label("Theme");
    theme.add_css_class("bg-swatch-theme");
    theme.set_tooltip_text(Some("Use the Omarchy theme's background"));
    row.append(&theme);
    let mut presets = Vec::new();
    for (name, rgb) in SWATCHES {
        let button = gtk4::Button::new();
        button.add_css_class("bg-swatch");
        button.set_tooltip_text(Some(&format!("{name} ({})", hex(rgb))));
        button.update_property(&[gtk4::accessible::Property::Label(name)]);
        button.set_child(Some(&swatch_area(Rc::new(std::cell::Cell::new(rgb)))));
        row.append(&button);
        presets.push((rgb, button));
    }
    let custom_rgb = Rc::new(std::cell::Cell::new(
        state.borrow().appearance.background_color.unwrap_or([128, 128, 128]),
    ));
    let custom_area = swatch_area(Rc::clone(&custom_rgb));
    let custom_row = gtk4::Box::new(Orientation::Horizontal, 6);
    custom_row.append(&custom_area);
    custom_row.append(&gtk4::Label::new(Some("Custom…")));
    let custom = gtk4::MenuButton::new();
    custom.add_css_class("bg-swatch-custom");
    custom.set_child(Some(&custom_row));
    custom.set_tooltip_text(Some("Pick any colour"));
    row.append(&custom);
    body.append(&row);

    // Shows which choice is in use.
    let paint: Rc<dyn Fn(Option<[u8; 3]>)> = {
        let (theme, custom, custom_area, custom_rgb) = (theme.clone(), custom.clone(), custom_area.clone(), Rc::clone(&custom_rgb));
        let presets = presets.clone();
        Rc::new(move |chosen| {
            let mark = |widget: &gtk4::Widget, on: bool| {
                if on { widget.add_css_class("bg-swatch-selected") } else { widget.remove_css_class("bg-swatch-selected") }
            };
            mark(theme.upcast_ref(), chosen.is_none());
            let mut preset = false;
            for (rgb, button) in &presets {
                let on = chosen == Some(*rgb);
                preset |= on;
                mark(button.upcast_ref(), on);
            }
            mark(custom.upcast_ref(), chosen.is_some() && !preset);
            if let Some(rgb) = chosen.filter(|_| !preset) {
                custom_rgb.set(rgb);
                custom_area.queue_draw();
            }
        })
    };
    let choose: Rc<dyn Fn(Option<[u8; 3]>)> = {
        let (state, paint) = (Rc::clone(&state), Rc::clone(&paint));
        Rc::new(move |chosen| {
            let snapshot = {
                let mut state = state.borrow_mut();
                state.appearance.background_color = chosen;
                state.clone()
            };
            apply(snapshot.appearance);
            crate::state::save_state_async(snapshot);
            paint(chosen);
        })
    };
    paint(state.borrow().appearance.background_color);
    theme.connect_clicked({
        let choose = Rc::clone(&choose);
        move |_| choose(None)
    });
    for (rgb, button) in presets {
        let choose = Rc::clone(&choose);
        button.connect_clicked(move |_| choose(Some(rgb)));
    }

    // A popover, not a dialog: a dialog window would open under the overlay.
    let popover = gtk4::Popover::new();
    let picker_box = gtk4::Box::new(Orientation::Vertical, 8);
    #[allow(deprecated)]
    let chooser = {
        let chooser = gtk4::ColorChooserWidget::new();
        chooser.set_use_alpha(false);
        chooser.set_show_editor(true);
        let [r, g, b] = custom_rgb.get();
        chooser.set_rgba(&gtk4::gdk::RGBA::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0));
        chooser
    };
    picker_box.append(&chooser);
    let use_it = gtk4::Button::with_label("Use this colour");
    use_it.add_css_class("launcher-btn");
    use_it.add_css_class("launcher-btn-primary");
    picker_box.append(&use_it);
    popover.set_child(Some(&picker_box));
    custom.set_popover(Some(&popover));
    use_it.connect_clicked({
        let popover = popover.downgrade();
        move |_| {
            #[allow(deprecated)]
            let rgba = chooser.rgba();
            let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            choose(Some([channel(rgba.red()), channel(rgba.green()), channel(rgba.blue())]));
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
        }
    });
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
    color_controls(&body, Rc::clone(&state));
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
    fn the_background_follows_the_chosen_colour_and_opacity() {
        let theme = Appearance::default();
        assert_eq!(background_css(&theme), None, "the theme's own background");
        let tinted = Appearance { transparent: true, background_opacity: 40, ..theme };
        assert!(background_css(&tinted).unwrap().contains("rgba(13, 16, 32, 0.4)"));
        let black = Appearance { background_color: Some([0, 0, 0]), ..theme };
        assert!(background_css(&black).unwrap().contains("rgba(0, 0, 0, 1)"), "opaque without transparency");
        let see_through = Appearance { background_color: Some([33, 20, 46]), transparent: true, background_opacity: 0, ..theme };
        assert!(background_css(&see_through).unwrap().contains("rgba(33, 20, 46, 0.05)"), "never fully invisible");
        assert_eq!(hex([15, 27, 51]), "#0f1b33");
    }

    #[test]
    fn background_colour_choices_are_saved() {
        crate::gtk_test::run_in_child_process("appearance::tests::colour_choices_child");
    }

    #[test]
    fn colour_choices_child() {
        if !crate::gtk_test::is_child() {
            return;
        }
        // Choices are saved: never into the user's own state file.
        let home = std::env::temp_dir().join(format!("dw-appearance-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("HOME", &home);
        gtk4::init().unwrap();
        crate::styles::apply_styles();
        let state = Rc::new(RefCell::new(crate::state::AppState::default()));
        let parent = gtk4::Box::new(Orientation::Vertical, 0);
        settings_controls(&parent, Rc::clone(&state));
        fn find(widget: &gtk4::Widget, class: &str, found: &mut Vec<gtk4::Widget>) {
            if widget.has_css_class(class) {
                found.push(widget.clone());
            }
            let mut child = widget.first_child();
            while let Some(next) = child {
                find(&next, class, found);
                child = next.next_sibling();
            }
        }
        let all = |class: &str| {
            let mut found = Vec::new();
            find(parent.upcast_ref(), class, &mut found);
            found
        };
        let swatches = all("bg-swatch");
        assert_eq!(swatches.len(), SWATCHES.len());
        let theme = all("bg-swatch-theme").pop().unwrap().downcast::<gtk4::Button>().unwrap();
        assert!(theme.has_css_class("bg-swatch-selected"), "the theme's colour to start with");

        swatches[0].clone().downcast::<gtk4::Button>().unwrap().emit_clicked();
        assert_eq!(state.borrow().appearance.background_color, Some(SWATCHES[0].1));
        assert_eq!(current().background_color, Some(SWATCHES[0].1));
        assert!(swatches[0].has_css_class("bg-swatch-selected") && !theme.has_css_class("bg-swatch-selected"));
        assert!(background_css(&current()).unwrap().contains("rgba(0, 0, 0, 1)"));

        theme.emit_clicked();
        assert_eq!(state.borrow().appearance.background_color, None);
        assert!(theme.has_css_class("bg-swatch-selected") && !swatches[0].has_css_class("bg-swatch-selected"));
        crate::state::flush_state_saves();
        let saved = std::fs::read_to_string(home.join(".config/desktop-workspace/state.json")).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(saved["appearance"]["background_color"], serde_json::Value::Null);
        assert!(saved["appearance"].get("background_color").is_some(), "the choice is saved");
        let _ = std::fs::remove_dir_all(&home);
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
            background_color: Some([12, 34, 56]),
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
