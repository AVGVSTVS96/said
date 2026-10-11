use std::{
    env, fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::PathBuf,
    process::Command,
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let executable = env::current_exe()?;
    let bin = executable.parent().ok_or("missing executable directory")?;
    #[cfg(target_os = "linux")]
    let (host, archive, models) = {
        let prefix = bin.parent().ok_or("missing install prefix")?;
        let data = env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or(
            PathBuf::from(env::var_os("HOME").ok_or("HOME is required")?).join(".local/share"),
        );
        (
            prefix.join("lib/said/said-host"),
            prefix.join("lib/said/runtime.archive"),
            data.join("said/models"),
        )
    };
    #[cfg(target_os = "macos")]
    let (host, archive, models) = (
        bin.join("said-host"),
        bin.parent()
            .ok_or("missing bundle Contents")?
            .join("Resources/runtime.archive"),
        PathBuf::from(env::var_os("HOME").ok_or("HOME is required")?)
            .join("Library/Application Support/said/models"),
    );
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.is_empty() || args.first().is_some_and(|arg| arg == "--install-model") {
        fs::create_dir_all(&models)?;
        fs::set_permissions(&models, fs::Permissions::from_mode(0o700))?;
        let destination = models.join(concat!(
            "runtime-",
            env!("SAID_RUNTIME_VERSION"),
            ".archive"
        ));
        if !destination.exists() {
            let temporary = models.join(format!(".runtime-{}.part", std::process::id()));
            fs::copy(archive, &temporary)?;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
            fs::rename(temporary, destination)?;
        }
    }
    Err(Command::new(host).args(args).exec().into())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("said: {error}");
        std::process::exit(1);
    }
}
