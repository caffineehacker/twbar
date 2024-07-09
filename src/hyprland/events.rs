use async_std::{
    io::{self, prelude::BufReadExt, BufReader},
    os::unix::net::UnixStream,
    path::PathBuf,
    stream::StreamExt,
    sync::{Arc, Condvar, Mutex},
    task::{self, JoinHandle},
};
use std::env::var;

pub trait EventData: Clone {
    fn parse(data: &str) -> Option<Self> where Self: Sized;
}

#[derive(Clone)]
pub struct ActiveWindow {
    pub title: String,
}

impl EventData for ActiveWindow {
    fn parse(data: &str) -> Option<Self> {
        let (_class_name, title) = data.split_once(",")?;
        
        Some(Self {
            title: title.to_owned()
        })
    }
}

pub struct HyprlandEvents {
    listeners: Arc<HyperlandEventListeners>,
    event_task: JoinHandle<()>,
}

struct EventStreamData<T: EventData> {
    current_value: Mutex<(i64, T)>,

    trigger: Condvar,
}

impl<T: EventData> EventStreamData<T> {
    fn new(initial_value: T) -> Self {
        Self {
            current_value: Mutex::new((0, initial_value)),
            trigger: Condvar::new(),
        }
    }

    async fn update(&self, new_data: &str) {
        if let Some(new_value) = T::parse(new_data) {
            let mut data_lock = self.current_value.lock().await;
            *data_lock = (data_lock.0 + 1, new_value);
            self.trigger.notify_all();
        }
    }
}

pub struct EventStream<T: EventData> {
    data: Arc<EventStreamData<T>>,
    last_seen_iteration: i64,
}

impl<T: EventData> EventStream<T> {
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
                    if let Some((command, data)) = line.split_once(">>") {
                        match command {
                          "activewindow" => listeners.active_window.update(data).await,
                          _ => ()
                        }
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
