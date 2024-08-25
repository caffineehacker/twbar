use std::cell::RefCell;

use gio::glib::clone;
use gio::prelude::*;
use gtk4::glib::{Object, Properties};
use gtk4::subclass::prelude::*;
use gtk4::{glib, Accessible, Actionable, Buildable, Button, ConstraintTarget, Label, Widget};
use gtk4::{prelude::*, Orientation};
use log::trace;

use crate::hyprland::commands::HyprlandCommands;
use crate::hyprland::windows::HyprlandWindow;
use crate::xdg_applications::XdgApplicationsCache;

// Object holding the state
#[derive(Properties, Default)]
#[properties(wrapper_type = TaskbarButton)]
pub struct TaskbarButtonImpl {
    #[property(get, set = Self::set_hyprland_window, construct)]
    hyprland_window: RefCell<HyprlandWindow>,
}

impl TaskbarButtonImpl {
    fn set_hyprland_window(&self, current_window: HyprlandWindow) {
        let previous_window = self.hyprland_window.replace(current_window.clone());
        if !current_window.title.is_empty() {
            self.obj().set_tooltip_text(Some(&current_window.title));
        }

        if previous_window.class != current_window.class
            || previous_window.initial_class != current_window.initial_class
        {
            glib::spawn_future_local(clone!(
                #[weak(rename_to = button)]
                self.obj(),
                async move {
                    let cache = XdgApplicationsCache::get_instance().await;
                    let mut app_info =
                        cache.get_application_by_class(&current_window.initial_class);
                    if app_info.is_none() {
                        app_info = cache.get_application_by_class(&current_window.class);
                    }

                    if app_info.is_some() {
                        let app_info = app_info.unwrap();
                        let icon = app_info.string("Icon");
                        if icon.is_some() {
                            let button_box = gtk4::Box::new(Orientation::Horizontal, 8);
                            let image = gtk4::Image::new();
                            image.set_icon_name(icon.unwrap().as_str().into());
                            button_box.append(&image);
                            let label = Label::new(app_info.name().as_str().into());
                            button_box.append(&label);
                            button.set_child(Some(&button_box));
                        } else {
                            button.set_label(app_info.name().as_str());
                        }
                    } else {
                        button.set_label(&current_window.initial_class);
                    }
                }
            ));
        }
    }
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for TaskbarButtonImpl {
    const NAME: &'static str = "TwBarTaskbarButton";
    type Type = TaskbarButton;
    type ParentType = gtk4::Button;
}

// Trait shared by all GObjects
#[glib::derived_properties]
impl ObjectImpl for TaskbarButtonImpl {
    fn constructed(&self) {
        self.parent_constructed();

        self.obj().set_has_frame(false);
        self.obj().add_css_class("taskbar_button");
        self.obj().set_focusable(false);
    }
}

// Trait shared by all widgets
impl WidgetImpl for TaskbarButtonImpl {}

// Trait shared by all buttons
impl ButtonImpl for TaskbarButtonImpl {
    fn activate(&self) {
        glib::spawn_future_local(clone!(
            #[strong(rename_to = obj)]
            self.obj(),
            async move {
                let address = obj.hyprland_window().address;
                HyprlandCommands::set_active_window(&address).await;
            }
        ));
    }

    fn clicked(&self) {
        trace!("Clicked");

        self.activate();
    }
}

// Self encapsulated button that triggers the appropriate Taskbar on click
glib::wrapper! {
    pub struct TaskbarButton(ObjectSubclass<TaskbarButtonImpl>)
        @extends Button, Widget,
        @implements Accessible, Actionable, Buildable, ConstraintTarget;
}

impl TaskbarButton {
    pub fn new(window: &HyprlandWindow) -> Self {
        Object::builder()
            .property("hyprland-window", window)
            .build()
    }
}
