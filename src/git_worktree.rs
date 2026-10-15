//! Git helpers for harness cards: a fresh worktree per launch, and the branch
//! badge on each card's header.
//!
//! Two agents in one checkout write over each other's files. With **⑂** on in
//! the top bar, a harness started in a git repository gets its own worktree on
//! a new branch next to the repository (`<repo>.worktrees/<agent>-<stamp>`),
//! so parallel agents each change their own copy. Worktrees are never removed
//! here: they hold the agent's work, and `git worktree remove` is the user's
//! call.
//!
//! The badge reads `git status` with `GIT_OPTIONAL_LOCKS=0`, so a refresh never
//! takes the index lock an agent's own `git` command may need.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long one folder's badge is reused. Cards refresh every second; `git
/// status` on a large checkout is too slow for that.
const BADGE_TTL: Duration = Duration::from_secs(5);

fn git(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    command
}

fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let out = git(dir).args(args).stderr(Stdio::null()).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
}

/// The top folder of the work tree `dir` is in, or `None` outside a repository.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    git_output(dir, &["rev-parse", "--show-toplevel"])
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
}

/// `claude`, `custom-My Tool!` → `claude`, `custom-my-tool`: a branch and
/// folder name made only of `[a-z0-9-]`.
fn slug(agent: &str) -> String {
    let mut out = String::new();
    for c in agent.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() { "harness".to_string() } else { out.to_string() }
}

/// Where a new worktree for `agent` goes, and its branch: beside the
/// repository, never inside it, so the original checkout's `git status` and
/// file watchers do not see a second copy of the project.
fn worktree_location(root: &Path, agent: &str, stamp: &str) -> (PathBuf, String) {
    let name = format!("{}-{stamp}", slug(agent));
    let repo = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    let base = root.parent().unwrap_or(root).join(format!("{repo}.worktrees"));
    (base.join(&name), name)
}

fn branch_exists(root: &Path, branch: &str) -> bool {
    git(root)
        .args(["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Create a worktree on a new branch from the current `HEAD` of the repository
/// `dir` is in, and return the folder the harness should start in: the same
/// subfolder of the new worktree when `dir` was below the repository's top.
///
/// `Err` says why not (not a repository, no commit yet, git refused); the
/// caller then starts the harness in `dir` as usual.
pub fn create_for_card(dir: &str, agent: &str) -> Result<String, String> {
    let dir = Path::new(dir);
    let root = repo_root(dir).ok_or_else(|| "not a git repository".to_string())?;
    let stamp = chrono::Local::now().format("%m%d-%H%M%S").to_string();
    let (mut path, mut branch) = worktree_location(&root, agent, &stamp);
    // Two launches in the same second: number the second one.
    let mut n = 2;
    while path.exists() || branch_exists(&root, &branch) {
        let (p, b) = worktree_location(&root, agent, &format!("{stamp}-{n}"));
        (path, branch) = (p, b);
        n += 1;
        if n > 50 {
            return Err("no free worktree name".into());
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let out = git(&root)
        .args(["worktree", "add", "--quiet", "-b", &branch])
        .arg(&path)
        .arg("HEAD")
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("git worktree add failed");
        return Err(line.trim().trim_start_matches("fatal: ").to_string());
    }
    // Keep the subfolder: a harness started in `repo/app` starts in
    // `worktree/app`, where its config and history expect to be.
    let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let canonical_root = root.canonicalize().unwrap_or(root);
    let start = canonical_dir
        .strip_prefix(&canonical_root)
        .ok()
        .map(|rel| path.join(rel))
        .filter(|p| p.is_dir())
        .unwrap_or(path);
    Ok(start.to_string_lossy().into_owned())
}

/// What a card's header says about the git checkout it runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    /// Branch name, or the short commit on a detached `HEAD`.
    pub branch: String,
    /// Changed, staged and untracked paths.
    pub changed: usize,
}

impl Badge {
    pub fn label(&self) -> String {
        if self.changed == 0 {
            format!("⑂ {}", self.branch)
        } else {
            format!("⑂ {} · {}", self.branch, self.changed)
        }
    }

    pub fn tooltip(&self, dir: &str) -> String {
        let changes = match self.changed {
            0 => "no changes".to_string(),
            1 => "1 changed file".to_string(),
            n => format!("{n} changed files"),
        };
        format!("Branch {} · {changes}\n{}", self.branch, crate::state::display_dir(dir))
    }
}

/// Parse `git status --porcelain=v2 --branch`.
fn parse_status(text: &str) -> Option<Badge> {
    let mut head = None;
    let mut oid = None;
    let mut changed = 0;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            head = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.oid ") {
            oid = Some(rest.trim().to_string());
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            changed += 1;
        }
    }
    let branch = match head?.as_str() {
        "(detached)" => {
            let oid = oid.filter(|o| o != "(initial)")?;
            oid.chars().take(7).collect()
        }
        name => name.to_string(),
    };
    Some(Badge { branch, changed })
}

fn badge_uncached(dir: &str) -> Option<Badge> {
    let text = git_output(Path::new(dir), &["status", "--porcelain=v2", "--branch", "--untracked-files=normal"])?;
    parse_status(&text)
}

type BadgeCache = Mutex<HashMap<String, (Instant, Option<Badge>)>>;

/// The badge for the checkout `dir` is in, `None` outside a repository. Reused
/// for [`BADGE_TTL`]. Worker threads only: it runs `git`.
pub fn badge(dir: &str) -> Option<Badge> {
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return None;
    }
    static CACHE: OnceLock<BadgeCache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((at, value)) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(dir) {
        if at.elapsed() < BADGE_TTL {
            return value.clone();
        }
    }
    let value = badge_uncached(dir);
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(dir.to_string(), (Instant::now(), value.clone()));
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dw-git-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo(tag: &str) -> (PathBuf, PathBuf) {
        let parent = scratch(tag);
        let root = parent.join("proj");
        std::fs::create_dir_all(root.join("app")).unwrap();
        run(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("app/a.txt"), "a").unwrap();
        run(&root, &["add", "."]);
        run(&root, &["commit", "-q", "-m", "init"]);
        (parent, root)
    }

    #[test]
    fn slug_keeps_branch_names_safe() {
        assert_eq!(slug("claude"), "claude");
        assert_eq!(slug("custom-My Tool!"), "custom-my-tool");
        assert_eq!(slug("!!"), "harness");
    }

    #[test]
    fn worktrees_go_beside_the_repository() {
        let (path, branch) = worktree_location(Path::new("/w/proj"), "codex", "1015-120000");
        assert_eq!(path, PathBuf::from("/w/proj.worktrees/codex-1015-120000"));
        assert_eq!(branch, "codex-1015-120000");
    }

    #[test]
    fn status_parses_branch_detached_and_changes() {
        let text = "# branch.oid 0123456789abcdef\n# branch.head main\n# branch.upstream origin/main\n1 .M N... 100644 100644 100644 a b f.rs\n? new.txt\n";
        assert_eq!(parse_status(text), Some(Badge { branch: "main".into(), changed: 2 }));
        let detached = "# branch.oid 0123456789abcdef\n# branch.head (detached)\n";
        assert_eq!(parse_status(detached), Some(Badge { branch: "0123456".into(), changed: 0 }));
        assert_eq!(parse_status("garbage"), None);
        assert_eq!(Badge { branch: "main".into(), changed: 0 }.label(), "⑂ main");
        assert_eq!(Badge { branch: "main".into(), changed: 3 }.label(), "⑂ main · 3");
    }

    #[test]
    fn worktree_for_a_card_keeps_the_subfolder_and_a_new_branch() {
        let (parent, root) = repo("wt");
        let start = create_for_card(root.join("app").to_str().unwrap(), "claude").unwrap();
        let start = PathBuf::from(start);
        assert!(start.ends_with("app"), "{start:?}");
        assert!(start.join("a.txt").is_file());
        assert!(start.starts_with(parent.join("proj.worktrees")));
        let badge = badge_uncached(start.to_str().unwrap()).unwrap();
        assert!(badge.branch.starts_with("claude-"), "{badge:?}");
        assert_eq!(badge.changed, 0);
        // A second launch in the same second gets its own worktree.
        let second = create_for_card(root.to_str().unwrap(), "claude").unwrap();
        assert_ne!(PathBuf::from(&second), start.parent().unwrap());
        // The original checkout is untouched and still on main.
        std::fs::write(root.join("untracked.txt"), "x").unwrap();
        assert_eq!(badge_uncached(root.to_str().unwrap()), Some(Badge { branch: "main".into(), changed: 1 }));
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn worktree_outside_a_repository_is_refused() {
        let dir = scratch("plain");
        assert!(create_for_card(dir.to_str().unwrap(), "claude").is_err());
        assert_eq!(badge(dir.to_str().unwrap()), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
