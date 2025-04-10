use async_std::task::sleep;
use std::time::Duration;

use chrono::Local;
use gio::glib::clone;
use gtk4::glib::Object;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{Accessible, Buildable, ConstraintTarget, Orientable, Widget, glib};

// Object holding the state
#[derive(Default)]
pub struct ClockImpl {}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for ClockImpl {
    const NAME: &'static str = "TwBarClock";
    type Type = Clock;
    type ParentType = gtk4::Box;
}

// Trait shared by all GObjects
impl ObjectImpl for ClockImpl {
    fn constructed(&self) {
        self.parent_constructed();

        let label = gtk4::Label::new(Some(""));
        self.obj().append(&label);

        self.obj().add_css_class("clock");
        self.obj().set_spacing(0);

        glib::spawn_future_local(clone!(
            #[weak]
            label,
            async move {
                loop {
                    let now = Local::now();
                    label.set_text(&format!("{}", now.format("%b %e %Y %l:%M %p")));
                    sleep(Duration::from_secs(10)).await;
                }
            }
        ));
    }
}

// Trait shared by all widgets
impl WidgetImpl for ClockImpl {}

// Trait shared by all boxes
impl BoxImpl for ClockImpl {}

// Self encapsulated label that shows the time
glib::wrapper! {
    pub struct Clock(ObjectSubclass<ClockImpl>)
        @extends gtk4::Box, Widget,
        @implements Accessible, Buildable, ConstraintTarget, Orientable;
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock {
    pub fn new() -> Self {
        Object::builder().build()
    }
}
