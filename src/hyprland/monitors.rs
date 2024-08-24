use async_std::{
    sync::{Arc, Mutex, Weak},
    task,
};
use log::error;
use serde::Deserialize;

use super::{
    commands::HyprlandCommands,
    events::{HyprlandEvent, HyprlandEvents, LatestEventValue, LatestEventValueListener},
};

#[derive(Clone, Default, Deserialize)]
pub struct HyprlandMonitor {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub make: String,
    pub model: String,
    pub serial: String,
    pub width: u32,
    pub height: u32,
    #[serde(rename = "refreshRate")]
    pub refresh_rate: f32,
    pub x: u32,
    pub y: u32,
    #[serde(rename = "activeWorkspace")]
    pub active_workspace: MonitorWorkspace,
    #[serde(rename = "specialWorkspace")]
    pub special_workspace: MonitorWorkspace,
    pub reserved: Vec<u32>,
    pub scale: f32,
    pub transform: u32,
    pub focused: bool,
    #[serde(rename = "dpmsStatus")]
    pub dpms_status: bool,
    pub vrr: bool,
    #[serde(rename = "activelyTearing")]
    pub actively_tearing: bool,
    pub disabled: bool,
    #[serde(rename = "currentFormat")]
    pub current_format: String,
    #[serde(rename = "availableModes")]
    pub available_modes: Vec<String>,
}

#[derive(Clone, Default, Deserialize)]
pub struct MonitorWorkspace {
    pub id: u32,
    pub name: String,
}

pub struct HyprlandMonitors {
    monitors: Arc<LatestEventValue<Vec<HyprlandMonitor>>>,
}

impl HyprlandMonitors {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<HyprlandMonitors>> = Mutex::new(Weak::new());

        let mut mutex_guard = INSTANCE.lock().await;
        match mutex_guard.upgrade() {
            Some(instance) => instance,
            None => {
                let instance = Self::new().await;
                *mutex_guard = Arc::downgrade(&instance);
                instance
            }
        }
    }

    async fn new() -> Arc<Self> {
        let monitors = Arc::new(LatestEventValue::new());

        let instance = Arc::new(Self {
            monitors: monitors.clone(),
        });

        {
            let instance = instance.clone();
            task::spawn(async move {
                let events = HyprlandEvents::instance().await;
                let mut events = events.get_event_stream().await;

                instance.force_refresh().await;

                loop {
                    let event = events.recv().await.unwrap();
                    match event {
                        HyprlandEvent::MonitorAdded(_) => instance.force_refresh().await,
                        HyprlandEvent::MonitorRemoved(_) => instance.force_refresh().await,
                        _ => {}
                    }
                }
            });
        }

        instance
    }

    pub async fn force_refresh(&self) {
        self.monitors
            .update_fn(|_| {
                task::block_on(async {
                    let monitors = HyprlandCommands::send_command("j/monitors").await;
                    let deserialized = serde_json::from_str::<Vec<HyprlandMonitor>>(&monitors);
                    if deserialized.is_err() {
                        error!(
                            "Failed to deserialize: {}, {}",
                            monitors,
                            deserialized.err().unwrap()
                        );
                        return None;
                    }

                    Some(deserialized.unwrap())
                })
            })
            .await;
    }

    pub fn get_monitor_state_emitter(&self) -> LatestEventValueListener<Vec<HyprlandMonitor>> {
        LatestEventValueListener::new(self.monitors.clone())
    }
}
