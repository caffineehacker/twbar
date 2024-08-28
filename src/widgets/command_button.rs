use async_std::process::Command;
use std::borrow::Borrow;
use std::cell::OnceCell;

use gio::prelude::*;
use gtk4::glib::{Object, Properties};
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{
    glib, Accessible, Align, Buildable, ConstraintTarget, GestureClick, Label, Orientable, Widget,
};

#[derive(glib::Boxed, Default, Clone, Debug)]
#[boxed_type(name = "ButtonCommandType")]
pub struct ButtonCommand {
    pub command: String,
    pub args: Vec<String>,
    pub allow_failure: bool,
}

#[derive(glib::Boxed, Default, Clone, Debug)]
#[boxed_type(name = "CommandsType")]
pub struct Commands {
    commands: Vec<ButtonCommand>,
}

// Object holding the state
#[derive(Properties, Default)]
#[properties(wrapper_type = CommandButton)]
pub struct CommandButtonImpl {
    #[property(get, construct_only)]
    commands: OnceCell<Commands>,
    #[property(construct_only)]
    label: OnceCell<String>,
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for CommandButtonImpl {
    const NAME: &'static str = "TwBarCommandButton";
    type Type = CommandButton;
    type ParentType = gtk4::Box;
}

// Trait shared by all GObjects
#[glib::derived_properties]
impl ObjectImpl for CommandButtonImpl {
    fn constructed(&self) {
        self.parent_constructed();

        let event_controller = GestureClick::new();

        let label = Label::new(self.label.get().map(|l| l.as_str()));
        // The glyph is really 2 chars wide when using a glyph
        label.set_width_chars(2);
        label.set_halign(Align::Center);
        self.obj().append(&label);
        self.obj().set_halign(Align::Center);

        let commands = self.obj().commands().borrow().clone();
        event_controller.connect_released(move |_box, _, _, _| {
            let commands = commands.clone();
            glib::spawn_future_local(async move {
                for command in commands.commands.iter() {
                    let output = Command::new(&command.command)
                        .args(&command.args)
                        .output()
                        .await
                        .unwrap();
                    if !output.status.success() && !command.allow_failure {
                        log::error!("Command {:?} failed: {}", command, output.status);
                        log::error!(
                            "Command {:?}: Stdout: {}",
                            command,
                            String::from_utf8(output.stdout)
                                .unwrap_or("error converting to UTF8".to_owned())
                        );
                        log::error!(
                            "Command {:?}: Stderr: {}",
                            command,
                            String::from_utf8(output.stderr)
                                .unwrap_or("error converting to UTF8".to_owned())
                        );
                    } else {
                        log::trace!(
                            "Command {:?}: Stdout: {}",
                            command,
                            String::from_utf8(output.stdout)
                                .unwrap_or("error converting to UTF8".to_owned())
                        );
                        log::trace!(
                            "Command {:?}: Stderr: {}",
                            command,
                            String::from_utf8(output.stderr)
                                .unwrap_or("error converting to UTF8".to_owned())
                        );
                    }
                }
            });
        });

        self.obj().add_controller(event_controller);
    }
}

// Trait shared by all widgets
impl WidgetImpl for CommandButtonImpl {}

impl BoxImpl for CommandButtonImpl {}

glib::wrapper! {
    /// Self encapsulated button that triggers the appropriate Command on click
    pub struct CommandButton(ObjectSubclass<CommandButtonImpl>)
        @extends gtk4::Box, Widget,
        @implements Accessible, Buildable, ConstraintTarget, Orientable;
}

impl CommandButton {
    pub fn new(label: &str, commands: Vec<ButtonCommand>) -> Self {
        Object::builder()
            .property("label", label)
            .property("commands", Commands { commands: commands })
            .build()
    }
}
