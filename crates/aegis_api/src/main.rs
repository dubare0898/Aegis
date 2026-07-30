use aegis_fusion::FusionEngine;
use aegis_recommend::{AcceptEffect, RecommendEngine};
use aegis_schema::{AirPicture, ClientCommand, OperatorState, ServerMessage, ZoneKind};
use aegis_sim::{resolve_scenario_dir, Simulation};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "aegis_api")]
struct Args {
    #[arg(long, default_value = "military-base-swarm")]
    scenario: String,
    #[arg(long, default_value_t = 8080)]
    port: u16,
    #[arg(long)]
    seed: Option<u64>,
    /// Directory of built operator console (`index.html`). Served as the web UI.
    #[arg(long)]
    console_dist: Option<PathBuf>,
    /// If set, begin advancing the simulation immediately (default: paused until Start).
    #[arg(long, default_value_t = false)]
    auto_start: bool,
}

struct AppState {
    sim: RwLock<Simulation>,
    fusion: RwLock<FusionEngine>,
    recommend: RwLock<RecommendEngine>,
    last_picture: RwLock<Option<AirPicture>>,
    /// Latest fused track positions for effector targeting.
    last_track_pos: RwLock<Vec<(Uuid, aegis_schema::Vec3)>>,
    tx: broadcast::Sender<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "aegis_api=info,tower_http=info".into()),
        )
        .init();

    let args = Args::parse();
    let scenario_dir = resolve_scenario_dir(&args.scenario);
    info!(path = %scenario_dir.display(), "loading scenario");
    let mut sim = Simulation::load(&scenario_dir, args.seed)?;
    let seed = sim.seed;
    if args.auto_start {
        info!("auto-start enabled — simulation running");
        sim.start();
    } else {
        info!("simulation idle — wait for operator Start");
    }

    let (tx, _) = broadcast::channel::<String>(64);
    let state = Arc::new(AppState {
        sim: RwLock::new(sim),
        fusion: RwLock::new(FusionEngine::new(seed)),
        recommend: RwLock::new(RecommendEngine::new()),
        last_picture: RwLock::new(None),
        last_track_pos: RwLock::new(Vec::new()),
        tx: tx.clone(),
    });

    let tick_state = state.clone();
    tokio::spawn(async move {
        loop {
            let snapshot = {
                let mut sim = tick_state.sim.write().await;
                let mut fusion = tick_state.fusion.write().await;
                let mut recommend = tick_state.recommend.write().await;

                let speed = sim.speed;
                let steps = speed.round().max(1.0) as u32;
                let mut detections = Vec::new();
                let dt = sim.dt;
                for _ in 0..steps {
                    detections = sim.step();
                }
                let t = sim.t;
                let mut tracks = fusion.process(t, dt * steps as f64, &detections);
                let track_pos: Vec<_> = tracks.iter().map(|tr| (tr.id, tr.position)).collect();
                sim.set_eo_truth_targets(&track_pos);
                *tick_state.last_track_pos.write().await = track_pos;

                // Sync neutralized truth into operator state for UI.
                for te in sim.truth_entities() {
                    if te.neutralized {
                        recommend.mark_neutralized(te.id);
                    }
                }

                let recs = recommend.evaluate(t, &mut tracks, sim.zones());
                build_picture(&sim, &recommend, detections, tracks, recs)
            };

            *tick_state.last_picture.write().await = Some(snapshot.clone());
            let msg = ServerMessage::Snapshot(snapshot);
            if let Ok(json) = serde_json::to_string(&msg) {
                let _ = tick_state.tx.send(json);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    let mut app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/snapshot", get(get_snapshot))
        .route("/api/command", post(post_command))
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    if let Some(dist) = args.console_dist {
        if dist.exists() {
            info!(path = %dist.display(), "serving console dist");
            let index = dist.join("index.html");
            app =
                app.fallback_service(ServeDir::new(dist).not_found_service(ServeFile::new(index)));
        } else {
            warn!(path = %dist.display(), "console_dist path does not exist");
        }
    }

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_picture(
    sim: &Simulation,
    recommend: &RecommendEngine,
    detections: Vec<aegis_schema::Detection>,
    tracks: Vec<aegis_schema::Track>,
    recommendations: Vec<aegis_schema::Recommendation>,
) -> AirPicture {
    AirPicture {
        t: sim.t,
        tick: sim.tick,
        running: sim.running,
        speed: sim.speed,
        seed: sim.seed,
        scenario_id: sim.manifest.id.clone(),
        detections,
        tracks,
        recommendations,
        sensors: sim.sensor_status(),
        zones: sim.zones().to_vec(),
        truth: sim.truth_entities(),
        operator_events: recommend.operator_events().to_vec(),
        operator_state: recommend.operator_state().clone(),
        effectors: sim.effector_status(),
        defeat_events: sim.defeat_events().to_vec(),
    }
}

async fn get_snapshot(State(state): State<Arc<AppState>>) -> Json<AirPicture> {
    if let Some(pic) = state.last_picture.read().await.clone() {
        return Json(pic);
    }
    let sim = state.sim.read().await;
    let recommend = state.recommend.read().await;
    Json(AirPicture {
        t: sim.t,
        tick: sim.tick,
        running: sim.running,
        speed: sim.speed,
        seed: sim.seed,
        scenario_id: sim.manifest.id.clone(),
        detections: sim.last_detections.clone(),
        tracks: Vec::new(),
        recommendations: Vec::new(),
        sensors: sim.sensor_status(),
        zones: sim.zones().to_vec(),
        truth: sim.truth_entities(),
        operator_events: recommend.operator_events().to_vec(),
        operator_state: OperatorState::default(),
        effectors: sim.effector_status(),
        defeat_events: sim.defeat_events().to_vec(),
    })
}

async fn post_command(
    State(state): State<Arc<AppState>>,
    Json(cmd): Json<ClientCommand>,
) -> Json<ServerMessage> {
    apply_command(&state, cmd).await;
    Json(ServerMessage::Info {
        message: "ok".into(),
    })
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client_socket(socket, state))
}

async fn client_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => match serde_json::from_str::<ClientCommand>(&text) {
                Ok(cmd) => apply_command(&state, cmd).await,
                Err(err) => warn!("bad client command: {err}"),
            },
            Message::Close(_) => break,
            _ => {}
        }
    }

    send_task.abort();
}

async fn apply_command(state: &AppState, cmd: ClientCommand) {
    match cmd {
        ClientCommand::Start => state.sim.write().await.start(),
        ClientCommand::Pause => state.sim.write().await.pause(),
        ClientCommand::SetSpeed { speed } => state.sim.write().await.set_speed(speed),
        ClientCommand::Reset { seed } => {
            let mut sim = state.sim.write().await;
            sim.reset(seed);
            let seed = sim.seed;
            sim.pause();
            drop(sim);
            state.fusion.write().await.reset_with_seed(seed);
            state.recommend.write().await.reset();
            state.last_track_pos.write().await.clear();
        }
        ClientCommand::CueEo { track_id } => state.sim.write().await.cue_eo(track_id),
        ClientCommand::FailSensor { sensor_id } => state.sim.write().await.fail_sensor(&sensor_id),
        ClientCommand::RestoreSensor { sensor_id } => {
            state.sim.write().await.restore_sensor(&sensor_id)
        }
        ClientCommand::DisposeRecommendation {
            recommendation_id,
            disposition,
            actor,
            reason_code,
        } => {
            let t = state.sim.read().await.t;
            let disposed = {
                let mut recommend = state.recommend.write().await;
                recommend.dispose(t, recommendation_id, disposition, actor, reason_code)
            };
            if let Some((_event, effect)) = disposed {
                apply_accept_effect(state, effect).await;
            }
        }
    }
}

async fn apply_accept_effect(state: &AppState, effect: AcceptEffect) {
    match effect {
        AcceptEffect::CueEo { track_id } => {
            state.sim.write().await.cue_eo(track_id);
            state
                .recommend
                .write()
                .await
                .annotate_effect_result("EO tasked");
        }
        AcceptEffect::AlertSector => {
            let mut recommend = state.recommend.write().await;
            let sim = state.sim.read().await;
            for z in sim.zones() {
                if matches!(z.kind, ZoneKind::KeepOut | ZoneKind::NoFly) {
                    recommend.mark_zone_alerted(z.id.clone());
                }
            }
            recommend.annotate_effect_result("keep-out / no-fly alerted");
        }
        AcceptEffect::ActivateJammer { track_id } => {
            let track_pos = {
                let positions = state.last_track_pos.read().await;
                positions
                    .iter()
                    .find(|(id, _)| *id == track_id)
                    .map(|(_, p)| *p)
            };
            let mut sim = state.sim.write().await;
            let pos = track_pos.unwrap_or(aegis_schema::Vec3::zero());
            let note = sim.activate_jammer(track_id, pos);
            drop(sim);
            state.recommend.write().await.annotate_effect_result(note);
        }
        AcceptEffect::EvacuatePad => {
            state
                .recommend
                .write()
                .await
                .annotate_effect_result("pad evacuated");
        }
        AcceptEffect::HandOff { track_id: _ } => {
            state
                .recommend
                .write()
                .await
                .annotate_effect_result("higher echelon notified");
        }
        AcceptEffect::EngageKinetic { track_id } => {
            let track_pos = {
                let positions = state.last_track_pos.read().await;
                positions
                    .iter()
                    .find(|(id, _)| *id == track_id)
                    .map(|(_, p)| *p)
            };
            let mut sim = state.sim.write().await;
            let pos = track_pos.unwrap_or(aegis_schema::Vec3::zero());
            let note = sim.fire_kinetic(track_id, pos);
            drop(sim);
            state.recommend.write().await.annotate_effect_result(note);
        }
        AcceptEffect::None => {}
    }
}
