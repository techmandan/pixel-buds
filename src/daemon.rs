use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use bluer::rfcomm::{Profile, ProfileHandle, ReqError, Role, Stream};
use bluer::{Adapter, Address, Device, Session};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader};
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot};

use maestro::protocol::codec::Codec;
use maestro::protocol::types::{settings_rsp, OobeAction, RuntimeInfo, SettingsRsp};
use maestro::protocol::utils;
use maestro::pwrpc::client::{Client, ClientHandle};
use maestro::service::settings::{self, AncState, SettingValue};
use maestro::service::MaestroService;

use crate::media::MediaController;
use crate::pbp::PbpClient;
use crate::types::{BatteryInfo, PodBattery, PluginStatus, SettingsInfo};

const POLL_IDLE: Duration = Duration::from_millis(1000);
const RECONNECT_DELAY: Duration = Duration::from_millis(600);
const RESUME_DEBOUNCE: Duration = Duration::from_millis(500);
const OOBE_POLL_INTERVAL: Duration = Duration::from_millis(750);
const MEDIA_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(1500);
const BLUEZ_BATTERY_REFRESH: Duration = Duration::from_secs(30);

const PIXEL_BUDS_CLASS: u32 = 0x240404;
const PIXEL_BUDS2_CLASS: u32 = 0x244404;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum DaemonRequest {
    Poll,
    SetAnc { value: String },
    CycleAnc,
    SetSetting { key: String, value: bool },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PluginStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

enum ControlMsg {
    SetAnc {
        value: String,
        reply: oneshot::Sender<Result<String>>,
    },
    CycleAnc {
        reply: oneshot::Sender<Result<String>>,
    },
    SetSetting {
        key: String,
        value: bool,
        reply: oneshot::Sender<Result<()>>,
    },
}

pub struct Daemon {
    status_path: PathBuf,
    sock_path: PathBuf,
    client: PbpClient,
}

impl Daemon {
    pub fn new() -> Self {
        let state_home = std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| crate::pbp::user_home_dir().join(".local/state"));

        let state_dir = state_home.join("pixel-buds");
        let _ = std::fs::create_dir_all(&state_dir);
        let status_path = state_dir.join("status.json");
        let sock_path = state_dir.join("daemon.sock");
        let client = PbpClient::new(&state_dir);

        Self {
            status_path,
            sock_path,
            client,
        }
    }

    pub fn client(&self) -> &PbpClient {
        &self.client
    }

    pub fn send_request(&self, req: &DaemonRequest) -> Result<DaemonResponse> {
        let mut stream = StdUnixStream::connect(&self.sock_path)
            .context("Failed to connect to pixel-buds daemon socket")?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(4)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

        let mut payload = serde_json::to_string(req)?;
        payload.push('\n');
        stream.write_all(payload.as_bytes())?;
        stream.flush()?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let resp: DaemonResponse = serde_json::from_str(line.trim())
            .context("Invalid response from pixel-buds daemon")?;
        Ok(resp)
    }

    pub fn write_status(&self, status: &PluginStatus) -> Result<()> {
        let json = serde_json::to_string_pretty(status)?;
        let tmp_path = self.status_path.with_extension("tmp");
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &self.status_path)?;
        Ok(())
    }

    pub fn read_status_file(&self) -> Option<PluginStatus> {
        let data = std::fs::read_to_string(&self.status_path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn poll_once(&self) -> Result<PluginStatus> {
        if let Ok(resp) = self.send_request(&DaemonRequest::Poll) {
            if resp.ok {
                if let Some(st) = resp.status {
                    return Ok(st);
                }
            }
        }

        let status = if let Some((mac, name)) = self.client.find_connected_buds() {
            self.client.read_status(&mac, &name, None)?
        } else {
            self.client.disconnected_status()
        };
        let _ = self.write_status(&status);
        Ok(status)
    }

    pub fn run_loop(&self) -> Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("Failed to create tokio runtime")?;

        rt.block_on(self.run_async_loop())
    }

    async fn run_async_loop(&self) -> Result<()> {
        let _ = std::fs::remove_file(&self.sock_path);
        let listener = UnixListener::bind(&self.sock_path)
            .with_context(|| format!("Failed to bind {:?}", self.sock_path))?;

        let initial_status = self
            .read_status_file()
            .unwrap_or_else(|| self.client.disconnected_status());
        let _ = self.write_status(&initial_status);

        let shared_status = Arc::new(Mutex::new(initial_status));
        let (ctrl_tx, mut ctrl_rx) = mpsc::unbounded_channel::<ControlMsg>();

        // Spawn Unix control socket server
        let status_for_sock = Arc::clone(&shared_status);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                };
                let status_ref = Arc::clone(&status_for_sock);
                let tx = ctrl_tx.clone();
                tokio::spawn(async move {
                    let (reader, mut writer) = stream.into_split();
                    let mut lines = TokioBufReader::new(reader).lines();
                    if let Ok(Some(line)) = lines.next_line().await {
                        let resp = match serde_json::from_str::<DaemonRequest>(&line) {
                            Ok(DaemonRequest::Poll) => {
                                let st = status_ref.lock().unwrap().clone();
                                DaemonResponse {
                                    ok: true,
                                    mode: None,
                                    status: Some(st),
                                    error: None,
                                }
                            }
                            Ok(DaemonRequest::SetAnc { value }) => {
                                let (reply_tx, reply_rx) = oneshot::channel();
                                if tx
                                    .send(ControlMsg::SetAnc {
                                        value,
                                        reply: reply_tx,
                                    })
                                    .is_ok()
                                {
                                    match tokio::time::timeout(Duration::from_secs(3), reply_rx).await {
                                        Ok(Ok(Ok(mode))) => DaemonResponse {
                                            ok: true,
                                            mode: Some(mode),
                                            status: None,
                                            error: None,
                                        },
                                        Ok(Ok(Err(e))) => DaemonResponse {
                                            ok: false,
                                            mode: None,
                                            status: None,
                                            error: Some(e.to_string()),
                                        },
                                        _ => DaemonResponse {
                                            ok: false,
                                            mode: None,
                                            status: None,
                                            error: Some("Timed out communicating with earbuds".to_string()),
                                        },
                                    }
                                } else {
                                    DaemonResponse {
                                        ok: false,
                                        mode: None,
                                        status: None,
                                        error: Some("Daemon channel closed".to_string()),
                                    }
                                }
                            }
                            Ok(DaemonRequest::CycleAnc) => {
                                let (reply_tx, reply_rx) = oneshot::channel();
                                if tx.send(ControlMsg::CycleAnc { reply: reply_tx }).is_ok() {
                                    match tokio::time::timeout(Duration::from_secs(3), reply_rx).await {
                                        Ok(Ok(Ok(mode))) => DaemonResponse {
                                            ok: true,
                                            mode: Some(mode),
                                            status: None,
                                            error: None,
                                        },
                                        Ok(Ok(Err(e))) => DaemonResponse {
                                            ok: false,
                                            mode: None,
                                            status: None,
                                            error: Some(e.to_string()),
                                        },
                                        _ => DaemonResponse {
                                            ok: false,
                                            mode: None,
                                            status: None,
                                            error: Some("Timed out cycling ANC".to_string()),
                                        },
                                    }
                                } else {
                                    DaemonResponse {
                                        ok: false,
                                        mode: None,
                                        status: None,
                                        error: Some("Daemon channel closed".to_string()),
                                    }
                                }
                            }
                            Ok(DaemonRequest::SetSetting { key, value }) => {
                                let (reply_tx, reply_rx) = oneshot::channel();
                                if tx
                                    .send(ControlMsg::SetSetting {
                                        key,
                                        value,
                                        reply: reply_tx,
                                    })
                                    .is_ok()
                                {
                                    match tokio::time::timeout(Duration::from_secs(3), reply_rx).await {
                                        Ok(Ok(Ok(()))) => DaemonResponse {
                                            ok: true,
                                            mode: None,
                                            status: None,
                                            error: None,
                                        },
                                        Ok(Ok(Err(e))) => DaemonResponse {
                                            ok: false,
                                            mode: None,
                                            status: None,
                                            error: Some(e.to_string()),
                                        },
                                        _ => DaemonResponse {
                                            ok: false,
                                            mode: None,
                                            status: None,
                                            error: Some("Timed out setting option".to_string()),
                                        },
                                    }
                                } else {
                                    DaemonResponse {
                                        ok: false,
                                        mode: None,
                                        status: None,
                                        error: Some("Daemon channel closed".to_string()),
                                    }
                                }
                            }
                            Err(e) => DaemonResponse {
                                ok: false,
                                mode: None,
                                status: None,
                                error: Some(format!("Invalid request: {}", e)),
                            },
                        };

                        if let Ok(mut out) = serde_json::to_string(&resp) {
                            out.push('\n');
                            let _ = writer.write_all(out.as_bytes()).await;
                            let _ = writer.flush().await;
                        }
                    }
                });
            }
        });

        let session = Session::new().await.context("Failed to connect to BlueZ D-Bus")?;
        let adapter = session.default_adapter().await.context("No Bluetooth adapter found")?;

        let mut media = MediaController::new();
        let mut prev_left_in_ear: Option<bool> = None;
        let mut prev_right_in_ear: Option<bool> = None;

        loop {
            // Drain any stale control messages while disconnected
            while let Ok(msg) = ctrl_rx.try_recv() {
                match msg {
                    ControlMsg::SetAnc { reply, .. } | ControlMsg::CycleAnc { reply } => {
                        let _ = reply.send(Err(anyhow::anyhow!("Pixel Buds not connected")));
                    }
                    ControlMsg::SetSetting { reply, .. } => {
                        let _ = reply.send(Err(anyhow::anyhow!("Pixel Buds not connected")));
                    }
                }
            }

            if let Some((dev, mac, name)) = find_connected_maestro_device(&adapter).await {
                match connect_maestro_rfcomm(&session, &dev).await {
                    Ok(stream) => {
                        let codec = Codec::new();
                        let framed = codec.wrap(stream);
                        let mut rpc_client = Client::new(framed);
                        let handle = rpc_client.handle();

                        match utils::resolve_channel(&mut rpc_client).await {
                            Ok(channel) => {
                                let session_res = self
                                    .run_connected_session(
                                        rpc_client,
                                        handle,
                                        channel,
                                        &mac,
                                        &name,
                                        &shared_status,
                                        &mut ctrl_rx,
                                        &mut media,
                                        &mut prev_left_in_ear,
                                        &mut prev_right_in_ear,
                                    )
                                    .await;

                                if session_res.is_err() {
                                    // Connection reset (e.g. bud handoff or disconnect)
                                    tokio::time::sleep(RECONNECT_DELAY).await;
                                }
                            }
                            Err(_) => {
                                tokio::time::sleep(RECONNECT_DELAY).await;
                            }
                        }
                    }
                    Err(_) => {
                        // Fallback status while Bluetooth profile is still settling
                        let combined = self.client.get_bluez_battery(&mac).unwrap_or(-1);
                        let now = now_secs();
                        let case = self.client.resolve_cached_case(now);
                        let status = PluginStatus {
                            ok: true,
                            connected: true,
                            device_name: name,
                            model_name: "Google Pixel Buds Pro 2".to_string(),
                            mac,
                            battery: BatteryInfo {
                                left: PodBattery {
                                    level: combined,
                                    charging: false,
                                    in_ear: prev_left_in_ear.unwrap_or(true),
                                    in_case: false,
                                },
                                right: PodBattery {
                                    level: combined,
                                    charging: false,
                                    in_ear: prev_right_in_ear.unwrap_or(true),
                                    in_case: false,
                                },
                                case,
                                combined,
                            },
                            noise_mode: "active".to_string(),
                            settings: SettingsInfo {
                                speech_detection: true,
                                ohd: true,
                                multipoint: true,
                                gestures: true,
                                volume_eq: true,
                            },
                            last_updated: now,
                        };
                        self.update_status(&shared_status, status);
                        tokio::time::sleep(RECONNECT_DELAY).await;
                    }
                }
            } else {
                prev_left_in_ear = None;
                prev_right_in_ear = None;
                media.clear();

                let status = self.client.disconnected_status();
                self.update_status(&shared_status, status);
                tokio::time::sleep(POLL_IDLE).await;
            }
        }
    }

    fn update_status(&self, shared: &Arc<Mutex<PluginStatus>>, status: PluginStatus) {
        let _ = self.write_status(&status);
        if let Ok(mut guard) = shared.lock() {
            *guard = status;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_connected_session<S, E>(
        &self,
        mut rpc_client: Client<S>,
        handle: ClientHandle,
        channel: u32,
        mac: &str,
        name: &str,
        shared_status: &Arc<Mutex<PluginStatus>>,
        ctrl_rx: &mut mpsc::UnboundedReceiver<ControlMsg>,
        media: &mut MediaController,
        prev_left_in_ear: &mut Option<bool>,
        prev_right_in_ear: &mut Option<bool>,
    ) -> Result<()>
    where
        S: futures::Sink<maestro::pwrpc::types::RpcPacket> + Unpin,
        S: futures::Stream<Item = Result<maestro::pwrpc::types::RpcPacket, E>> + Unpin,
        maestro::pwrpc::Error: From<E>,
        maestro::pwrpc::Error: From<S::Error>,
    {
        let mut service = MaestroService::new(handle, channel);

        let task = async {
            let combined = self.client.get_bluez_battery(mac).unwrap_or(-1);
            let now = now_secs();

            // Read initial settings once on connection
            let anc_state = service
                .read_setting(settings::id::CurrentAncrState)
                .await
                .unwrap_or(AncState::Active);
            let speech_detection = service
                .read_setting(settings::id::SpeechDetection)
                .await
                .unwrap_or(true);
            let ohd = service
                .read_setting(settings::id::OhdEnable)
                .await
                .unwrap_or(true);
            let multipoint = service
                .read_setting(settings::id::MultipointEnable)
                .await
                .unwrap_or(true);
            let gestures = service
                .read_setting(settings::id::GestureEnable)
                .await
                .unwrap_or(true);
            let volume_eq = service
                .read_setting(settings::id::VolumeEqEnable)
                .await
                .unwrap_or(true);

            let mut curr_status = PluginStatus {
                ok: true,
                connected: true,
                device_name: name.to_string(),
                model_name: "Google Pixel Buds Pro 2".to_string(),
                mac: mac.to_string(),
                battery: BatteryInfo {
                    left: PodBattery {
                        level: combined,
                        charging: false,
                        in_ear: prev_left_in_ear.unwrap_or(false),
                        in_case: false,
                    },
                    right: PodBattery {
                        level: combined,
                        charging: false,
                        in_ear: prev_right_in_ear.unwrap_or(false),
                        in_case: false,
                    },
                    case: self.client.resolve_cached_case(now),
                    combined,
                },
                noise_mode: anc_to_str(anc_state),
                settings: SettingsInfo {
                    speech_detection,
                    ohd,
                    multipoint,
                    gestures,
                    volume_eq,
                },
                last_updated: now,
            };

            let mut rt_call = service.subscribe_to_runtime_info()?;
            let mut oobe_call = service.subscribe_to_oobe_actions()?;
            let mut set_call = service.subscribe_to_settings_changes()?;

            let mut resume_deadline: Option<tokio::time::Instant> = None;
            let mut oobe_interval = tokio::time::interval_at(
                tokio::time::Instant::now() + OOBE_POLL_INTERVAL,
                OOBE_POLL_INTERVAL,
            );
            oobe_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            let mut media_interval = tokio::time::interval(MEDIA_SNAPSHOT_INTERVAL);
            media_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            let mut bluez_interval = tokio::time::interval(BLUEZ_BATTERY_REFRESH);
            bluez_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                let mut rt_stream = rt_call.stream();
                let mut oobe_stream = oobe_call.stream();
                let mut set_stream = set_call.stream();

                let resume_sleep = async {
                    if let Some(deadline) = resume_deadline {
                        tokio::time::sleep_until(deadline).await;
                    } else {
                        futures::future::pending::<()>().await;
                    }
                };

                tokio::select! {
                    rt_item = rt_stream.next() => {
                        let info: RuntimeInfo = rt_item
                            .ok_or_else(|| anyhow::anyhow!("runtime stream ended"))??;
                        self.apply_runtime_info(
                            &info,
                            &mut curr_status,
                            prev_left_in_ear,
                            prev_right_in_ear,
                            &mut resume_deadline,
                            media,
                        );
                        self.update_status(shared_status, curr_status.clone());
                    }
                    oobe_item = oobe_stream.next() => {
                        if let Some(Ok(rsp)) = oobe_item {
                            if self.apply_oobe_action(
                                rsp.action,
                                &mut curr_status,
                                prev_left_in_ear,
                                prev_right_in_ear,
                                &mut resume_deadline,
                                media,
                            ) {
                                self.update_status(shared_status, curr_status.clone());
                            }
                        } else {
                            drop(oobe_call);
                            oobe_call = service.subscribe_to_oobe_actions()?;
                        }
                    }
                    set_item = set_stream.next() => {
                        let rsp: SettingsRsp = set_item
                            .ok_or_else(|| anyhow::anyhow!("settings stream ended"))??;
                        if self.apply_settings_rsp(&rsp, &mut curr_status, &mut resume_deadline, media) {
                            self.update_status(shared_status, curr_status.clone());
                        }
                    }
                    _ = oobe_interval.tick() => {
                        drop(oobe_call);
                        oobe_call = service.subscribe_to_oobe_actions()?;
                    }
                    _ = media_interval.tick() => {
                        if curr_status.settings.ohd {
                            let any_in_ear = (curr_status.battery.left.in_ear && !curr_status.battery.left.in_case)
                                || (curr_status.battery.right.in_ear && !curr_status.battery.right.in_case);
                            if any_in_ear {
                                media.snapshot_playing();
                            }
                        }
                    }
                    _ = resume_sleep => {
                        resume_deadline = None;
                        if curr_status.settings.ohd {
                            let any_in_ear = (curr_status.battery.left.in_ear && !curr_status.battery.left.in_case)
                                || (curr_status.battery.right.in_ear && !curr_status.battery.right.in_case);
                            if any_in_ear {
                                media.on_bud_inserted();
                            }
                        }
                    }
                    _ = bluez_interval.tick() => {
                        if let Some(c) = self.client.get_bluez_battery(mac) {
                            if c != curr_status.battery.combined {
                                curr_status.battery.combined = c;
                                curr_status.last_updated = now_secs();
                                self.update_status(shared_status, curr_status.clone());
                            }
                        }
                    }
                    Some(cmd) = ctrl_rx.recv() => {
                        match cmd {
                            ControlMsg::SetAnc { value, reply } => {
                                let res = set_anc_on_service(&mut service, &value).await;
                                if let Ok(ref mode_str) = res {
                                    curr_status.noise_mode = mode_str.clone();
                                    curr_status.last_updated = now_secs();
                                    self.update_status(shared_status, curr_status.clone());
                                }
                                let _ = reply.send(res);
                            }
                            ControlMsg::CycleAnc { reply } => {
                                let res = cycle_anc_on_service(&mut service).await;
                                if let Ok(ref mode_str) = res {
                                    curr_status.noise_mode = mode_str.clone();
                                    curr_status.last_updated = now_secs();
                                    self.update_status(shared_status, curr_status.clone());
                                }
                                let _ = reply.send(res);
                            }
                            ControlMsg::SetSetting { key, value, reply } => {
                                let res = set_setting_on_service(&mut service, &key, value).await;
                                if res.is_ok() {
                                    match key.replace('-', "_").as_str() {
                                        "speech_detection" => curr_status.settings.speech_detection = value,
                                        "ohd" => {
                                            curr_status.settings.ohd = value;
                                            if !value {
                                                resume_deadline = None;
                                                media.clear();
                                            }
                                        }
                                        "multipoint" => curr_status.settings.multipoint = value,
                                        "gestures" => curr_status.settings.gestures = value,
                                        "volume_eq" => curr_status.settings.volume_eq = value,
                                        _ => {}
                                    }
                                    curr_status.last_updated = now_secs();
                                    self.update_status(shared_status, curr_status.clone());
                                }
                                let _ = reply.send(res);
                            }
                        }
                    }
                }
            }
        };

        tokio::select! {
            res = rpc_client.run() => {
                res?;
                bail!("RPC client terminated unexpectedly");
            }
            res = task => {
                res
            }
        }
    }

    fn apply_runtime_info(
        &self,
        info: &RuntimeInfo,
        status: &mut PluginStatus,
        prev_left_in_ear: &mut Option<bool>,
        prev_right_in_ear: &mut Option<bool>,
        resume_deadline: &mut Option<tokio::time::Instant>,
        media: &mut MediaController,
    ) {
        let now = now_secs();

        if let Some(ref b) = info.battery_info {
            if let Some(ref l) = b.left {
                status.battery.left.level = l.level;
                status.battery.left.charging = l.state == 2;
            }
            if let Some(ref r) = b.right {
                status.battery.right.level = r.level;
                status.battery.right.charging = r.state == 2;
            }
            if let Some(ref c) = b.case {
                if c.level >= 0 {
                    let chg = c.state == 2;
                    self.client.save_case_cache(c.level, chg);
                    status.battery.case.level = c.level;
                    status.battery.case.charging = chg;
                    status.battery.case.last_known = false;
                    status.battery.case.timestamp = now;
                    status.battery.case.stale = false;
                } else {
                    status.battery.case = self.client.resolve_cached_case(now);
                }
            } else {
                status.battery.case = self.client.resolve_cached_case(now);
            }
        }

        if status.battery.left.level < 0 && status.battery.combined >= 0 {
            status.battery.left.level = status.battery.combined;
        }
        if status.battery.right.level < 0 && status.battery.combined >= 0 {
            status.battery.right.level = status.battery.combined;
        }

        if let Some(ref p) = info.placement {
            status.battery.left.in_case = p.left_bud_in_case;
            status.battery.right.in_case = p.right_bud_in_case;

            if p.left_bud_in_case {
                if *prev_left_in_ear == Some(true) {
                    *prev_left_in_ear = Some(false);
                    *resume_deadline = None;
                    if status.settings.ohd {
                        media.on_bud_removed();
                    }
                }
                status.battery.left.in_ear = false;
            } else {
                status.battery.left.in_ear = prev_left_in_ear.unwrap_or(false);
            }

            if p.right_bud_in_case {
                if *prev_right_in_ear == Some(true) {
                    *prev_right_in_ear = Some(false);
                    *resume_deadline = None;
                    if status.settings.ohd {
                        media.on_bud_removed();
                    }
                }
                status.battery.right.in_ear = false;
            } else {
                status.battery.right.in_ear = prev_right_in_ear.unwrap_or(false);
            }
        }

        status.last_updated = now;
    }

    fn apply_oobe_action(
        &self,
        action_raw: i32,
        status: &mut PluginStatus,
        prev_left_in_ear: &mut Option<bool>,
        prev_right_in_ear: &mut Option<bool>,
        resume_deadline: &mut Option<tokio::time::Instant>,
        media: &mut MediaController,
    ) -> bool {
        let (is_left, new_in_ear) = match OobeAction::try_from(action_raw) {
            Ok(OobeAction::LeftOnHead) => (true, true),
            Ok(OobeAction::LeftOffHead) => (true, false),
            Ok(OobeAction::RightOnHead) => (false, true),
            Ok(OobeAction::RightOffHead) => (false, false),
            _ => return false,
        };

        let (prev_slot, pod) = if is_left {
            (prev_left_in_ear, &mut status.battery.left)
        } else {
            (prev_right_in_ear, &mut status.battery.right)
        };

        let effective_in_ear = new_in_ear && !pod.in_case;
        let old_opt = *prev_slot;
        *prev_slot = Some(effective_in_ear);
        pod.in_ear = effective_in_ear;

        if old_opt == Some(effective_in_ear) {
            return false;
        }

        status.last_updated = now_secs();
        eprintln!(
            "pixel-buds: {} bud in_ear: {:?} -> {}",
            if is_left { "left" } else { "right" },
            old_opt,
            effective_in_ear
        );

        // Trigger auto-pause / auto-resume only on state transitions after initial snapshot
        if let Some(old_in_ear) = old_opt {
            if old_in_ear != effective_in_ear && status.settings.ohd {
                if !effective_in_ear {
                    // Earbud was pulled out of ear -> pause immediately!
                    *resume_deadline = None;
                    media.on_bud_removed();
                } else {
                    // Earbud was placed back in ear -> debounce slightly before auto-resuming
                    *resume_deadline = Some(tokio::time::Instant::now() + RESUME_DEBOUNCE);
                }
            }
        }

        true
    }

    fn apply_settings_rsp(
        &self,
        rsp: &SettingsRsp,
        status: &mut PluginStatus,
        resume_deadline: &mut Option<tokio::time::Instant>,
        media: &mut MediaController,
    ) -> bool {
        let Some(settings_rsp::ValueOneof::Value(ref val)) = rsp.value_oneof else {
            return false;
        };
        let Some(ref oneof) = val.value_oneof else {
            return false;
        };

        let setting_val = SettingValue::from(oneof.clone());
        match setting_val {
            SettingValue::CurrentAncrState(anc) => {
                status.noise_mode = anc_to_str(anc);
            }
            SettingValue::SpeechDetection(v) => {
                status.settings.speech_detection = v;
            }
            SettingValue::OhdEnable(v) => {
                status.settings.ohd = v;
                if !v {
                    *resume_deadline = None;
                    media.clear();
                }
            }
            SettingValue::MultipointEnable(v) => {
                status.settings.multipoint = v;
            }
            SettingValue::GestureEnable(v) => {
                status.settings.gestures = v;
            }
            SettingValue::VolumeEqEnable(v) => {
                status.settings.volume_eq = v;
            }
            _ => return false,
        }

        status.last_updated = now_secs();
        true
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn anc_to_str(anc: AncState) -> String {
    match anc {
        AncState::Active => "active".to_string(),
        AncState::Aware => "aware".to_string(),
        AncState::Off => "off".to_string(),
        AncState::Adaptive => "adaptive".to_string(),
        AncState::Unknown(_) => "active".to_string(),
    }
}

async fn set_anc_on_service(service: &mut MaestroService, mode: &str) -> Result<String> {
    let mode_lower = mode.trim().to_lowercase();
    let anc = match mode_lower.as_str() {
        "active" => AncState::Active,
        "aware" => AncState::Aware,
        "off" => AncState::Off,
        "adaptive" => AncState::Adaptive,
        _ => bail!("Invalid ANC mode: {}", mode),
    };
    service
        .write_setting(SettingValue::CurrentAncrState(anc))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to set ANC: {:?}", e))?;
    Ok(mode_lower)
}

async fn cycle_anc_on_service(service: &mut MaestroService) -> Result<String> {
    let enabled = service
        .read_setting(settings::id::AncrGestureLoop)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read ANC loop: {:?}", e))?;
    let state = service
        .read_setting(settings::id::CurrentAncrState)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read ANC state: {:?}", e))?;

    let states = [
        (AncState::Active, enabled.active),
        (AncState::Off, enabled.off),
        (AncState::Aware, enabled.aware),
        (AncState::Adaptive, enabled.adaptive),
    ];

    let index = states.iter().position(|(s, _)| *s == state).unwrap_or(0);
    for offs in 1..states.len() {
        let next = (index + offs) % states.len();
        let (next_state, is_enabled) = states[next];
        if is_enabled {
            service
                .write_setting(SettingValue::CurrentAncrState(next_state))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to write ANC state: {:?}", e))?;
            return Ok(anc_to_str(next_state));
        }
    }

    Ok(anc_to_str(state))
}

async fn set_setting_on_service(service: &mut MaestroService, key: &str, val: bool) -> Result<()> {
    let setting_val = match key.replace('-', "_").as_str() {
        "speech_detection" => SettingValue::SpeechDetection(val),
        "ohd" => SettingValue::OhdEnable(val),
        "multipoint" => SettingValue::MultipointEnable(val),
        "gestures" => SettingValue::GestureEnable(val),
        "volume_eq" => SettingValue::VolumeEqEnable(val),
        _ => bail!("Unknown setting: {}", key),
    };
    service
        .write_setting(setting_val)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to set {}: {:?}", key, e))?;
    Ok(())
}

async fn find_connected_maestro_device(adapter: &Adapter) -> Option<(Device, String, String)> {
    let addrs = adapter.device_addresses().await.ok()?;
    for addr in addrs {
        let Ok(dev) = adapter.device(addr) else {
            continue;
        };
        if !dev.is_connected().await.unwrap_or(false) {
            continue;
        }
        let class = dev.class().await.ok().flatten().unwrap_or(0);
        let uuids = dev.uuids().await.ok().flatten().unwrap_or_default();
        let alias = dev
            .alias()
            .await
            .unwrap_or_else(|_| "Pixel Buds Pro 2".to_string());

        let is_pbp = class == PIXEL_BUDS_CLASS
            || class == PIXEL_BUDS2_CLASS
            || uuids.contains(&maestro::UUID)
            || alias.to_lowercase().contains("pixel buds");

        if is_pbp && uuids.contains(&maestro::UUID) {
            return Some((dev, addr.to_string(), alias));
        }
    }
    None
}

async fn connect_maestro_rfcomm(session: &Session, dev: &Device) -> Result<Stream> {
    let maestro_profile = Profile {
        uuid: maestro::UUID,
        role: Some(Role::Client),
        require_authentication: Some(false),
        require_authorization: Some(false),
        auto_connect: Some(false),
        ..Default::default()
    };

    let mut handle = session.register_profile(maestro_profile).await?;
    let stream = tokio::try_join!(
        try_connect_profile(dev),
        handle_requests_for_profile(&mut handle, dev.address()),
    )?
    .1;

    Ok(stream)
}

async fn try_connect_profile(dev: &Device) -> Result<()> {
    const RETRY_TIMEOUT: Duration = Duration::from_millis(800);
    const MAX_TRIES: u32 = 3;

    let mut i = 0;
    while let Err(err) = dev.connect_profile(&maestro::UUID).await {
        if i >= MAX_TRIES {
            return Err(err.into());
        }
        i += 1;
        tokio::time::sleep(RETRY_TIMEOUT).await;
    }

    Ok(())
}

async fn handle_requests_for_profile(
    handle: &mut ProfileHandle,
    address: Address,
) -> Result<Stream> {
    while let Some(req) = handle.next().await {
        if req.device() == address {
            return Ok(req.accept()?);
        } else {
            req.reject(ReqError::Rejected);
        }
    }

    bail!("Profile terminated without connection request")
}
