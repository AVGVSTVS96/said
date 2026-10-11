use said_app::{
    ipc,
    runtime::{Host, worker_config},
    storage::Paths,
};
use said_core::{Mode, Result, Target};
use said_engine::{
    host::WorkerClient,
    manifest::{Manifest, worker_main},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

fn option(args: &[String], key: &str) -> Option<PathBuf> {
    args.iter()
        .position(|arg| arg == key)
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from)
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--worker")
        && args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--model-cache" | "--model-manifest" | "--trace"
            )
        })
    {
        return said_bench::cli(&args);
    }
    let paths = Paths::discover()?;
    if args.first().map(String::as_str) == Some("bench") {
        let mut args = args;
        if !args.iter().any(|arg| arg == "--model-cache") {
            args.extend([
                "--model-cache".into(),
                paths.models.to_string_lossy().into(),
            ]);
        }
        return said_bench::cli(&args);
    }

    if args.first().map(String::as_str) == Some("--worker") {
        let models = option(&args, "--models").unwrap_or(paths.models);
        return worker_main(
            args.get(1).map(String::as_str),
            &Manifest::bundled()?,
            &models,
        );
    }
    #[cfg(target_os = "linux")]
    if args.first().map(String::as_str) == Some("--nested-proof") {
        return said_app::proof::nested(
            args.get(1).ok_or("supply a synthetic WAV")?,
            &option(&args, "--models").ok_or("supply --models")?,
        );
    }
    if args.first().map(String::as_str) == Some("--install-model") {
        let models = option(&args, "--models").ok_or("supply --models destination")?;
        let manifest = Manifest::bundled()?;
        return said_app::download::install(
            &manifest,
            manifest.default_model()?,
            &models,
            |progress| {
                eprintln!("{}: {} bytes", progress.file, progress.received);
            },
        );
    }
    if args.first().map(String::as_str) == Some("--replay") {
        let wav = args.get(1).ok_or("--replay needs a synthetic WAV")?;
        let data = option(&args, "--data").ok_or("--replay needs an isolated --data directory")?;
        let mut paths = Paths::new(data.clone(), data.join("config"));
        paths.models = option(&args, "--models").ok_or("--replay needs --models")?;
        let recorder = said_app::proof::replay_recorder(std::path::Path::new(wav), &paths.data)?;
        let platform = said_platform::fake::FakePlatform::new(Some(Target {
            app_id: "scratch".into(),
            window_id: "synthetic".into(),
            name: "Scratch target".into(),
        }));
        let platform = said_app::proof::SavedPlatform {
            fake: platform,
            database: paths.data.join("history.sqlite"),
        };
        let mut host = Host::open(platform, recorder, paths)?;
        let client = WorkerClient::warm(worker_config(&host.paths.models, &host.settings.engine)?)?;
        host.set_client(client);
        host.start(Mode::Quick, false)?;
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            host.poll()?;
            if let Some(row) = host.history.last()? {
                if row.pasted_at.is_some() {
                    let pasted: Vec<_> = host
                        .platform
                        .fake
                        .calls
                        .borrow()
                        .iter()
                        .filter_map(|call| match call {
                            said_platform::fake::PlatformCall::Paste { text, send, .. } => {
                                Some(serde_json::json!({"text":text,"send":send}))
                            }
                            _ => None,
                        })
                        .collect();
                    println!("{}", serde_json::json!({"utterance":row,"paste":pasted}));
                    return Ok(());
                }
                if row.status == Some(said_core::UtteranceStatus::Failed)
                    && !host.session.views().any(|view| {
                        matches!(
                            view.phase,
                            said_core::Phase::Recording
                                | said_core::Phase::Finishing
                                | said_core::Phase::Saving
                        )
                    })
                {
                    return Err(
                        format!("host replay failed: {:?}", host.state.notifications).into(),
                    );
                }
                if row.status == Some(said_core::UtteranceStatus::Empty) {
                    println!("{}", serde_json::json!({"utterance":row,"paste":[]}));
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err("synthetic host replay did not finish within diagnostic limit".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    if !args.is_empty() {
        if args[0] == "import-vocabulary" {
            let source = args.get(1).ok_or("specify wispr or codex")?;
            let path = match args.get(2) {
                Some(path) => PathBuf::from(path),
                None => said_app::cli::vocabulary_source(source)?,
            };
            return said_app::cli::import_vocabulary(&paths, source, &path);
        }
        #[cfg(target_os = "linux")]
        {
            let runtime =
                std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is required")?;
            let command = if args.first().map(String::as_str) == Some("history")
                && args.get(1).map(String::as_str) == Some("search")
            {
                let mut command = vec!["history".into()];
                command.extend(args.iter().skip(2).cloned());
                command
            } else {
                args
            };
            println!(
                "{}",
                said_platform::linux::control::run_cli(
                    &PathBuf::from(runtime),
                    env!("PRODUCT_STORAGE_KEY"),
                    &command
                )?
            );
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        return Err(
            "desktop control CLI is available on Linux; launch the macOS app for dictation".into(),
        );
    }
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ipc::connect,
            ipc::ui_action,
            ipc::host_action
        ])
        .register_asynchronous_uri_scheme_protocol("said-audio", |context, request, responder| {
            let connection = context
                .app_handle()
                .try_state::<ipc::Connection>()
                .map(|state| state.inner().clone());
            std::thread::spawn(move || {
                let response = match connection {
                    Some(connection) => ipc::audio(&connection, request),
                    None => tauri::http::Response::builder()
                        .status(503)
                        .body(Vec::new())
                        .unwrap(),
                };
                responder.respond(response);
            });
        })
        .setup(move |app| {
            let connection = ipc::spawn(app.handle().clone(), paths.clone())
                .map_err(|error| -> Box<dyn std::error::Error> { error })?;
            app.manage(connection);
            let items = [
                MenuItem::with_id(app, "interactive", "Interactive", true, None::<&str>)?,
                MenuItem::with_id(app, "history", "History", true, None::<&str>)?,
                MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?,
                MenuItem::with_id(app, "setup", "Setup", true, None::<&str>)?,
                MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
            ];
            let menu = Menu::with_items(
                app,
                &items
                    .iter()
                    .map(|item| item as &dyn tauri::menu::IsMenuItem<_>)
                    .collect::<Vec<_>>(),
            )?;
            TrayIconBuilder::with_id("main")
                .icon(
                    app.default_window_icon()
                        .ok_or("missing tray icon")?
                        .clone(),
                )
                .menu(&menu)
                .tooltip(env!("PRODUCT_NAME"))
                .on_menu_event(|app, event| {
                    let id = event.id.as_ref();
                    if id == "quit" {
                        let _ = app
                            .state::<ipc::Connection>()
                            .0
                            .send(ipc::Request::Shutdown);
                    } else if id == "interactive" {
                        let (reply, _) = crossbeam_channel::bounded(1);
                        let _ = app.state::<ipc::Connection>().0.send(ipc::Request::Action(
                            said_core::HostAction::Start {
                                mode: Mode::Interactive,
                                scratch: false,
                            },
                            reply,
                        ));
                    } else if let Some(window) = app.get_webview_window(id) {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                })
                .build(app)?;
            if paths.config.join("setup-complete").is_file()
                && let Some(window) = app.get_webview_window("setup")
            {
                window.hide()?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if window.label() == "interactive" {
                    let _ = window
                        .app_handle()
                        .state::<ipc::Connection>()
                        .0
                        .send(ipc::Request::CloseInteractive);
                }
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())?;
    Ok(())
}
