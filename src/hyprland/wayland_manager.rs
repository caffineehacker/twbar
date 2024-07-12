use std::{borrow::BorrowMut, collections::{HashMap, HashSet}, ops::DerefMut};

use async_broadcast::{broadcast, InactiveReceiver, Receiver, Sender};
use wayland_client::{backend::ObjectId, event_created_child, globals::{registry_queue_init, GlobalListContents}, protocol::wl_registry, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1}, zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1}};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1}, ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1}};
use async_std::{sync::{Arc, Mutex, RwLock, Weak}, task};

#[derive(Clone)]
pub enum WaylandWindowEvent {
    NewWindowExt(ExtForeignToplevel),
    NewWindowWlr(ZWlrForeignTopLevel),
    UpdatedWindowExt(ExtForeignToplevel),
    UpdatedWindowWlr(ZWlrForeignTopLevel),
    RemovedWindowExt(ExtForeignToplevel),
    RemovedWindowWlr(ZWlrForeignTopLevel),
}

pub struct WaylandManager {
    window_event_receiver: InactiveReceiver<WaylandWindowEvent>,
    wayland_windows_state: Arc<RwLock<WaylandWindowsState>>,
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
        let (mut sender, receiver) = broadcast(1024);
        sender.set_overflow(true);
        sender.set_await_active(false);

        let windows_state = Arc::new(RwLock::new(WaylandWindowsState {
            ext_windows: HashMap::new(),
            zwlr_windows: HashMap::new(),
        }));

        let new_instance = Arc::new(Self {
            window_event_receiver: receiver.deactivate(),
            wayland_windows_state: windows_state.clone(),
        });

        task::spawn(async move {
            let conn = Connection::connect_to_env().unwrap();
            let (globals, mut queue) = registry_queue_init::<WaylandWindowDispatchReceiver>(&conn).unwrap();

            globals
                .bind::<ZwlrForeignToplevelManagerV1, _, _>(&queue.handle(), 1..=3, ()).unwrap();
            globals
                .bind::<ExtForeignToplevelListV1, _, _>(&queue.handle(), 1..=1, ()).unwrap();

            let mut state = WaylandWindowDispatchReceiver {
                event_sender: sender,
                wayland_windows_state: windows_state,
            };

            queue.roundtrip(&mut state).unwrap();

            loop {
                queue.blocking_dispatch(&mut state).unwrap();
            }
        });

        new_instance
    }

    // Returns the currently known window states as a vec of WaylandWindowEvents (just NewWindow* events) and a receiver for future events.
    pub async fn create_window_listener(&self) -> (Vec<WaylandWindowEvent>, Receiver<WaylandWindowEvent>) {
        let windows_state = self.wayland_windows_state.read().await;
        let mut events : Vec<WaylandWindowEvent> = windows_state.ext_windows.iter().map(|(_, w)| WaylandWindowEvent::NewWindowExt(w.clone())).collect();
        events.extend(windows_state.zwlr_windows.iter().map(|(_, w)| WaylandWindowEvent::NewWindowWlr(w.clone())));
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

struct WaylandWindowDispatchReceiver {
    wayland_windows_state: Arc<RwLock<WaylandWindowsState>>,
    event_sender: Sender<WaylandWindowEvent>,
}

struct WaylandWindowsState {
    ext_windows: HashMap<ObjectId, ExtForeignToplevel>,
    zwlr_windows: HashMap<ObjectId, ZWlrForeignTopLevel>,
}

// impl Dispatch<wl_registry::WlRegistry, ()> for WlAppData {
//     fn event(
//         _state: &mut Self,
//         registry: &wl_registry::WlRegistry,
//         event: wl_registry::Event,
//         _: &(),
//         _: &Connection,
//         qh: &QueueHandle<WlAppData>,
//     ) {
//         // When receiving events from the wl_registry, we are only interested in the
//         // `global` event, which signals a new available global.
//         // When receiving this event, we just print its characteristics in this example.
//         if let wl_registry::Event::Global { name, interface, version } = event {
//             match &interface[..] {
//                 "zwlr_foreign_toplevel_manager_v1" => {
//                     println!("Binding");
//                     registry.bind::<zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1, _, _>(name, version, qh, ());
//                 },
//                 _ => { println!("[{}] {} (v{})", name, interface, version); }
//             }
//         }
//     }
// }

impl Dispatch<ExtForeignToplevelListV1, ()> for WaylandWindowDispatchReceiver {
    fn event(
        _state: &mut Self,
        _proxy: &ExtForeignToplevelListV1,
        event: <ExtForeignToplevelListV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel: _ } => {},
                // task::block_on( async { 
                //     state.wayland_windows_state.write().await.ext_windows.insert(toplevel.id(), ExtForeignToplevel::new(toplevel.id()));
                // }),
            ext_foreign_toplevel_list_v1::Event::Finished => {},
            _ => todo!(),
        }
    }

    event_created_child!(WaylandWindowDispatchReceiver, ExtForeignToplevelListV1, [
        _ => ( ExtForeignToplevelHandleV1, ())
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for WaylandWindowDispatchReceiver {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: <ExtForeignToplevelHandleV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // Window, is_new, is_deleted
        static WINDOW_STATE: Mutex<(Option<ExtForeignToplevel>, bool, bool)> = Mutex::new((None, false, false));
        println!("Ext window event received");

        task::block_on(async {
            let mut window_state = WINDOW_STATE.lock().await;
            let (window_opt, is_new, is_deleted) = window_state.deref_mut();

            if window_opt.is_none() {
                *window_opt = state.wayland_windows_state.read().await.ext_windows.get(&handle.id()).cloned();
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
                ext_foreign_toplevel_handle_v1::Event::Identifier { identifier } => window.identifier = identifier,
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
                let mut writer = state.wayland_windows_state.write().await;
                state.event_sender.broadcast_direct(event).await.unwrap_or_default();
                writer.ext_windows.insert(handle.id(), window.clone());
                *window_opt = None;
            }

        });
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WaylandWindowDispatchReceiver {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: <ZwlrForeignToplevelManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel: _ } => { },
            zwlr_foreign_toplevel_manager_v1::Event::Finished => println!("Top level manager finished"),
            _ => todo!(),
        }
    }

    event_created_child!(WaylandWindowDispatchReceiver, ZwlrForeignToplevelManagerV1, [
        _ => ( ZwlrForeignToplevelHandleV1, ())
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for WaylandWindowDispatchReceiver {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: <ZwlrForeignToplevelHandleV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // Window, is_new, is_deleted
        static WINDOW_STATE: Mutex<(Option<ZWlrForeignTopLevel>, bool, bool)> = Mutex::new((None, false, false));

        task::block_on(async {
            let mut window_state = WINDOW_STATE.lock().await;
            let (window_opt, is_new, is_deleted) = window_state.deref_mut();

            if window_opt.is_none() {
                *window_opt = state.wayland_windows_state.read().await.zwlr_windows.get(&handle.id()).cloned();
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
                zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { output } => { window.output.insert(output.id()); },
                zwlr_foreign_toplevel_handle_v1::Event::OutputLeave { output } => { window.output.remove(&output.id()); },
                zwlr_foreign_toplevel_handle_v1::Event::Parent { parent } => window.parent_id = parent.map(|p| p.id()),
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
                        let mut writer = state.wayland_windows_state.write().await;
                        state.event_sender.broadcast_direct(event).await.unwrap_or_default();
                        writer.zwlr_windows.insert(handle.id(), window.clone());
                        *window_opt = None;
                },
                zwlr_foreign_toplevel_handle_v1::Event::Closed => println!("{0}: Closed", handle.id()),
                _ => todo!(),
            }
        });
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandWindowDispatchReceiver {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _: &GlobalListContents,
        _connection: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}