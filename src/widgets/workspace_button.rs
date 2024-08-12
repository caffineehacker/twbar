use std::cell::RefCell;

use gio::prelude::*;
use gtk4::glib::{Object, Properties};
use gtk4::subclass::prelude::*;
use gtk4::{glib, Accessible, Actionable, Buildable, Button, ConstraintTarget, Widget};
use gtk4::{prelude::*, Orientation};

use crate::hyprland::commands::HyprlandCommands;
use crate::hyprland::workspaces::HyprlandWorkspace;

// Object holding the state
#[derive(Properties, Default)]
#[properties(wrapper_type = WorkspaceButton)]
pub struct WorkspaceButtonImpl {
    #[property(get, construct_only)]
    workspace_id: RefCell<i32>,
    #[property(get, construct_only)]
    workspace_name: RefCell<String>,
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for WorkspaceButtonImpl {
    const NAME: &'static str = "TwBarWorkspaceButton";
    type Type = WorkspaceButton;
    type ParentType = gtk4::Button;
}

// Trait shared by all GObjects
#[glib::derived_properties]
impl ObjectImpl for WorkspaceButtonImpl {
    fn constructed(&self) {
        self.parent_constructed();

        let container = gtk4::Box::new(Orientation::Horizontal, 0);
        let label = gtk4::Label::new(Some(&self.workspace_name.borrow()));
        label.set_halign(gtk4::Align::Center);
        container.append(&label);
        container.set_halign(gtk4::Align::Center);
        self.obj().set_child(Some(&container));
        self.obj().set_has_frame(false);
        self.obj().add_css_class("workspace");
        self.obj().add_css_class("circular");
        self.obj().set_focusable(false);
    }
}

// Trait shared by all widgets
impl WidgetImpl for WorkspaceButtonImpl {}

// Trait shared by all buttons
impl ButtonImpl for WorkspaceButtonImpl {
    fn activate(&self) {
        println!("Activating workspace");

        let workspace_id = self.workspace_id.borrow().clone();
        glib::spawn_future_local(async move {
            HyprlandCommands::set_active_workspace(workspace_id).await;
        });
    }

    fn clicked(&self) {
        println!("Clicked");

        self.activate();
    }
}

// Self encapsulated button that triggers the appropriate workspace on click
glib::wrapper! {
    pub struct WorkspaceButton(ObjectSubclass<WorkspaceButtonImpl>)
        @extends Button, Widget,
        @implements Accessible, Actionable, Buildable, ConstraintTarget;
}

impl WorkspaceButton {
    pub fn new(workspace: &HyprlandWorkspace) -> Self {
        Object::builder()
            .property("workspace-id", workspace.id)
            .property("workspace-name", workspace.name.clone())
            .build()
    }
}
