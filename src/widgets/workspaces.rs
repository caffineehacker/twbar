use std::cell::{OnceCell, RefCell};
use std::collections::HashSet;

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
        let workspaces: Vec<&HyprlandWorkspace> = workspaces
            .iter()
            .filter(|w| w.windows > 0 || w.id == *self.selected_workspace_id.borrow())
            .collect();
        //let workspace_ids: HashSet<i32> = workspaces.iter().map(|w| w.id).collect();
        let mut workspace_ids_already_added = HashSet::new();

        let mut child = self.obj().first_child();
        let mut child_index = 0;
        let mut in_order = true;
        while let Some(button) = child {
            if let Some(workspace_button) = button.downcast_ref::<WorkspaceButton>() {
                let button_id = workspace_button.workspace_id();
                if let Some((i, _)) = workspaces
                    .iter()
                    .enumerate()
                    .find(|(_i, w)| w.id == button_id)
                {
                    if i != child_index {
                        in_order = false;
                    }
                    workspace_ids_already_added.insert(button_id);
                } else {
                    println!("Removing workspace {}", workspace_button.workspace_id());
                    self.obj().remove(&button);
                }
            }
            child = button.next_sibling();
            child_index += 1;
        }

        if in_order && child_index == workspaces.len() {
            return;
        }

        println!("Redoing workspace buttons");

        let mut append_mode = false;
        'workspace_loop: for (index, workspace) in workspaces.into_iter().enumerate() {
            if workspace_ids_already_added.contains(&workspace.id) {
                // We may need to move it to the right place
                self.move_button(workspace.id, index);
            } else if append_mode {
                self.obj().append(&WorkspaceButton::new(workspace));
            } else {
                let mut child = self.obj().first_child();
                let mut child_index = 0;
                while let Some(button) = child {
                    if child_index + 1 == index {
                        self.obj()
                            .insert_child_after(&WorkspaceButton::new(workspace), Some(&button));
                        continue 'workspace_loop;
                    }
                    child = button.next_sibling();
                    child_index += 1;
                }

                append_mode = true;
                self.obj().append(&WorkspaceButton::new(workspace));
            }
        }
    }

    fn move_button(&self, id: i32, target_index: usize) {
        let mut child = self.obj().first_child();
        let mut child_index = 0;
        let mut target_sibling = None;

        while let Some(button) = child.clone() {
            if target_index == child_index + 1 {
                target_sibling = child;
            }

            let child_id = button
                .downcast_ref::<WorkspaceButton>()
                .unwrap()
                .workspace_id();
            if child_id == id {
                if child_index == target_index {
                    return;
                }
                if target_sibling.is_some() || target_index == 0 {
                    self.obj()
                        .reorder_child_after(&button, target_sibling.as_ref());
                    return;
                }
                panic!("You should call this method in order so objects are always either in the right place or moved further up!");
            }

            child = button.next_sibling();
            child_index += 1;
        }

        panic!("Could not find target child");
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

        self.obj().set_widget_name("workspaces");
        let monitor_id = *self.monitor_id.get().unwrap();

        glib::spawn_future_local(clone!(
            #[weak(rename_to = me)]
            self,
            async move {
                let hyprland_workspaces = HyprlandWorkspaces::instance().await;
                let mut workspaces_state = hyprland_workspaces.get_workspaces_state_emitter();

                loop {
                    let workspaces = workspaces_state.next().await;
                    let mut workspaces: Vec<HyprlandWorkspace> = workspaces
                        .into_iter()
                        .filter(|w| w.monitor_id == monitor_id)
                        .collect();
                    workspaces.sort_by_key(|w| w.id);

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
