use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use gio::glib::clone;
use gio::prelude::*;
use gtk4::glib::{Object, Properties};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{glib, Accessible, Buildable, ConstraintTarget, Orientable, Widget};
use log::trace;

use crate::hyprland::events::{HyprlandEvent, HyprlandEvents};
use crate::hyprland::windows::{HyprlandWindow, HyprlandWindows};

use super::taskbar_button::TaskbarButton;

// Object holding the state
#[derive(Default, Properties)]
#[properties(wrapper_type = Taskbar)]
pub struct TaskbarImpl {
    #[property(get, construct_only)]
    monitor_id: OnceCell<i32>,
    selected_address: RefCell<String>,
    windows: RefCell<Vec<HyprlandWindow>>,
}

impl TaskbarImpl {
    fn update_buttons(&self) {
        let windows = self.windows.borrow();
        let mut windows: Vec<&HyprlandWindow> = windows
            .iter()
            .filter(|w| w.monitor == *self.monitor_id.get().unwrap())
            .collect();
        windows.sort_by_key(|w| (w.workspace.id, w.at));

        trace!("Windows: {:?}", windows);

        let mut buttons = HashMap::new();
        let mut child = self.obj().first_child();
        while let Some(button) = child.as_ref() {
            let taskbar_button = button.clone().downcast::<TaskbarButton>().unwrap();
            child = button.next_sibling();

            let window_address = taskbar_button.hyprland_window().address;
            if windows.iter().any(|w| w.address == window_address) {
                if window_address != *self.selected_address.borrow() {
                    taskbar_button.remove_css_class("active");
                }
                buttons.insert(window_address, taskbar_button);
            } else {
                self.obj().remove(&taskbar_button);
            }
        }

        let mut last_button = None;
        for w in windows.iter() {
            // The process is to find the button that belongs here, if no button belongs here add one
            let button = buttons.get(&w.address);
            if let Some(button) = button {
                if w.address == *self.selected_address.borrow() {
                    button.add_css_class("active");
                }
                self.obj().reorder_child_after(button, last_button.as_ref());
                last_button = Some(button.clone());
            } else {
                let new_button = TaskbarButton::new(w);
                if w.address == *self.selected_address.borrow() {
                    new_button.add_css_class("active");
                }
                self.obj()
                    .insert_child_after(&new_button, last_button.as_ref());
                last_button = Some(new_button);
            }
        }
    }
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for TaskbarImpl {
    const NAME: &'static str = "TwBarTaskbar";
    type Type = Taskbar;
    type ParentType = gtk4::Box;
}

// Trait shared by all GObjects
#[glib::derived_properties]
impl ObjectImpl for TaskbarImpl {
    fn constructed(&self) {
        self.parent_constructed();

        self.obj().add_css_class("taskbar");

        glib::spawn_future_local(clone!(
            #[weak(rename_to = me)]
            self,
            async move {
                let hyprland_windows = HyprlandWindows::instance().await;
                let mut windows_state = hyprland_windows.get_windows_update_emitter();

                loop {
                    let windows = windows_state.next().await;

                    me.windows.set(windows);
                    me.update_buttons();
                }
            }
        ));

        glib::spawn_future_local(clone!(
            #[weak(rename_to = me)]
            self,
            async move {
                let events = HyprlandEvents::instance().await;
                let mut event_stream = events.get_event_stream().await;

                loop {
                    match event_stream.recv_direct().await {
                        Ok(HyprlandEvent::ActiveWindowV2(address)) => {
                            me.selected_address.set(address);
                            me.update_buttons();
                        }
                        Ok(_) => {}
                        _ => return,
                    };
                }
            }
        ));
    }
}

// Trait shared by all widgets
impl WidgetImpl for TaskbarImpl {}

// Trait shared by all boxes
impl BoxImpl for TaskbarImpl {}

// Self encapsulated button that triggers the appropriate workspace on click
glib::wrapper! {
    pub struct Taskbar(ObjectSubclass<TaskbarImpl>)
        @extends gtk4::Box, Widget,
        @implements Accessible, Buildable, ConstraintTarget, Orientable;
}

impl Taskbar {
    pub fn new(monitor: i32) -> Self {
        Object::builder().property("monitor-id", monitor).build()
    }
}
