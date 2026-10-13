//! Files for a session: the "+ file" button on a local card's header, which
//! opens the system file chooser (the desktop portal's), and files dropped on
//! the card. Either way the files' paths are pasted into the session, quoted
//! like a terminal quotes a drop, so they appear in the harness's prompt where
//! the user can read them and keep typing. Claude Code and Codex attach an
//! image whose path is pasted.
use gtk4::prelude::*;
use gtk4::{gdk, gio};
use gtk4_layer_shell::{Layer, LayerShell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use vte4::prelude::*;

/// What is done with chosen or dropped files.
pub type OnFiles = Rc<dyn Fn(Vec<PathBuf>)>;

/// A big "+" with a file icon. Choosing files is wired by [`choose_on_click`].
pub fn button() -> gtk4::Button {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
    let plus = gtk4::Label::new(Some("+"));
    plus.add_css_class("term-attach-plus");
    row.append(&plus);
    let icon = gtk4::Image::from_icon_name("text-x-generic-symbolic");
    icon.add_css_class("term-attach-icon");
    row.append(&icon);
    let button = gtk4::Button::new();
    button.set_child(Some(&row));
    button.add_css_class("term-btn");
    button.add_css_class("term-attach-btn");
    button.set_tooltip_text(Some(
        "Add files: choose them in the file manager, or drop them on this card. Their paths go into the prompt.",
    ));
    button.update_property(&[gtk4::accessible::Property::Label("Add files")]);
    button
}

/// Open the system file chooser from `button`, starting in `folder`.
///
/// The overlay is a layer surface above every window, the chooser a normal
/// window: the overlay steps down to the bottom layer while the chooser is
/// open and comes back on top when it closes, whatever was chosen.
pub fn choose_on_click(button: &gtk4::Button, folder: impl Fn() -> Option<PathBuf> + 'static, on_files: OnFiles) {
    button.connect_clicked(move |button| {
        let dialog = gtk4::FileDialog::builder()
            .title("Choose files for this session")
            .accept_label("Add to prompt")
            .modal(false)
            .build();
        if let Some(dir) = folder().filter(|dir| dir.is_dir()) {
            dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
        }
        let overlay = button.root().and_downcast::<gtk4::Window>();
        let lowered = overlay.as_ref().filter(|w| gtk4_layer_shell::is_supported() && w.is_layer_window());
        if let Some(window) = lowered {
            window.set_layer(Layer::Bottom);
        }
        let restore = lowered.map(|w| w.downgrade());
        let on_files = Rc::clone(&on_files);
        // No parent: a layer surface cannot be exported as a portal parent.
        dialog.open_multiple(None::<&gtk4::Window>, None::<&gio::Cancellable>, move |result| {
            if let Some(window) = restore.as_ref().and_then(|w| w.upgrade()) {
                window.set_layer(Layer::Overlay);
            }
            let Ok(files) = result else { return };
            let paths = (0..files.n_items())
                .filter_map(|i| files.item(i).and_downcast::<gio::File>())
                .filter_map(|file| file.path())
                .collect::<Vec<_>>();
            if !paths.is_empty() {
                on_files(paths);
            }
        });
    });
}

/// Accept files dropped anywhere on `widget`. While files are dragged over it,
/// it carries `term-attach-drop` so the card can show that it takes them.
pub fn accept_drops(widget: &impl IsA<gtk4::Widget>, on_files: OnFiles) {
    let target = gtk4::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    // Before the terminal: VTE would otherwise take the drop as plain text.
    target.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let widget = widget.as_ref().clone();
    target.connect_enter({
        let widget = widget.downgrade();
        move |_, _, _| {
            if let Some(widget) = widget.upgrade() {
                widget.add_css_class("term-attach-drop");
            }
            gdk::DragAction::COPY
        }
    });
    target.connect_leave({
        let widget = widget.downgrade();
        move |_| {
            if let Some(widget) = widget.upgrade() {
                widget.remove_css_class("term-attach-drop");
            }
        }
    });
    target.connect_drop({
        let widget = widget.downgrade();
        move |_, value, _, _| {
            if let Some(widget) = widget.upgrade() {
                widget.remove_css_class("term-attach-drop");
            }
            let Ok(list) = value.get::<gdk::FileList>() else { return false };
            let paths: Vec<PathBuf> = list.files().iter().filter_map(|file| file.path()).collect();
            if paths.is_empty() {
                return false;
            }
            on_files(paths);
            true
        }
    });
    widget.add_controller(target);
}

/// One path as a shell word: unchanged when it needs no quoting, otherwise in
/// single quotes, as terminals quote a dropped file.
pub fn quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    let plain = !text.is_empty()
        && text.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+~@%,:=".contains(c));
    if plain {
        text.into_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

/// The text pasted for `paths`: each quoted, separated by spaces, and a final
/// space so the user can keep typing after them.
pub fn prompt_text(paths: &[PathBuf]) -> String {
    let mut text = paths.iter().map(|path| quote(path)).collect::<Vec<_>>().join(" ");
    text.push(' ');
    text
}

/// Paste `text` into the session: through the card's own terminal when it has
/// one (VTE applies bracketed paste, as for Ctrl+Shift+V), otherwise straight
/// into the tmux session with bracketed paste.
pub fn paste(terminal: Option<&vte4::Terminal>, session: &str, text: &str) -> Result<(), String> {
    if let Some(terminal) = terminal.filter(|t| t.is_realized()) {
        terminal.paste_text(text);
        return Ok(());
    }
    let buffer = format!("dw-attach-{}", std::process::id());
    let tmux = |args: &[&str]| -> Result<(), String> {
        let out = Command::new("tmux").args(args).output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    };
    tmux(&["set-buffer", "-b", &buffer, "--", text])?;
    tmux(&["paste-buffer", "-p", "-d", "-b", &buffer, "-t", session])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_quoted_like_a_terminal_drop() {
        assert_eq!(quote(Path::new("/home/me/notes.md")), "/home/me/notes.md");
        assert_eq!(quote(Path::new("/home/me/My Photos/cat 1.png")), "'/home/me/My Photos/cat 1.png'");
        assert_eq!(quote(Path::new("/tmp/it's.txt")), r"'/tmp/it'\''s.txt'");
        assert_eq!(quote(Path::new("/tmp/$(rm -rf ~)")), "'/tmp/$(rm -rf ~)'");
        assert_eq!(quote(Path::new("/tmp/файл.txt")), "'/tmp/файл.txt'");
        assert_eq!(
            prompt_text(&[PathBuf::from("/a/b.png"), PathBuf::from("/c d/e.txt")]),
            "/a/b.png '/c d/e.txt' "
        );
    }
}
