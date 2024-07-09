use async_std::{
    io::{self, prelude::BufReadExt, BufReader},
    os::unix::net::UnixStream,
    path::PathBuf,
    stream::StreamExt,
    sync::{Arc, Condvar, Mutex},
    task::{self, JoinHandle},
};
use std::env::var;

#[derive(Clone)]
pub struct ActiveWindow {
    pub title: String,
}

pub struct HyprlandEvents {
    listeners: Arc<HyperlandEventListeners>,
    event_task: JoinHandle<()>,
}

struct EventStreamData<T> {
    current_value: Mutex<(i64, T)>,

    trigger: Condvar,
}

impl<T> EventStreamData<T> {
    fn new(initial_value: T) -> Self {
        Self {
            current_value: Mutex::new((0, initial_value)),
            trigger: Condvar::new(),
        }
    }
}

pub struct EventStream<T: Clone> {
    data: Arc<EventStreamData<T>>,
    last_seen_iteration: i64,
}

impl<T: Clone> EventStream<T> {
    fn new(data: Arc<EventStreamData<T>>) -> Self {
        Self {
            data,
            last_seen_iteration: 0,
        }
    }

    pub async fn next(&mut self) -> T {
        let guard = self
            .data
            .trigger
            .wait_until(
                self.data.current_value.lock().await,
                |(iteration, _data)| *iteration != self.last_seen_iteration,
            )
            .await;

        self.last_seen_iteration = guard.0;

        guard.1.clone()
    }
}

struct HyperlandEventListeners {
    active_window: Arc<EventStreamData<ActiveWindow>>,
}

impl HyprlandEvents {
    pub async fn new() -> Self {
        let listeners = Arc::new(HyperlandEventListeners {
            active_window: Arc::new(EventStreamData::new(ActiveWindow {
                title: "".to_owned(),
            })),
        });
        HyprlandEvents {
            listeners: listeners.clone(),

            event_task: task::spawn(async move {
                let event_stream = Self::create_event_socket().await.unwrap();
                let mut lines = BufReader::new(event_stream).lines();

                while let Some(Ok(line)) = lines.next().await {
                    if line.starts_with("activewindow>>") {
                        let data = line.split_once(">>").unwrap().1;
                        let (_window_class, title) = data.split_once(',').unwrap_or(("", ""));
                        let active_window = ActiveWindow {
                            title: title.to_owned(),
                        };

                        let mut data_lock = listeners.active_window.current_value.lock().await;
                        *data_lock = (data_lock.0 + 1, active_window);
                        listeners.active_window.trigger.notify_all();
                    }
                }
            }),
        }
    }

    async fn create_event_socket() -> Result<UnixStream, io::Error> {
        let instance = match var("HYPRLAND_INSTANCE_SIGNATURE") {
            Ok(var) => var,
            Err(_) => {
                panic!("Could not find HYPRLAND_INSTANCE_SIGNATURE variable, is Hyprland running?")
            }
        };

        let xdg_runtime_dir = match var("XDG_RUNTIME_DIR") {
            Ok(var) => var,
            Err(_) => {
                panic!("Could not find XDG_RUNTIME_DIR variable")
            }
        };

        let path = PathBuf::from(xdg_runtime_dir)
            .join("hypr")
            .join(instance)
            .join(".socket2.sock");
        if !path.exists().await {
            panic!("Could not find Hyprland socket path");
        }

        UnixStream::connect(path).await
    }

    pub async fn get_active_window_emitter(&self) -> EventStream<ActiveWindow> {
        EventStream::new(self.listeners.active_window.clone())
    }
}
