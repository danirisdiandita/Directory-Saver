use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result, bail};

/// How a terminal is told which directory to start in.
#[derive(Clone, Copy)]
enum DirArg {
    /// Only the process working directory.
    Cwd,
    /// One argument: the flag with the directory appended, as in `--working-directory=<dir>`.
    Joined(&'static str),
    /// These arguments, then the directory as its own argument.
    Then(&'static [&'static str]),
}

/// Tried in order; the first one found on PATH wins.
const TERMINALS: &[(&str, DirArg)] = &[
    ("gnome-terminal", DirArg::Joined("--working-directory=")),
    ("ptyxis", DirArg::Joined("--working-directory=")),
    ("kgx", DirArg::Joined("--working-directory=")),
    ("konsole", DirArg::Then(&["--workdir"])),
    ("xfce4-terminal", DirArg::Joined("--working-directory=")),
    ("kitty", DirArg::Then(&["--directory"])),
    ("alacritty", DirArg::Then(&["--working-directory"])),
    ("wezterm", DirArg::Then(&["start", "--cwd"])),
    ("x-terminal-emulator", DirArg::Cwd),
    ("xterm", DirArg::Cwd),
];

fn args(dir_arg: DirArg, dir: &Path) -> Vec<OsString> {
    match dir_arg {
        DirArg::Cwd => Vec::new(),
        DirArg::Joined(flag) => {
            let mut arg = OsString::from(flag);
            arg.push(dir);
            vec![arg]
        }
        DirArg::Then(flags) => flags
            .iter()
            .map(OsString::from)
            .chain([dir.as_os_str().to_owned()])
            .collect(),
    }
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn find_on_path(program: &OsStr, path: &OsStr) -> Option<PathBuf> {
    if Path::new(program).components().count() > 1 {
        return is_executable(Path::new(program)).then(|| program.into());
    }
    std::env::split_paths(path)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

/// `$TERMINAL` wins; a known name keeps its directory flag, anything else gets only the cwd.
fn resolve(env_terminal: Option<OsString>, path: &OsStr) -> Option<(PathBuf, DirArg)> {
    if let Some(name) = env_terminal.filter(|name| !name.is_empty()) {
        let base = Path::new(&name).file_name().unwrap_or(&name).to_owned();
        let dir_arg = TERMINALS
            .iter()
            .find(|(program, _)| OsStr::new(program) == base)
            .map_or(DirArg::Cwd, |&(_, dir_arg)| dir_arg);
        if let Some(exe) = find_on_path(&name, path) {
            return Some((exe, dir_arg));
        }
    }
    TERMINALS.iter().find_map(|&(program, dir_arg)| {
        find_on_path(OsStr::new(program), path).map(|exe| (exe, dir_arg))
    })
}

pub fn open(dir: &Path) -> Result<()> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some((exe, dir_arg)) = resolve(std::env::var_os("TERMINAL"), &path) else {
        bail!("No terminal emulator found");
    };
    let mut child = Command::new(&exe)
        .args(args(dir_arg, dir))
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("Could not start {}", exe.display()))?;
    // Some launchers (x-terminal-emulator on Debian) block until the window closes, so
    // wait off the UI thread to avoid leaving a zombie behind.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(program: &str, dir: &str) -> Vec<String> {
        let &(_, dir_arg) = TERMINALS.iter().find(|(name, _)| *name == program).unwrap();
        args(dir_arg, Path::new(dir))
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn passes_the_directory_the_way_each_terminal_expects() {
        assert_eq!(argv("gnome-terminal", "/a b"), ["--working-directory=/a b"]);
        assert_eq!(argv("kitty", "/a b"), ["--directory", "/a b"]);
        assert_eq!(argv("wezterm", "/a b"), ["start", "--cwd", "/a b"]);
        assert_eq!(argv("konsole", "/a b"), ["--workdir", "/a b"]);
        assert!(argv("xterm", "/a b").is_empty());
    }

    fn fake_bin(names: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            let file = dir.path().join(name);
            std::fs::write(&file, "").unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        dir
    }

    #[test]
    fn picks_the_first_installed_terminal_in_table_order() {
        let bin = fake_bin(&["xterm", "x-terminal-emulator", "kitty"]);
        let (exe, dir_arg) = resolve(None, bin.path().as_os_str()).unwrap();
        assert_eq!(exe, bin.path().join("kitty"));
        assert_eq!(args(dir_arg, Path::new("/d")), ["--directory", "/d"]);
    }

    #[test]
    fn prefers_terminal_env_and_keeps_its_directory_flag() {
        let bin = fake_bin(&["gnome-terminal", "wezterm"]);
        let (exe, dir_arg) = resolve(Some("wezterm".into()), bin.path().as_os_str()).unwrap();
        assert_eq!(exe, bin.path().join("wezterm"));
        assert_eq!(args(dir_arg, Path::new("/d")), ["start", "--cwd", "/d"]);
    }

    #[test]
    fn unknown_terminal_env_relies_on_the_working_directory() {
        let bin = fake_bin(&["foot", "gnome-terminal"]);
        let (exe, dir_arg) = resolve(Some("foot".into()), bin.path().as_os_str()).unwrap();
        assert_eq!(exe, bin.path().join("foot"));
        assert!(args(dir_arg, Path::new("/d")).is_empty());
    }

    #[test]
    fn reports_none_when_nothing_is_installed() {
        let bin = fake_bin(&[]);
        assert!(resolve(Some("missing".into()), bin.path().as_os_str()).is_none());
    }
}
