#[cfg(not(target_os = "macos"))]
compile_error!("mdvr requires macOS");

#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
pub mod contracts;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod files;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod navigation;
#[cfg(target_os = "macos")]
mod platform;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[allow(clippy::field_reassign_with_default)]
mod preferences;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod theme;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod ui;

#[cfg(target_os = "macos")]
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

#[cfg(target_os = "macos")]
use files::{PathError, PathKind, resolve_path};
#[cfg(target_os = "macos")]
use preferences::{LaunchIntent, LaunchState, Preferences, resolve_launch};

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LaunchPlan {
    pub intent: LaunchIntent,
    pub state: LaunchState,
    pub picker_root: PathBuf,
    pub explicit: bool,
}

#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq)]
enum CliError {
    MultipleInputs(usize),
    UnsupportedInput(String),
    Path(PathError),
    InvalidLaunch(String),
}

#[cfg(target_os = "macos")]
impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MultipleInputs(count) => {
                write!(f, "expected zero or one input, received {count}")
            }
            Self::UnsupportedInput(input) => write!(f, "unsupported input: {input}"),
            Self::Path(error) => error.fmt(f),
            Self::InvalidLaunch(error) => f.write_str(error),
        }
    }
}

#[cfg(target_os = "macos")]
fn parse_args<I>(args: I, caller_cwd: &Path) -> Result<CliCommand, CliError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let _program = args.next();
    let inputs = args.collect::<Vec<_>>();
    if inputs.len() > 1 {
        return Err(CliError::MultipleInputs(inputs.len()));
    }

    let Some(input) = inputs.first() else {
        return launch_command(caller_cwd, None);
    };
    if input == "--help" {
        return Ok(CliCommand::Help);
    }
    if input == "--version" {
        return Ok(CliCommand::Version);
    }

    let text = input.to_string_lossy();
    if text.starts_with('-') || text.starts_with("http://") || text.starts_with("https://") {
        return Err(CliError::UnsupportedInput(text.into_owned()));
    }
    launch_command(caller_cwd, Some(Path::new(input)))
}

#[cfg(target_os = "macos")]
fn launch_command(caller_cwd: &Path, requested: Option<&Path>) -> Result<CliCommand, CliError> {
    let resolved = resolve_path(caller_cwd, requested).map_err(CliError::Path)?;
    let intent = match resolved.kind {
        PathKind::File => LaunchIntent::ExplicitFile,
        PathKind::Directory if resolved.explicit => LaunchIntent::ExplicitDirectory,
        PathKind::Directory => LaunchIntent::Bare,
    };
    let state = resolve_launch(
        intent,
        (resolved.explicit).then_some(resolved.path.as_path()),
        &Preferences::default(),
        false,
    )
    .map_err(|error| CliError::InvalidLaunch(error.to_string()))?;
    let picker_root = state
        .browsing_root
        .clone()
        .unwrap_or_else(|| resolved.path.clone());
    Ok(CliCommand::Launch(LaunchPlan {
        intent,
        state,
        picker_root,
        explicit: resolved.explicit,
    }))
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq)]
enum CliCommand {
    Help,
    Version,
    Launch(LaunchPlan),
}

#[cfg(target_os = "macos")]
const HELP: &str = "Usage: mdvr [PATH]\n\nOpen one local file or browse one local directory.\nWith no path, browse caller's current directory.\n\nOptions:\n  --help       Show this help\n  --version    Show version\n";

#[cfg(target_os = "macos")]
fn main() {
    let caller_cwd = match env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("mdvr: cannot determine caller cwd: {error}");
            std::process::exit(2);
        }
    };
    match parse_args(env::args_os(), &caller_cwd) {
        Ok(CliCommand::Help) => print!("{HELP}"),
        Ok(CliCommand::Version) => println!("mdvr {}", env!("CARGO_PKG_VERSION")),
        Ok(CliCommand::Launch(plan)) => app::run(plan),
        Err(error) => {
            eprintln!("mdvr: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let number = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("mdvr-cli-{}-{number}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn arg(value: &str) -> OsString {
        OsString::from(value)
    }

    #[test]
    fn parses_help_version_and_rejects_extra_inputs() {
        let cwd = temp_dir();
        assert_eq!(
            parse_args([arg("mdvr"), arg("--help")], &cwd),
            Ok(CliCommand::Help)
        );
        assert_eq!(
            parse_args([arg("mdvr"), arg("--version")], &cwd),
            Ok(CliCommand::Version)
        );
        assert!(matches!(
            parse_args([arg("mdvr"), arg("one"), arg("two")], &cwd),
            Err(CliError::MultipleInputs(2))
        ));
    }

    #[test]
    fn resolves_relative_file_and_directory_without_restore() {
        let cwd = temp_dir();
        fs::write(cwd.join("README.md"), "# current").unwrap();
        let file = parse_args([arg("mdvr"), arg("README.md")], &cwd).unwrap();
        let CliCommand::Launch(file) = file else {
            panic!("expected launch")
        };
        assert_eq!(file.intent, LaunchIntent::ExplicitFile);
        assert_eq!(file.state.document, Some(cwd.join("README.md")));
        assert!(file.state.reading_locator.is_none());

        let directory = parse_args([arg("mdvr"), arg(".")], &cwd).unwrap();
        let CliCommand::Launch(directory) = directory else {
            panic!("expected launch")
        };
        assert_eq!(directory.intent, LaunchIntent::ExplicitDirectory);
        assert_eq!(directory.picker_root, cwd.join("."));
    }

    #[test]
    fn bare_launch_uses_caller_directory_and_path_errors_remain_distinct() {
        let cwd = temp_dir();
        let bare = parse_args([arg("mdvr")], &cwd).unwrap();
        let CliCommand::Launch(bare) = bare else {
            panic!("expected launch")
        };
        assert_eq!(bare.intent, LaunchIntent::Bare);
        assert_eq!(bare.picker_root, cwd);
        assert!(!bare.explicit);

        assert!(matches!(
            parse_args([arg("mdvr"), arg("missing.md")], &cwd),
            Err(CliError::Path(PathError::Missing(_)))
        ));
        assert!(matches!(
            parse_args([arg("mdvr"), arg("--bad")], &cwd),
            Err(CliError::UnsupportedInput(_))
        ));
    }
}
