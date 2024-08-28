use async_std::{
    sync::{Arc, Mutex, Weak},
    task,
};
use gio::glib::clone::Downgrade;
use log::error;
use serde::Deserialize;

use super::{
    commands::HyprlandCommands,
    events::{HyprlandEvents, LatestEventValue, LatestEventValueListener},
};

#[derive(Clone, Default, Deserialize, Debug)]
pub struct HyprlandWorkspace {
    pub id: i32,
    pub name: String,
    pub monitor: String,
    #[serde(rename = "monitorID")]
    pub monitor_id: i32,
    pub windows: i32,
    #[serde(rename = "hasfullscreen")]
    pub has_fullscreen: bool,
    #[serde(rename = "lastwindow")]
    pub last_window: String,
    #[serde(rename = "lastwindowtitle")]
    pub last_window_title: String,
}

pub struct HyprlandWorkspaces {
    workspaces: Arc<LatestEventValue<Vec<HyprlandWorkspace>>>,
    active_workspace_id: Arc<LatestEventValue<i32>>,
}

impl HyprlandWorkspaces {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<HyprlandWorkspaces>> = Mutex::new(Weak::new());

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
        let workspaces = Arc::new(LatestEventValue::new());

        let instance = Arc::new(Self {
            workspaces: workspaces.clone(),
            active_workspace_id: Arc::new(LatestEventValue::new()),
        });

        {
            let instance = instance.downgrade();
            task::spawn(async move {
                let mut events = HyprlandEvents::instance().await.get_event_stream().await;

                instance.upgrade().unwrap().force_refresh().await;

                loop {
                    let event = events.recv().await.unwrap();
                    match event {
                        super::events::HyprlandEvent::MoveWindowV2(_) => {
                            instance.upgrade().unwrap().force_refresh().await
                        }
                        super::events::HyprlandEvent::MonitorAddedV2(_) => {
                            instance.upgrade().unwrap().force_refresh().await
                        }
                        super::events::HyprlandEvent::MonitorRemoved(_) => {
                            instance.upgrade().unwrap().force_refresh().await
                        }
                        super::events::HyprlandEvent::CreateWorkspace(_) => {
                            instance.upgrade().unwrap().force_refresh().await
                        }
                        super::events::HyprlandEvent::CreateWorkspaceV2(_) => {}
                        super::events::HyprlandEvent::MoveWorkspace(_) => {
                            instance.upgrade().unwrap().force_refresh().await
                        }
                        super::events::HyprlandEvent::MoveWorkspaceV2(_) => {}
                        super::events::HyprlandEvent::RenameWorkspace(_) => {
                            instance.upgrade().unwrap().force_refresh().await
                        }
                        super::events::HyprlandEvent::ActiveSpecial(_) => {
                            instance.upgrade().unwrap().force_refresh().await
                        }
                        super::events::HyprlandEvent::DestroyWorkspace(_) => {
                            instance.upgrade().unwrap().force_refresh().await
                        }
                        super::events::HyprlandEvent::DestroyWorkspaceV2(_) => {}
                        super::events::HyprlandEvent::WorkspaceV2(workspace) => {
                            instance
                                .upgrade()
                                .unwrap()
                                .active_workspace_id
                                .update(workspace.id)
                                .await;
                        }
                        super::events::HyprlandEvent::FocusedMon(focused_mon) => {
                            // TODO: This should probably send the command "activeworkspace" to get all of the info about the current workspace including the id. String matching the name is not guaranteed to be correct.
                            let workspace_name = &focused_mon.workspace_name;
                            let instance = instance.upgrade().unwrap();
                            let workspaces = instance.workspaces.current_value.lock().await;
                            let workspace_id = workspaces.1.iter().find_map(|w| {
                                if w.name == *workspace_name {
                                    Some(w.id)
                                } else {
                                    None
                                }
                            });
                            if let Some(workspace_id) = workspace_id {
                                instance.active_workspace_id.update(workspace_id).await;
                            } else {
                                log::warn!("Failed to find workspace for focusedmon event. Event: {:?}\n\nWorkspaces: {:?}", focused_mon, workspaces);
                            }
                        }
                        _ => {}
                    }
                }
            });
        }

        instance
    }

    pub async fn force_refresh(&self) {
        self.workspaces
            .update_fn(|_| {
                task::block_on(async {
                    let workspaces = HyprlandCommands::send_command("j/workspaces").await;
                    let deserialized = serde_json::from_str::<Vec<HyprlandWorkspace>>(&workspaces);
                    if deserialized.is_err() {
                        error!(
                            "Failed to deserialize: {}, {}",
                            workspaces,
                            deserialized.err().unwrap()
                        );
                        return None;
                    }

                    Some(deserialized.unwrap())
                })
            })
            .await;
    }

    pub fn get_workspaces_state_emitter(&self) -> LatestEventValueListener<Vec<HyprlandWorkspace>> {
        LatestEventValueListener::new(self.workspaces.clone())
    }

    pub fn get_active_workspace_id_state(&self) -> LatestEventValueListener<i32> {
        LatestEventValueListener::new(self.active_workspace_id.clone())
    }
}
