//! Private identity and terminal-server isolation for this checkout.
use std::path::{Path, PathBuf};

pub const PHONE_BRIDGE_ENABLED: bool = true;

pub fn require_phone_bridge() -> Result<(), String> {
    Ok(())
}

fn terminal_root(home: &Path) -> PathBuf {
    home.join(".local/state/desktop-workspace/tmux")
}

/// Called before threads start. Both tmux commands and terminal children use a
/// private server directory; never inherit the containing Super Desktop card's
/// server socket or pane id.
pub fn initialize() {
    use std::os::unix::fs::PermissionsExt;
    let home = std::env::var_os("HOME").expect("HOME is required for a standalone desktop");
    let root = terminal_root(Path::new(&home));
    std::fs::create_dir_all(&root).expect("Cannot create private terminal directory");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
        .expect("Cannot protect private terminal directory");
    std::env::set_var("TMUX_TMPDIR", root);
    std::env::remove_var("TMUX");
    std::env::remove_var("TMUX_PANE");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_and_pc_connections_are_enabled() {
        assert!(PHONE_BRIDGE_ENABLED);
        assert!(require_phone_bridge().is_ok());
    }

    #[test]
    fn runtime_identity_does_not_target_super_desktop() {
        assert_eq!(crate::socket_path_in(Path::new("/run/user/1000")), Path::new("/run/user/1000/desktop-workspace.sock"));
        assert_eq!(terminal_root(Path::new("/home/test")), Path::new("/home/test/.local/state/desktop-workspace/tmux"));
        assert!(crate::state::get_state_path().ends_with("desktop-workspace/state.json"));
    }
}
