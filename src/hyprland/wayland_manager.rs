use std::{
    borrow::{Borrow, BorrowMut},
    collections::{HashMap, HashSet},
    ops::DerefMut,
};

use async_broadcast::{broadcast, InactiveReceiver, Receiver, Sender};
use async_std::{
    sync::{Arc, Mutex, RwLock, Weak},
    task,
};
use wayland_client::{
    backend::ObjectId,
    event_created_child,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_output::WlOutput, wl_registry},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols_wlr::{
    foreign_toplevel::v1::client::{
        zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
        zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
    },
    output_management::v1::client::{
        zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
        zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
        zwlr_output_mode_v1::ZwlrOutputModeV1,
    },
};

#[derive(Clone)]
pub enum WaylandWindowEvent {
    NewWindowExt(ExtForeignToplevel),
    NewWindowWlr(ZWlrForeignTopLevel),
    UpdatedWindowExt(ExtForeignToplevel),
    UpdatedWindowWlr(ZWlrForeignTopLevel),
    RemovedWindowExt(ExtForeignToplevel),
    RemovedWindowWlr(ZWlrForeignTopLevel),
}

#[derive(Clone)]
pub enum OutputEvent {
    OutputsUpdated(Vec<Output>),
}

#[derive(Clone, Default)]
pub struct Output {
    pub wl_output_id: u32,
    pub name: String,
    pub description: String,
    pub size: [i32; 2],
    pub mode: String,
    pub enabled: bool,
    pub current_mode: String,
    pub position: [i32; 2],
    pub transform: String,
    pub scale: f64,
    pub make: String,
    pub model: String,
    pub serial_number: String,
    pub adaptive_sync: bool,
}

pub struct WaylandManager {
    window_event_receiver: InactiveReceiver<WaylandWindowEvent>,
    windows_state: Arc<RwLock<WaylandWindowsState>>,
    output_event_receiver: InactiveReceiver<OutputEvent>,
    outputs_state: Arc<RwLock<HashMap<ObjectId, Output>>>,
}

impl WaylandManager {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<WaylandManager>> = Mutex::new(Weak::new());

        let mut mutex_guard = INSTANCE.lock().await;
        match mutex_guard.upgrade() {
            Some(instance) => instance,
            None => {
                let instance = Self::new();
                *mutex_guard = Arc::downgrade(&instance);
                instance
            }
        }
    }

    fn new() -> Arc<Self> {
        let (mut window_event_sender, window_event_receiver) = broadcast(1024);
        window_event_sender.set_overflow(true);
        window_event_sender.set_await_active(false);

        let windows_state = Arc::new(RwLock::new(WaylandWindowsState {
            ext_windows: HashMap::new(),
            zwlr_windows: HashMap::new(),
        }));

        let (mut output_event_sender, output_event_receiver) = broadcast(1024);
        output_event_sender.set_overflow(true);
        output_event_sender.set_await_active(false);

        let outputs_state = Arc::new(RwLock::new(HashMap::new()));

        let new_instance = Arc::new(Self {
            window_event_receiver: window_event_receiver.deactivate(),
            windows_state: windows_state.clone(),
            output_event_receiver: output_event_receiver.deactivate(),
            outputs_state: outputs_state.clone(),
        });

        task::spawn(async move {
            let conn = Connection::connect_to_env().unwrap();
            let display = conn.display();
            let mut queue = conn.new_event_queue();
            let qh = queue.handle();
            display.get_registry(&qh, ());

            let mut state = WaylandDispatchReceiver {
                window_event_sender,
                windows_state,
                output_event_sender,
                outputs_state,
            };

            queue.roundtrip(&mut state).unwrap();

            // let (globals, mut queue) =
            //     registry_queue_init::<WaylandDispatchReceiver>(&conn).unwrap();

            // globals
            //     .bind::<ZwlrForeignToplevelManagerV1, _, _>(&queue.handle(), 1..=3, ())
            //     .unwrap();
            // globals
            //     .bind::<ExtForeignToplevelListV1, _, _>(&queue.handle(), 1..=1, ())
            //     .unwrap();
            // globals
            //     .bind::<ZwlrOutputManagerV1, _, _>(&queue.handle(), 1..=4, ())
            //     .unwrap();

            // let result = queue.roundtrip(&mut state);
            // if result.is_err() {
            //     panic!("Failed to roundtrip: {}", result.err().unwrap());
            // }

            loop {
                queue.blocking_dispatch(&mut state).unwrap();
            }
        });

        new_instance
    }

    // Returns the currently known window states as a vec of WaylandWindowEvents (just NewWindow* events) and a receiver for future events.
    pub async fn create_window_listener(
        &self,
    ) -> (Vec<WaylandWindowEvent>, Receiver<WaylandWindowEvent>) {
        let windows_state = self.windows_state.read().await;
        let mut events: Vec<WaylandWindowEvent> = windows_state
            .ext_windows
            .values()
            .map(|w| WaylandWindowEvent::NewWindowExt(w.clone()))
            .collect();
        events.extend(
            windows_state
                .zwlr_windows
                .values()
                .map(|w| WaylandWindowEvent::NewWindowWlr(w.clone())),
        );
        (events, self.window_event_receiver.activate_cloned())
    }
}

// Represents a window from the ExtForeignTopLevel protocol
#[derive(Clone)]
pub struct ExtForeignToplevel {
    // Raw wayland proxy ID
    pub id: ObjectId,
    pub title: String,
    pub app_id: String,
    // Identifier from the protocol message
    pub identifier: String,
}

impl ExtForeignToplevel {
    pub fn new(id: ObjectId) -> Self {
        Self {
            id,
            title: String::default(),
            app_id: String::default(),
            identifier: String::default(),
        }
    }
}

// Represents a window from the ZWlrForeignTopLevel protocol
#[derive(Clone)]
pub struct ZWlrForeignTopLevel {
    // Raw wayland proxy ID
    id: ObjectId,
    title: String,
    app_id: String,
    output: HashSet<ObjectId>,
    state: Vec<u8>,
    parent_id: Option<ObjectId>,
}

impl ZWlrForeignTopLevel {
    pub fn new(id: ObjectId) -> Self {
        Self {
            id,
            title: String::default(),
            app_id: String::default(),
            output: HashSet::new(),
            state: Vec::new(),
            parent_id: None,
        }
    }
}

struct WaylandDispatchReceiver {
    windows_state: Arc<RwLock<WaylandWindowsState>>,
    window_event_sender: Sender<WaylandWindowEvent>,
    outputs_state: Arc<RwLock<HashMap<ObjectId, Output>>>,
    output_event_sender: Sender<OutputEvent>,
}

struct WaylandWindowsState {
    ext_windows: HashMap<ObjectId, ExtForeignToplevel>,
    zwlr_windows: HashMap<ObjectId, ZWlrForeignTopLevel>,
}

impl Dispatch<ExtForeignToplevelListV1, ()> for WaylandDispatchReceiver {
    fn event(
        _state: &mut Self,
        _proxy: &ExtForeignToplevelListV1,
        event: <ExtForeignToplevelListV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel: _ } => {}
            // task::block_on( async {
            //     state.wayland_windows_state.write().await.ext_windows.insert(toplevel.id(), ExtForeignToplevel::new(toplevel.id()));
            // }),
            ext_foreign_toplevel_list_v1::Event::Finished => {}
            _ => todo!(),
        }
    }

    event_created_child!(WaylandDispatchReceiver, ExtForeignToplevelListV1, [
        _ => ( ExtForeignToplevelHandleV1, ())
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for WaylandDispatchReceiver {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: <ExtForeignToplevelHandleV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // Window, is_new, is_deleted
        static WINDOW_STATE: Mutex<(Option<ExtForeignToplevel>, bool, bool)> =
            Mutex::new((None, false, false));
        println!("Ext window event received");

        task::block_on(async {
            let mut window_state = WINDOW_STATE.lock().await;
            let (window_opt, is_new, is_deleted) = window_state.deref_mut();

            if window_opt.is_none() {
                *window_opt = state
                    .windows_state
                    .read()
                    .await
                    .ext_windows
                    .get(&handle.id())
                    .cloned();
                *is_new = window_opt.is_none();
                *is_deleted = false;
                if window_opt.is_none() {
                    *window_opt = Some(ExtForeignToplevel::new(handle.id()));
                }
            }

            let window = window_opt.as_mut().unwrap();
            let mut is_done = false;

            match event {
                ext_foreign_toplevel_handle_v1::Event::Title { title } => window.title = title,
                ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => window.app_id = app_id,
                ext_foreign_toplevel_handle_v1::Event::Identifier { identifier } => {
                    window.identifier = identifier
                }
                ext_foreign_toplevel_handle_v1::Event::Closed => *is_deleted = true,
                ext_foreign_toplevel_handle_v1::Event::Done => is_done = true,
                _ => todo!(),
            }

            // Hyprland has a bug where it will not send done except for new windows.
            if !*is_new || is_done {
                let event = {
                    if *is_new {
                        WaylandWindowEvent::NewWindowExt(window.clone())
                    } else if *is_deleted {
                        WaylandWindowEvent::RemovedWindowExt(window.clone())
                    } else {
                        WaylandWindowEvent::UpdatedWindowExt(window.clone())
                    }
                };
                // To avoid race conditions, we lock the writer before sending to the broadcaster.
                // When creating a new receiver for the broadcast, we lock the state for reading, copy everything out and create a new receiver, then unlock it.
                // In combination this effectively means that we can ensure no messages are missed.
                let mut writer = state.windows_state.write().await;
                state
                    .window_event_sender
                    .broadcast_direct(event)
                    .await
                    .unwrap_or_default();
                writer.ext_windows.insert(handle.id(), window.clone());
                *window_opt = None;
            }
        });
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WaylandDispatchReceiver {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: <ZwlrForeignToplevelManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel: _ } => {}
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                println!("Top level manager finished")
            }
            _ => todo!(),
        }
    }

    event_created_child!(WaylandDispatchReceiver, ZwlrForeignToplevelManagerV1, [
        _ => ( ZwlrForeignToplevelHandleV1, ())
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for WaylandDispatchReceiver {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: <ZwlrForeignToplevelHandleV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // Window, is_new, is_deleted
        static WINDOW_STATE: Mutex<(Option<ZWlrForeignTopLevel>, bool, bool)> =
            Mutex::new((None, false, false));

        task::block_on(async {
            let mut window_state = WINDOW_STATE.lock().await;
            let (window_opt, is_new, is_deleted) = window_state.deref_mut();

            if window_opt.is_none() {
                *window_opt = state
                    .windows_state
                    .read()
                    .await
                    .zwlr_windows
                    .get(&handle.id())
                    .cloned();
                *is_new = window_opt.is_none();
                *is_deleted = false;
                if window_opt.is_none() {
                    *window_opt = Some(ZWlrForeignTopLevel::new(handle.id()));
                }
            }

            let window = window_opt.as_mut().unwrap();

            match event {
                zwlr_foreign_toplevel_handle_v1::Event::Title { title } => window.title = title,
                zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => window.app_id = app_id,
                zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { output } => {
                    window.output.insert(output.id());
                }
                zwlr_foreign_toplevel_handle_v1::Event::OutputLeave { output } => {
                    window.output.remove(&output.id());
                }
                zwlr_foreign_toplevel_handle_v1::Event::Parent { parent } => {
                    window.parent_id = parent.map(|p| p.id())
                }
                zwlr_foreign_toplevel_handle_v1::Event::State { state } => window.state = state,
                zwlr_foreign_toplevel_handle_v1::Event::Done => {
                    let event = {
                        if *is_new {
                            WaylandWindowEvent::NewWindowWlr(window.clone())
                        } else if *is_deleted {
                            WaylandWindowEvent::RemovedWindowWlr(window.clone())
                        } else {
                            WaylandWindowEvent::UpdatedWindowWlr(window.clone())
                        }
                    };
                    // To avoid race conditions, we lock the writer before sending to the broadcaster.
                    // When creating a new receiver for the broadcast, we lock the state for reading, copy everything out and create a new receiver, then unlock it.
                    // In combination this effectively means that we can ensure no messages are missed.
                    let mut writer = state.windows_state.write().await;
                    state
                        .window_event_sender
                        .broadcast_direct(event)
                        .await
                        .unwrap_or_default();
                    writer.zwlr_windows.insert(handle.id(), window.clone());
                    *window_opt = None;
                }
                zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                    println!("{0}: Closed", handle.id())
                }
                _ => todo!(),
            }
        });
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandDispatchReceiver {
    fn event(
        _state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                println!("New global: {0}: {1} version={2}", name, interface, version);
                match interface.as_str() {
                    "zwlr_foreign_toplevel_manager_v1" => {
                        registry.bind::<ZwlrForeignToplevelManagerV1, _, _>(name, version, qh, ());
                    }
                    "ext_foreign_toplevel_list_v1" => {
                        registry.bind::<ExtForeignToplevelListV1, _, _>(name, version, qh, ());
                    }
                    "zwlr_output_manager_v1" => {
                        registry.bind::<ZwlrOutputManagerV1, _, _>(name, version, qh, ());
                    }
                    "wl_output" => {
                        registry.bind::<WlOutput, _, _>(name, version, qh, ());
                    }
                    &_ => {}
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                println!("Global removed {0}", name);
            }
            _ => todo!(),
        }
    }
}

impl Dispatch<WlOutput, ()> for WaylandDispatchReceiver {
    fn event(
        _state: &mut Self,
        _proxy: &WlOutput,
        _event: <WlOutput as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        println!("WLOuput ID: {:?}, event: {:?}", _proxy, _event);
    }
}

impl Dispatch<ZwlrOutputManagerV1, ()> for WaylandDispatchReceiver {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrOutputManagerV1,
        event: <ZwlrOutputManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        task::block_on(async {
            match event {
                zwlr_output_manager_v1::Event::Done { serial: _ } => {
                    state
                        .output_event_sender
                        .broadcast_direct(OutputEvent::OutputsUpdated(
                            state.outputs_state.read().await.values().cloned().collect(),
                        ))
                        .await
                        .ok();
                }
                _ => {}
            }
        });
    }

    event_created_child!(WaylandDispatchReceiver, ZwlrOutputManagerV1, [
      _ => ( ZwlrOutputHeadV1, ())
    ]);
}

impl Dispatch<ZwlrOutputHeadV1, ()> for WaylandDispatchReceiver {
    fn event(
        state: &mut Self,
        proxy: &ZwlrOutputHeadV1,
        event: <ZwlrOutputHeadV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        task::block_on(async {
            let mut outputs_write = state.outputs_state.write().await;
            let mut output = outputs_write.get_mut(&proxy.id());
            if output.is_none() {
                outputs_write.insert(proxy.id(), Output::default());
                output = outputs_write.get_mut(&proxy.id());
            }
            let output = output.as_mut().unwrap();

            match event {
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::Name { name } => output.name = name,
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::Description { description } => output.description = description,
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::PhysicalSize { width, height } => output.size = [width, height],
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::Mode { mode: _mode } => {},
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::Enabled { enabled } => output.enabled = enabled != 0,
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::CurrentMode { mode: _mode } => {},
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::Position { x, y } => output.position = [x, y],
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::Transform { transform: _transform } => {},
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::Scale { scale } => output.scale = scale,
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::Make { make } => output.make = make,
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::Model { model } => output.model = model,
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::SerialNumber { serial_number } => output.serial_number = serial_number,
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::AdaptiveSync { state } => output.adaptive_sync = state.into_result().map(|s| s == zwlr_output_head_v1::AdaptiveSyncState::Enabled).unwrap_or(false),
                wayland_protocols_wlr::output_management::v1::client::zwlr_output_head_v1::Event::Finished => {
                    let mut outputs_write = state.outputs_state.write().await;
                    outputs_write.remove(&proxy.id());
                    proxy.release();
                },
                _ => todo!(),
            }
        });
    }

    event_created_child!(WaylandDispatchReceiver, ZwlrOutputHeadV1, [
      _ => ( ZwlrOutputModeV1, ())
    ]);
}

impl Dispatch<ZwlrOutputModeV1, ()> for WaylandDispatchReceiver {
    fn event(
        state: &mut Self,
        proxy: &ZwlrOutputModeV1,
        event: <ZwlrOutputModeV1 as Proxy>::Event,
        data: &(),
        conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
    }
}
