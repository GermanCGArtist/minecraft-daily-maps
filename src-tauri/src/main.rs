// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mapcore::model::{Config, Dimension, SnapshotInfo};
use mapcore::pipeline::{self, ImportOutcome, ImportRequest};
use mapcore::site;
use mapcore::store::{SnapRef, Store};
use mapcore::timeutil::{self, GuessedTime};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

/* ------------------------------------------------------------------ */
/* State                                                              */
/* ------------------------------------------------------------------ */

#[derive(Clone, Serialize)]
struct StepState {
    id: String,
    label: String,
    state: String, // pending | running | done | skip | error
    message: String,
}

#[derive(Clone)]
enum Payload {
    Import(ImportRequest),
    Rerender(SnapRef, Vec<Dimension>),
}

#[derive(Clone, Serialize)]
struct Job {
    id: u64,
    kind: String, // import | rerender
    title: String,
    status: String, // awaiting | queued | running | done | duplicate | error
    steps: Vec<StepState>,
    message: String,
    path: String,
    date: String,
    time: String,
    dimensions: Vec<String>,
    from_inbox: bool,
    #[serde(skip)]
    payload: Option<Payload>,
}

#[derive(Default)]
struct Inner {
    store: Option<Store>,
    cfg: Config,
    queue: Vec<Job>,
    next_id: u64,
}

struct AppState {
    inner: Mutex<Inner>,
    tx: Mutex<Sender<u64>>,
}

type Shared = Arc<AppState>;

#[derive(Serialize)]
struct StateDto {
    data_dir: Option<String>,
    suggested_data_dir: String,
    platform: &'static str,
    config: Config,
    snapshots: Vec<SnapshotInfo>,
    queue: Vec<Job>,
}

#[derive(Serialize, Deserialize, Default)]
struct Bootstrap {
    data_dir: Option<String>,
}

fn step_label(id: &str) -> String {
    match id {
        "hash" => "Check for duplicates".to_string(),
        "extract" => "Extract world".to_string(),
        "diff" => "Compare with previous snapshot".to_string(),
        "html" => "Build HTML".to_string(),
        other => match other.strip_prefix("render:") {
            Some("overworld") => "Render Overworld".to_string(),
            Some("nether") => "Render Nether".to_string(),
            Some("end") => "Render The End".to_string(),
            Some(d) => format!("Render {}", d),
            None => other.to_string(),
        },
    }
}

fn hhmm(t: &str) -> String {
    if t.len() == 4 {
        format!("{}:{}", &t[0..2], &t[2..4])
    } else {
        t.to_string()
    }
}

fn dims_from_strings(v: &[String]) -> Vec<Dimension> {
    v.iter().filter_map(|s| Dimension::from_key(s)).collect()
}

fn dim_keys(v: &[Dimension]) -> Vec<String> {
    v.iter().map(|d| d.key().to_string()).collect()
}

fn emit_queue(app: &AppHandle, st: &Shared) {
    let queue = st.inner.lock().unwrap().queue.clone();
    let _ = app.emit("queue-update", queue);
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

fn current_store(st: &Shared) -> Result<(Store, Config), String> {
    let g = st.inner.lock().unwrap();
    match &g.store {
        Some(s) => Ok((s.clone(), g.cfg.clone())),
        None => Err("Choose a data folder first.".to_string()),
    }
}

fn bootstrap_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("bootstrap.json"))
}

fn load_bootstrap(app: &AppHandle) -> Bootstrap {
    bootstrap_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_bootstrap(app: &AppHandle, b: &Bootstrap) {
    if let Some(p) = bootstrap_path(app) {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(s) = serde_json::to_string_pretty(b) {
            let _ = std::fs::write(p, s);
        }
    }
}

fn suggested_data_dir(app: &AppHandle) -> String {
    app.path()
        .document_dir()
        .or_else(|_| app.path().home_dir())
        .map(|d| d.join("minecraft-maps").to_string_lossy().to_string())
        .unwrap_or_else(|_| "minecraft-maps".to_string())
}

fn build_state(app: &AppHandle, st: &Shared) -> StateDto {
    let (store, cfg, queue) = {
        let g = st.inner.lock().unwrap();
        (g.store.clone(), g.cfg.clone(), g.queue.clone())
    };
    StateDto {
        data_dir: store.as_ref().map(|s| s.root.to_string_lossy().to_string()),
        suggested_data_dir: suggested_data_dir(app),
        platform: std::env::consts::OS,
        config: cfg,
        snapshots: store
            .as_ref()
            .map(|s| s.snapshot_infos())
            .unwrap_or_default(),
        queue,
    }
}

/* ------------------------------------------------------------------ */
/* Job queue                                                          */
/* ------------------------------------------------------------------ */

fn push_job(app: &AppHandle, st: &Shared, mut job: Job, start: bool) -> u64 {
    let id = {
        let mut g = st.inner.lock().unwrap();
        g.next_id += 1;
        job.id = g.next_id;
        job.status = if start {
            "queued".into()
        } else {
            "awaiting".into()
        };
        let id = job.id;
        g.queue.push(job);
        id
    };
    if start {
        let _ = st.tx.lock().unwrap().send(id);
    }
    emit_queue(app, st);
    id
}

fn new_job(
    kind: &str,
    title: String,
    path: String,
    date: String,
    time: String,
    dims: Vec<String>,
    from_inbox: bool,
) -> Job {
    Job {
        id: 0,
        kind: kind.to_string(),
        title,
        status: "queued".into(),
        steps: Vec::new(),
        message: String::new(),
        path,
        date,
        time,
        dimensions: dims,
        from_inbox,
        payload: None,
    }
}

fn update_step(app: &AppHandle, st: &Shared, job_id: u64, step: &str, state: &str, msg: &str) {
    {
        let mut g = st.inner.lock().unwrap();
        if let Some(job) = g.queue.iter_mut().find(|j| j.id == job_id) {
            let new_state = match state {
                "start" => "running",
                "done" => "done",
                "skip" => "skip",
                "error" => "error",
                _ => "",
            };
            let idx = match job.steps.iter().position(|s| s.id == step) {
                Some(i) => i,
                None => {
                    job.steps.push(StepState {
                        id: step.to_string(),
                        label: step_label(step),
                        state: "pending".into(),
                        message: String::new(),
                    });
                    job.steps.len() - 1
                }
            };
            let s = &mut job.steps[idx];
            if !new_state.is_empty() {
                s.state = new_state.to_string();
            }
            if !msg.is_empty() {
                s.message = msg.to_string();
            }
        }
    }
    emit_queue(app, st);
}

fn finish_job(app: &AppHandle, st: &Shared, job_id: u64, status: &str, message: String) {
    {
        let mut g = st.inner.lock().unwrap();
        if let Some(job) = g.queue.iter_mut().find(|j| j.id == job_id) {
            job.status = status.to_string();
            job.message = message;
            // A failed step stays marked; anything still running was interrupted.
            for s in job.steps.iter_mut() {
                if s.state == "running" {
                    s.state = if status == "error" {
                        "error".into()
                    } else {
                        "done".into()
                    };
                }
            }
        }
    }
    emit_queue(app, st);
    let _ = app.emit("snapshots-changed", ());
}

fn run_job(app: &AppHandle, st: &Shared, job_id: u64) {
    let (payload, title, store_cfg) = {
        let mut g = st.inner.lock().unwrap();
        let store_cfg = g.store.clone().map(|s| (s, g.cfg.clone()));
        match g.queue.iter_mut().find(|j| j.id == job_id) {
            Some(job) => {
                job.status = "running".into();
                (job.payload.clone(), job.title.clone(), store_cfg)
            }
            None => return, // dismissed while queued
        }
    };
    emit_queue(app, st);

    let (store, cfg) = match store_cfg {
        Some(x) => x,
        None => {
            finish_job(app, st, job_id, "error", "No data folder selected.".into());
            return;
        }
    };
    let payload = match payload {
        Some(p) => p,
        None => {
            finish_job(app, st, job_id, "error", "Nothing to do.".into());
            return;
        }
    };

    // Pre-register the steps so the UI shows the whole plan immediately.
    let planned = match &payload {
        Payload::Import(req) => pipeline::plan_steps(&req.dimensions, true),
        Payload::Rerender(_, dims) => pipeline::plan_steps(dims, false),
    };
    {
        let mut g = st.inner.lock().unwrap();
        if let Some(job) = g.queue.iter_mut().find(|j| j.id == job_id) {
            job.steps = planned
                .iter()
                .map(|id| StepState {
                    id: id.clone(),
                    label: step_label(id),
                    state: "pending".into(),
                    message: String::new(),
                })
                .collect();
        }
    }
    emit_queue(app, st);

    let report =
        |step: &str, state: &str, msg: &str| update_step(app, st, job_id, step, state, msg);

    match payload {
        Payload::Import(req) => {
            let from_inbox = req.move_zip;
            let zip_path = req.zip_path.clone();
            match pipeline::import(&cfg, &store, &req, &report) {
                Ok(ImportOutcome::Imported(r)) => {
                    let msg = format!("Imported {} {}", r.date, hhmm(&r.id));
                    finish_job(app, st, job_id, "done", msg.clone());
                    notify(app, "Map ready", &msg);
                }
                Ok(ImportOutcome::Duplicate(r)) => {
                    if from_inbox {
                        move_to_duplicates(&store, &zip_path);
                    }
                    let msg = format!(
                        "Skipped: this zip was already imported as {} {}",
                        r.date,
                        hhmm(&r.id)
                    );
                    finish_job(app, st, job_id, "duplicate", msg);
                }
                Err(e) => {
                    let msg = format!("{:#}", e);
                    finish_job(app, st, job_id, "error", msg.clone());
                    notify(app, "Import failed", &format!("{}: {}", title, msg));
                }
            }
        }
        Payload::Rerender(r, dims) => match pipeline::rerender(&cfg, &store, &r, &dims, &report) {
            Ok(()) => {
                let msg = format!("Re-rendered {} {}", r.date, hhmm(&r.id));
                finish_job(app, st, job_id, "done", msg.clone());
                notify(app, "Re-render finished", &msg);
            }
            Err(e) => {
                let msg = format!("{:#}", e);
                finish_job(app, st, job_id, "error", msg.clone());
                notify(app, "Re-render failed", &msg);
            }
        },
    }
}

fn move_to_duplicates(store: &Store, zip: &Path) {
    let dir = store.inbox_dir().join("_duplicates");
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Some(name) = zip.file_name() {
            let _ = std::fs::rename(zip, dir.join(name));
        }
    }
}

fn worker(app: AppHandle, st: Shared, rx: std::sync::mpsc::Receiver<u64>) {
    for id in rx {
        run_job(&app, &st, id);
    }
}

/// Watches `inbox/` for finished zips. A file is picked up once its size has
/// stopped changing between two polls.
fn inbox_watcher(app: AppHandle, st: Shared) {
    let mut sizes: HashMap<PathBuf, u64> = HashMap::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    loop {
        std::thread::sleep(Duration::from_secs(4));
        let (store, cfg) = match current_store(&st) {
            Ok(x) => x,
            Err(_) => continue,
        };
        let entries = match std::fs::read_dir(store.inbox_dir()) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_zip = path.is_file()
                && path
                    .extension()
                    .map(|x| x.eq_ignore_ascii_case("zip"))
                    .unwrap_or(false);
            if !is_zip || seen.contains(&path) {
                continue;
            }
            let size = match entry.metadata() {
                Ok(m) => m.len(),
                Err(_) => continue,
            };
            if sizes.get(&path) != Some(&size) {
                sizes.insert(path, size);
                continue;
            }
            // Stable: queue it exactly once while it stays in the inbox.
            seen.insert(path.clone());
            let guess = timeutil::guess_import_time(&path);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let dims = dim_keys(&cfg.default_dimensions);
            let mut job = new_job(
                "import",
                name,
                path.to_string_lossy().to_string(),
                guess.date.clone(),
                guess.time.clone(),
                dims.clone(),
                true,
            );
            if cfg.auto_import_inbox {
                job.payload = Some(Payload::Import(ImportRequest {
                    zip_path: path.clone(),
                    date: guess.date,
                    time: guess.time,
                    dimensions: dims_from_strings(&dims),
                    move_zip: true,
                }));
                push_job(&app, &st, job, true);
            } else {
                push_job(&app, &st, job, false);
            }
        }
        sizes.retain(|p, _| p.exists());
        seen.retain(|p| p.exists());
    }
}

/* ------------------------------------------------------------------ */
/* Commands                                                           */
/* ------------------------------------------------------------------ */

#[tauri::command]
async fn get_state(app: AppHandle, state: State<'_, Shared>) -> Result<StateDto, String> {
    Ok(build_state(&app, state.inner()))
}

#[tauri::command]
async fn set_data_dir(
    app: AppHandle,
    state: State<'_, Shared>,
    path: String,
) -> Result<StateDto, String> {
    let store = Store::new(PathBuf::from(&path));
    store.ensure_layout().map_err(|e| format!("{:#}", e))?;
    let cfg = store.load_config();
    store.save_config(&cfg).map_err(|e| format!("{:#}", e))?;
    {
        let mut g = state.inner().inner.lock().unwrap();
        g.store = Some(store.clone());
        g.cfg = cfg;
    }
    save_bootstrap(
        &app,
        &Bootstrap {
            data_dir: Some(path),
        },
    );
    let s2 = store.clone();
    std::thread::spawn(move || {
        let _ = site::rebuild_all(&s2);
    });
    Ok(build_state(&app, state.inner()))
}

#[tauri::command]
async fn save_config(
    app: AppHandle,
    state: State<'_, Shared>,
    config: Config,
) -> Result<StateDto, String> {
    let (store, _) = current_store(state.inner())?;
    store.save_config(&config).map_err(|e| format!("{:#}", e))?;
    state.inner().inner.lock().unwrap().cfg = config;
    Ok(build_state(&app, state.inner()))
}

#[tauri::command]
async fn guess_time(path: String) -> Result<GuessedTime, String> {
    Ok(timeutil::guess_import_time(Path::new(&path)))
}

#[tauri::command]
async fn enqueue_import(
    app: AppHandle,
    state: State<'_, Shared>,
    path: String,
    date: String,
    time: String,
    dimensions: Vec<String>,
) -> Result<u64, String> {
    current_store(state.inner())?;
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(format!("File not found: {}", path));
    }
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let in_inbox = current_store(state.inner())
        .map(|(s, _)| p.starts_with(s.inbox_dir()))
        .unwrap_or(false);
    let mut job = new_job(
        "import",
        name,
        path,
        date.clone(),
        time.clone(),
        dimensions.clone(),
        in_inbox,
    );
    job.payload = Some(Payload::Import(ImportRequest {
        zip_path: p,
        date,
        time,
        dimensions: dims_from_strings(&dimensions),
        move_zip: in_inbox,
    }));
    Ok(push_job(&app, state.inner(), job, true))
}

/// Confirm an inbox job that was waiting for the user (editable date/time/dimensions).
#[tauri::command]
async fn confirm_job(
    app: AppHandle,
    state: State<'_, Shared>,
    id: u64,
    date: String,
    time: String,
    dimensions: Vec<String>,
) -> Result<(), String> {
    {
        let mut g = state.inner().inner.lock().unwrap();
        let job = g
            .queue
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or("Job not found")?;
        if job.status != "awaiting" {
            return Err("This job is not waiting for confirmation.".into());
        }
        let p = PathBuf::from(&job.path);
        if !p.is_file() {
            return Err("The file is no longer in the inbox.".into());
        }
        job.date = date.clone();
        job.time = time.clone();
        job.dimensions = dimensions.clone();
        job.status = "queued".into();
        job.payload = Some(Payload::Import(ImportRequest {
            zip_path: p,
            date,
            time,
            dimensions: dims_from_strings(&dimensions),
            move_zip: job.from_inbox,
        }));
    }
    let _ = state.inner().tx.lock().unwrap().send(id);
    emit_queue(&app, state.inner());
    Ok(())
}

#[tauri::command]
async fn dismiss_job(app: AppHandle, state: State<'_, Shared>, id: u64) -> Result<(), String> {
    {
        let mut g = state.inner().inner.lock().unwrap();
        g.queue.retain(|j| !(j.id == id && j.status != "running"));
    }
    emit_queue(&app, state.inner());
    Ok(())
}

#[tauri::command]
async fn clear_finished(app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    {
        let mut g = state.inner().inner.lock().unwrap();
        g.queue
            .retain(|j| matches!(j.status.as_str(), "awaiting" | "queued" | "running"));
    }
    emit_queue(&app, state.inner());
    Ok(())
}

#[tauri::command]
async fn enqueue_rerender(
    app: AppHandle,
    state: State<'_, Shared>,
    date: String,
    time: String,
    dimensions: Vec<String>,
) -> Result<u64, String> {
    current_store(state.inner())?;
    let r = SnapRef {
        date: date.clone(),
        id: time.clone(),
    };
    let mut job = new_job(
        "rerender",
        format!("Re-render {} {}", date, hhmm(&time)),
        String::new(),
        date,
        time,
        dimensions.clone(),
        false,
    );
    job.payload = Some(Payload::Rerender(r, dims_from_strings(&dimensions)));
    Ok(push_job(&app, state.inner(), job, true))
}

#[tauri::command]
async fn delete_snapshot(
    app: AppHandle,
    state: State<'_, Shared>,
    date: String,
    time: String,
) -> Result<StateDto, String> {
    let (store, _) = current_store(state.inner())?;
    let r = SnapRef { date, id: time };
    tauri::async_runtime::spawn_blocking(move || pipeline::delete_snapshot(&store, &r))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{:#}", e))?;
    Ok(build_state(&app, state.inner()))
}

#[tauri::command]
async fn open_viewer(app: AppHandle, state: State<'_, Shared>) -> Result<(), String> {
    let (store, _) = current_store(state.inner())?;
    let index = store.site_dir().join("index.html");
    if !index.exists() {
        site::rebuild_index(&store).map_err(|e| format!("{:#}", e))?;
    }
    app.opener()
        .open_path(index.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_snapshot(
    app: AppHandle,
    state: State<'_, Shared>,
    date: String,
    time: String,
) -> Result<(), String> {
    let (store, _) = current_store(state.inner())?;
    let page = store.snapshot_dir(&date, &time).join("index.html");
    if !page.exists() {
        return Err("This snapshot has no page yet. Re-render it first.".into());
    }
    app.opener()
        .open_path(page.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Open the data folder, or a snapshot folder when date and time are given.
#[tauri::command]
async fn open_folder(
    app: AppHandle,
    state: State<'_, Shared>,
    date: Option<String>,
    time: Option<String>,
) -> Result<(), String> {
    let (store, _) = current_store(state.inner())?;
    let dir = match (date, time) {
        (Some(d), Some(t)) => store.snapshot_dir(&d, &t),
        (Some(d), None) => store.day_dir(&d),
        _ => store.root.clone(),
    };
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn rebuild_site(state: State<'_, Shared>) -> Result<(), String> {
    let (store, _) = current_store(state.inner())?;
    tauri::async_runtime::spawn_blocking(move || site::rebuild_all(&store))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{:#}", e))
}

/* ------------------------------------------------------------------ */
/* Entry point                                                        */
/* ------------------------------------------------------------------ */

fn main() {
    let (tx, rx) = channel::<u64>();
    let shared: Shared = Arc::new(AppState {
        inner: Mutex::new(Inner::default()),
        tx: Mutex::new(tx),
    });
    let setup_state = shared.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_data_dir,
            save_config,
            guess_time,
            enqueue_import,
            confirm_job,
            dismiss_job,
            clear_finished,
            enqueue_rerender,
            delete_snapshot,
            open_viewer,
            open_snapshot,
            open_folder,
            rebuild_site,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // Restore the data folder chosen on first launch.
            let boot = load_bootstrap(&handle);
            if let Some(dir) = boot.data_dir {
                let store = Store::new(PathBuf::from(dir));
                if store.root.exists() && store.ensure_layout().is_ok() {
                    let cfg = store.load_config();
                    {
                        let mut g = setup_state.inner().inner.lock().unwrap();
                        g.store = Some(store.clone());
                        g.cfg = cfg;
                    }
                    std::thread::spawn(move || {
                        let _ = site::rebuild_all(&store);
                    });
                }
            }

            let _ = handle.notification().request_permission();

            let (h1, s1) = (handle.clone(), setup_state.clone());
            std::thread::spawn(move || worker(h1, s1, rx));
            let (h2, s2) = (handle, setup_state);
            std::thread::spawn(move || inbox_watcher(h2, s2));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Minecraft Daily Maps");
}
