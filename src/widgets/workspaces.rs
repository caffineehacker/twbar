use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use gio::glib::clone;
use gio::prelude::*;
use gtk4::glib::{Object, Properties};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{glib, Accessible, Buildable, ConstraintTarget, Orientable, Widget};

use crate::hyprland::workspaces::{HyprlandWorkspace, HyprlandWorkspaces};
use crate::widgets::workspace_button::WorkspaceButton;

// Object holding the state
#[derive(Default, Properties)]
#[properties(wrapper_type = Workspaces)]
pub struct WorkspacesImpl {
    #[property(get, construct_only)]
    monitor_id: OnceCell<i32>,
    selected_workspace_id: RefCell<i32>,
    workspaces: RefCell<Vec<HyprlandWorkspace>>,
}

impl WorkspacesImpl {
    fn update_buttons(&self) {
        let workspaces = self.workspaces.borrow();
        let mut workspaces: Vec<&HyprlandWorkspace> = workspaces
            .iter()
            .filter(|w| {
                (w.windows > 0 || w.id == *self.selected_workspace_id.borrow())
                    && w.monitor_id == *self.monitor_id.get().unwrap()
            })
            .collect();
        workspaces.sort_by_key(|w| w.id);

        let mut buttons = HashMap::new();
        let mut child = self.obj().first_child();
        while let Some(button) = child.as_ref() {
            let workspace_button = button.clone().downcast::<WorkspaceButton>().unwrap();
            child = button.next_sibling();

            let workspace_id = workspace_button.workspace_id();
            if workspaces.iter().any(|w| w.id == workspace_id) {
                if workspace_id != *self.selected_workspace_id.borrow() {
                    workspace_button.remove_css_class("active");
                }
                buttons.insert(workspace_id, workspace_button);
            } else {
                self.obj().remove(&workspace_button);
            }
        }

        let mut last_button = None;
        for w in workspaces.iter() {
            // The process is to find the button that belongs here, if no button belongs here add one
            let button = buttons.get(&w.id);
            if let Some(button) = button {
                if w.id == *self.selected_workspace_id.borrow() {
                    button.add_css_class("active");
                }
                self.obj().reorder_child_after(button, last_button.as_ref());
                last_button = Some(button.clone());
            } else {
                let new_button = WorkspaceButton::new(w);
                if w.id == *self.selected_workspace_id.borrow() {
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
impl ObjectSubclass for WorkspacesImpl {
    const NAME: &'static str = "TwBarWorkspaces";
    type Type = Workspaces;
    type ParentType = gtk4::Box;
}

// Trait shared by all GObjects
#[glib::derived_properties]
impl ObjectImpl for WorkspacesImpl {
    fn constructed(&self) {
        self.parent_constructed();

        self.obj().add_css_class("workspaces");
        self.obj().set_spacing(0);

        glib::spawn_future_local(clone!(
            #[weak(rename_to = me)]
            self,
            async move {
                let hyprland_workspaces = HyprlandWorkspaces::instance().await;
                let mut workspaces_state = hyprland_workspaces.get_workspaces_state_emitter();

                loop {
                    let workspaces = workspaces_state.next().await;

                    me.workspaces.set(workspaces);
                    me.update_buttons();
                }
            }
        ));

        glib::spawn_future_local(clone!(
            #[weak(rename_to = me)]
            self,
            async move {
                let hyprland_workspaces = HyprlandWorkspaces::instance().await;

                let mut active_workspace = hyprland_workspaces.get_active_workspace_id_state();

                loop {
                    let active_workspace = active_workspace.next().await;
                    me.selected_workspace_id.set(active_workspace);

                    me.update_buttons();
                }
            }
        ));
    }
}

// Trait shared by all widgets
impl WidgetImpl for WorkspacesImpl {}

// Trait shared by all boxes
impl BoxImpl for WorkspacesImpl {}

// Self encapsulated button that triggers the appropriate workspace on click
glib::wrapper! {
    pub struct Workspaces(ObjectSubclass<WorkspacesImpl>)
        @extends gtk4::Box, Widget,
        @implements Accessible, Buildable, ConstraintTarget, Orientable;
}

impl Workspaces {
    pub fn new(monitor: i32) -> Self {
        Object::builder().property("monitor-id", monitor).build()
    }
}
