use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, RunEvent, State};

const API_HOST: &str = "127.0.0.1";
const API_PORT: u16 = 8080;

struct ApiProcess(Mutex<Option<Child>>);

fn resource_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .resource_dir()
        .map_err(|e| format!("resource dir: {e}"))
}

fn resolve_api_binary(root: &PathBuf) -> PathBuf {
    let candidates = [
        root.join("resources/cuas_api"),
        root.join("cuas_api"),
        // Dev: workspace release/debug next to checkout
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/debug/cuas_api"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/release/cuas_api"),
    ];
    for c in candidates {
        if c.exists() {
            return c;
        }
    }
    candidates[0].clone()
}

fn resolve_console_dist(root: &PathBuf) -> PathBuf {
    let candidates = [
        root.join("resources/console"),
        root.join("console"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../apps/console/dist"),
    ];
    for c in candidates {
        if c.join("index.html").exists() {
            return c;
        }
    }
    candidates[0].clone()
}

fn resolve_scenarios_dir(root: &PathBuf) -> PathBuf {
    let candidates = [
        root.join("resources/scenarios"),
        root.join("scenarios"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../scenarios"),
    ];
    for c in candidates {
        if c.join("military-base-swarm/scenario.json").exists() {
            return c;
        }
    }
    candidates[0].clone()
}

fn wait_for_health(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let url_host = format!("{API_HOST}:{API_PORT}");
    while Instant::now() < deadline {
        if TcpStream::connect(&url_host).is_ok() {
            // TCP up — give axum a beat to accept HTTP
            thread::sleep(Duration::from_millis(150));
            if let Ok(resp) = ureq_get_health() {
                if resp {
                    return true;
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn ureq_get_health() -> Result<bool, String> {
    // Avoid extra dep: raw HTTP via TcpStream
    use std::io::{Read, Write};
    let mut stream = TcpStream::connect((API_HOST, API_PORT)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok();
    let req = format!(
        "GET /health HTTP/1.1\r\nHost: {API_HOST}:{API_PORT}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf).ok();
    Ok(buf.contains("200") && buf.contains("ok"))
}

fn spawn_api(app: &AppHandle) -> Result<Child, String> {
    let root = resource_root(app).unwrap_or_else(|_| PathBuf::from("."));
    let api_bin = resolve_api_binary(&root);
    let console = resolve_console_dist(&root);
    let scenarios = resolve_scenarios_dir(&root);

    if !api_bin.exists() {
        return Err(format!(
            "cuas_api binary not found at {} — run scripts/prepare-desktop-resources.sh",
            api_bin.display()
        ));
    }
    if !console.join("index.html").exists() {
        return Err(format!(
            "console dist missing at {} — build apps/console first",
            console.display()
        ));
    }

    // Workdir = parent of scenarios so resolve_scenario_dir("military-base-swarm") works
    let workdir = scenarios
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.clone());

    // CWD must contain `scenarios/<name>/scenario.json` for resolve_scenario_dir.
    let workdir = if scenarios.file_name().and_then(|s| s.to_str()) == Some("scenarios") {
        scenarios
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| workdir)
    } else {
        workdir
    };

    Command::new(&api_bin)
        .current_dir(&workdir)
        .arg("--port")
        .arg(API_PORT.to_string())
        .arg("--console-dist")
        .arg(&console)
        // Default: idle — operator presses Start (no --auto-start)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn cuas_api ({}): {e}", api_bin.display()))
}

fn kill_api(state: &ApiProcess) {
    if let Ok(mut guard) = state.0.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(ApiProcess(Mutex::new(None)))
        .setup(|app| {
            let handle = app.handle().clone();
            let child = spawn_api(&handle).map_err(|e| {
                eprintln!("[cuas-desktop] {e}");
                e
            })?;
            {
                let state = app.state::<ApiProcess>();
                *state.0.lock().unwrap() = Some(child);
            }

            if !wait_for_health(Duration::from_secs(15)) {
                eprintln!("[cuas-desktop] timed out waiting for cuas_api on :{API_PORT}");
            }

            if let Some(window) = app.get_webview_window("main") {
                let url = format!("http://{API_HOST}:{API_PORT}/");
                let _ = window.eval(&format!("window.location.replace('{url}')"));
                let _ = window.show();
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error building CUAS desktop")
        .run(|app_handle, event| {
            if let RunEvent::Exit = event {
                let state = app_handle.state::<ApiProcess>();
                kill_api(&state);
            }
        });
}
