// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mcmap_core::{
    guess_stamp,
    model::{Config, Dimension, Incomplete, SnapshotMeta},
    pipeline::{self, ImportRequest},
    Error, Stamp, Store,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

// ---------------------------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Serialize)]
struct Job {
    id: u64,
    label: String,
    kind: String,
    /// queued | running | done | error | skipped
    status: String,
    step: String,
    detail: String,
    progress: f32,
    message: String,
}

enum Work {
    Import { id: u64, req: ImportRequest },
    Rerender { id: u64, date: String, time: String, dims: Vec<Dimension> },
    Resume { id: u64, date: String, time: String },
}

#[derive(Clone, Serialize)]
struct LogLine {
    ts: String,
    /// info | warn | error
    level: String,
    text: String,
}

struct Shared {
    root: Mutex<Option<PathBuf>>,
    jobs: Mutex<Vec<Job>>,
    logs: Mutex<VecDeque<LogLine>>,
    tx: Mutex<mpsc::Sender<Work>>,
    next_id: AtomicU64,
}

type Sh = Arc<Shared>;

#[derive(Serialize, Deserialize)]
struct Pointer {
    data_dir: PathBuf,
}

#[derive(Serialize)]
struct StateView {
    data_dir: Option<String>,
    config: Option<Config>,
    snapshots: Vec<SnapshotMeta>,
    incomplete: Vec<Incomplete>,
    jobs: Vec<Job>,
    os: &'static str,
}

fn current_store(sh: &Sh) -> Result<Store, String> {
    let root = sh
        .root
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Choose a data folder first.".to_string())?;
    Store::open(root).map_err(|e| e.to_string())
}

fn build_view(sh: &Sh) -> Result<StateView, String> {
    let jobs = sh.jobs.lock().unwrap().clone();
    let root = sh.root.lock().unwrap().clone();
    match root {
        None => Ok(StateView {
            data_dir: None,
            config: None,
            snapshots: vec![],
            incomplete: vec![],
            jobs,
            os: std::env::consts::OS,
        }),
        Some(root) => {
            let store = Store::open(&root).map_err(|e| e.to_string())?;
            Ok(StateView {
                data_dir: Some(root.display().to_string()),
                config: Some(store.load_config().map_err(|e| e.to_string())?),
                snapshots: store.list_snapshots().map_err(|e| e.to_string())?,
                incomplete: store.list_incomplete().map_err(|e| e.to_string())?,
                jobs,
                os: std::env::consts::OS,
            })
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Log: in-memory ring buffer (for the UI), `logs/app.log` in the data folder, and a live event
// ---------------------------------------------------------------------------------------------

fn level_of(text: &str) -> &'static str {
    let l = text.to_lowercase();
    if l.contains(" err]") || l.contains("exception") || l.contains("error:") || l.contains("failed") {
        "error"
    } else if l.contains("[stderr]") || l.contains(" wrn]") || l.contains("warn") {
        "warn"
    } else {
        "info"
    }
}

fn log(app: &AppHandle, sh: &Sh, level: &str, text: impl Into<String>) {
    let text: String = text.into();
    let now = chrono::Local::now();
    let line = LogLine { ts: now.format("%H:%M:%S").to_string(), level: level.to_string(), text };
    {
        let mut logs = sh.logs.lock().unwrap();
        logs.push_back(line.clone());
        while logs.len() > 2000 {
            logs.pop_front();
        }
    }
    let root = sh.root.lock().unwrap().clone();
    if let Some(root) = root {
        let dir = root.join("logs");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("app.log");
        // Keep the file from growing forever: start a fresh one above 20 MB.
        if fs::metadata(&path).map_or(false, |m| m.len() > 20 * 1024 * 1024) {
            let _ = fs::rename(&path, dir.join("app.log.old"));
        }
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(
                f,
                "{} {:<5} {}",
                now.format("%Y-%m-%d %H:%M:%S"),
                line.level.to_uppercase(),
                line.text
            );
        }
    }
    let _ = app.emit("log-line", &line);
}

// ---------------------------------------------------------------------------------------------
// Job queue
// ---------------------------------------------------------------------------------------------

fn update_job(app: &AppHandle, sh: &Sh, id: u64, f: impl FnOnce(&mut Job)) {
    let snapshot = {
        let mut jobs = sh.jobs.lock().unwrap();
        if let Some(j) = jobs.iter_mut().find(|j| j.id == id) {
            f(j);
        }
        jobs.clone()
    };
    let _ = app.emit("queue-update", &snapshot);
}

fn enqueue(app: &AppHandle, sh: &Sh, label: String, kind: &str, make: impl FnOnce(u64) -> Work) {
    let id = sh.next_id.fetch_add(1, Ordering::SeqCst);
    let snapshot = {
        let mut jobs = sh.jobs.lock().unwrap();
        jobs.push(Job {
            id,
            label,
            kind: kind.to_string(),
            status: "queued".into(),
            step: "queued".into(),
            detail: String::new(),
            progress: 0.0,
            message: String::new(),
        });
        // Keep the list short: drop the oldest finished jobs.
        while jobs.len() > 30 {
            if let Some(pos) = jobs.iter().position(|j| j.status != "queued" && j.status != "running") {
                jobs.remove(pos);
            } else {
                break;
            }
        }
        jobs.clone()
    };
    let _ = app.emit("queue-update", &snapshot);
    let work = make(id);
    let desc = match &work {
        Work::Import { req, .. } => format!(
            "Queued import {} as {} {} ({})",
            req.zip_path.display(),
            req.date,
            req.time,
            req.dimensions.iter().map(|d| d.as_str()).collect::<Vec<_>>().join(", ")
        ),
        Work::Rerender { date, time, dims, .. } => format!(
            "Queued re-render of {date} {time} ({})",
            dims.iter().map(|d| d.as_str()).collect::<Vec<_>>().join(", ")
        ),
        Work::Resume { date, time, .. } => format!("Queued resume of unfinished import {date} {time}"),
    };
    log(app, sh, "info", desc);
    let _ = sh.tx.lock().unwrap().send(work);
}

fn notify(app: &AppHandle, body: &str) {
    let _ = app
        .notification()
        .builder()
        .title("Minecraft Daily Maps")
        .body(body)
        .show();
}

fn worker(app: AppHandle, rx: mpsc::Receiver<Work>, sh: Sh) {
    for work in rx {
        let id = match &work {
            Work::Import { id, .. } | Work::Rerender { id, .. } | Work::Resume { id, .. } => *id,
        };
        update_job(&app, &sh, id, |j| {
            j.status = "running".into();
            j.step = "start".into();
        });
        log(&app, &sh, "info", format!("--- job {id} started ---"));

        let store = match current_store(&sh) {
            Ok(s) => s,
            Err(e) => {
                update_job(&app, &sh, id, |j| {
                    j.status = "error".into();
                    j.message = e;
                });
                continue;
            }
        };

        let last_logged = RefCell::new(String::new());
        let progress = |step: &str, detail: &str, frac: f32| {
            update_job(&app, &sh, id, |j| {
                j.step = step.to_string();
                j.detail = detail.to_string();
                j.progress = frac;
            });
            // Log every new step/detail pair (repeated percentage updates are skipped).
            let key = format!("{step}|{detail}");
            if *last_logged.borrow() != key {
                *last_logged.borrow_mut() = key;
                log(&app, &sh, level_of(detail), format!("[{step}] {detail}"));
            }
        };

        let (label, result) = match &work {
            Work::Import { req, .. } => (
                req.zip_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                pipeline::import(&store, req, &progress),
            ),
            Work::Rerender { date, time, dims, .. } => (
                format!("{date} {time}"),
                pipeline::rerender(&store, date, time, dims, &progress),
            ),
            Work::Resume { date, time, .. } => (
                format!("Resume {date} {time}"),
                pipeline::resume(&store, date, time, &progress),
            ),
        };

        match result {
            Ok(meta) => {
                update_job(&app, &sh, id, |j| {
                    j.status = "done".into();
                    j.step = "done".into();
                    j.progress = 1.0;
                    j.message = format!("Snapshot {} {}", meta.date, meta.time);
                });
                log(&app, &sh, "info", format!("--- job {id} finished: snapshot {} {} ---", meta.date, meta.time));
                notify(&app, &format!("Finished {} {}", meta.date, meta.time));
            }
            Err(Error::Duplicate(existing)) => {
                if let Work::Import { req, .. } = &work {
                    if req.move_source {
                        let dir = store.inbox_dir().join("duplicates");
                        let _ = fs::create_dir_all(&dir);
                        if let Some(name) = req.zip_path.file_name() {
                            let _ = fs::rename(&req.zip_path, dir.join(name));
                        }
                    }
                }
                log(&app, &sh, "warn", format!("Skipped: this zip was already imported as {existing}"));
                update_job(&app, &sh, id, |j| {
                    j.status = "skipped".into();
                    j.message = format!("Already imported as {existing}");
                });
            }
            Err(e) => {
                let msg = e.to_string();
                log(&app, &sh, "error", format!("--- job {id} FAILED: {msg} ---"));
                if store.list_incomplete().map_or(false, |l| !l.is_empty()) {
                    log(&app, &sh, "warn", "The partial render was kept. Use Resume under 'Unfinished imports' to continue it.");
                }
                update_job(&app, &sh, id, |j| {
                    j.status = "error".into();
                    j.message = msg.clone();
                });
                notify(&app, &format!("{label} failed: {msg}"));
            }
        }
        let _ = app.emit("state-changed", ());
    }
}

/// Polls `inbox/` and queues new zips once their size has stopped changing.
fn inbox_poller(app: AppHandle, sh: Sh) {
    let mut last_size: HashMap<PathBuf, u64> = HashMap::new();
    let mut queued: HashSet<PathBuf> = HashSet::new();
    loop {
        thread::sleep(Duration::from_secs(5));
        let Ok(store) = current_store(&sh) else { continue };
        let Ok(rd) = fs::read_dir(store.inbox_dir()) else { continue };
        let zips: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().map_or(false, |x| x.eq_ignore_ascii_case("zip")))
            .collect();
        queued.retain(|p| zips.contains(p));
        last_size.retain(|p, _| zips.contains(p));

        let cfg = store.load_config().unwrap_or_default();
        for path in zips {
            if queued.contains(&path) {
                continue;
            }
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if size > 0 && last_size.get(&path) == Some(&size) {
                let stamp = guess_stamp(&path);
                let dims = cfg.default_dimensions.clone();
                let label = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let p = path.clone();
                log(&app, &sh, "info", format!("Found {} in inbox", path.display()));
                enqueue(&app, &sh, label, "import", move |id| Work::Import {
                    id,
                    req: ImportRequest { zip_path: p, date: stamp.date, time: stamp.time, dimensions: dims, move_source: true },
                });
                queued.insert(path);
            } else {
                last_size.insert(path, size);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------------

#[tauri::command]
async fn get_state(sh: State<'_, Sh>) -> Result<StateView, String> {
    build_view(&sh)
}

#[tauri::command]
async fn set_data_dir(app: AppHandle, sh: State<'_, Sh>, path: String) -> Result<StateView, String> {
    let dir = PathBuf::from(path);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Store::open(&dir).map_err(|e| e.to_string())?; // creates inbox/, days/, site/
    let cfg_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&cfg_dir).map_err(|e| e.to_string())?;
    let pointer = serde_json::to_vec_pretty(&Pointer { data_dir: dir.clone() }).map_err(|e| e.to_string())?;
    fs::write(cfg_dir.join("app.json"), pointer).map_err(|e| e.to_string())?;
    *sh.root.lock().unwrap() = Some(dir.clone());
    log(&app, &sh, "info", format!("Data folder: {}", dir.display()));
    build_view(&sh)
}

#[tauri::command]
fn get_log(sh: State<'_, Sh>) -> Vec<LogLine> {
    sh.logs.lock().unwrap().iter().cloned().collect()
}

#[tauri::command]
fn save_config(sh: State<'_, Sh>, config: Config) -> Result<(), String> {
    current_store(&sh)?.save_config(&config).map_err(|e| e.to_string())
}

#[tauri::command]
fn stamp_for(path: String) -> Result<Stamp, String> {
    let p = PathBuf::from(path);
    if !p.is_file() {
        return Err("Not a file".into());
    }
    Ok(guess_stamp(&p))
}

#[derive(Deserialize)]
struct ImportArgs {
    path: String,
    date: String,
    time: String,
    dimensions: Vec<Dimension>,
}

#[tauri::command]
fn enqueue_import(app: AppHandle, sh: State<'_, Sh>, args: ImportArgs) -> Result<(), String> {
    current_store(&sh)?; // fail early if no data folder yet
    let path = PathBuf::from(&args.path);
    let label = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    enqueue(&app, &sh, label, "import", move |id| Work::Import {
        id,
        req: ImportRequest {
            zip_path: path,
            date: args.date,
            time: args.time,
            dimensions: args.dimensions,
            move_source: false,
        },
    });
    Ok(())
}

#[tauri::command]
fn enqueue_rerender(
    app: AppHandle,
    sh: State<'_, Sh>,
    date: String,
    time: String,
    dimensions: Vec<Dimension>,
) -> Result<(), String> {
    current_store(&sh)?;
    let label = format!("Re-render {date} {time}");
    enqueue(&app, &sh, label, "rerender", move |id| Work::Rerender { id, date, time, dims: dimensions });
    Ok(())
}

#[tauri::command]
fn enqueue_resume(app: AppHandle, sh: State<'_, Sh>, date: String, time: String) -> Result<(), String> {
    current_store(&sh)?;
    let label = format!("Resume {date} {time}");
    enqueue(&app, &sh, label, "resume", move |id| Work::Resume { id, date, time });
    Ok(())
}

#[tauri::command]
async fn delete_snapshot(sh: State<'_, Sh>, date: String, time: String) -> Result<StateView, String> {
    let store = current_store(&sh)?;
    pipeline::delete_snapshot(&store, &date, &time).map_err(|e| e.to_string())?;
    build_view(&sh)
}

/// kind: "data_folder" | "site" | "log" | "snapshot" | "snapshot_folder"
#[tauri::command]
fn open_target(
    app: AppHandle,
    sh: State<'_, Sh>,
    kind: String,
    date: Option<String>,
    time: Option<String>,
) -> Result<(), String> {
    let store = current_store(&sh)?;
    let path = match kind.as_str() {
        "data_folder" => store.root.clone(),
        "site" => store.site_dir().join("index.html"),
        "log" => store.root.join("logs").join("app.log"),
        "snapshot" | "snapshot_folder" => {
            let (d, t) = match (date, time) {
                (Some(d), Some(t)) => (d, t),
                _ => return Err("Missing snapshot".into()),
            };
            let dir = store.snapshot_dir(&d, &t);
            if kind == "snapshot" { dir.join("index.html") } else { dir }
        }
        _ => return Err("Unknown target".into()),
    };
    if !path.exists() {
        return Err(format!("{} does not exist yet.", path.display()));
    }
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------

fn main() {
    let (tx, rx) = mpsc::channel::<Work>();
    let shared: Sh = Arc::new(Shared {
        root: Mutex::new(None),
        jobs: Mutex::new(Vec::new()),
        logs: Mutex::new(VecDeque::new()),
        tx: Mutex::new(tx),
        next_id: AtomicU64::new(1),
    });
    let setup_shared = shared.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_log,
            set_data_dir,
            save_config,
            stamp_for,
            enqueue_import,
            enqueue_rerender,
            enqueue_resume,
            delete_snapshot,
            open_target
        ])
        .setup(move |app| {
            // Restore the data folder chosen on first launch.
            if let Ok(dir) = app.path().app_config_dir() {
                if let Ok(bytes) = fs::read(dir.join("app.json")) {
                    if let Ok(p) = serde_json::from_slice::<Pointer>(&bytes) {
                        if p.data_dir.is_dir() {
                            *setup_shared.root.lock().unwrap() = Some(p.data_dir);
                        }
                    }
                }
            }
            log(app.handle(), &setup_shared, "info", &format!("Minecraft Daily Maps v{} started", env!("CARGO_PKG_VERSION")));
            let (h1, s1) = (app.handle().clone(), setup_shared.clone());
            thread::spawn(move || worker(h1, rx, s1));
            let (h2, s2) = (app.handle().clone(), setup_shared);
            thread::spawn(move || inbox_poller(h2, s2));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the application");
}
