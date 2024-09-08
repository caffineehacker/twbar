use std::collections::HashMap;
use std::io::Error;
use std::time::Duration;

use async_std::task::sleep;
use async_std::{fs::File, io::ReadExt};

use gio::glib::clone;
use gio::prelude::*;
use gtk4::glib::Object;
use gtk4::subclass::prelude::*;
use gtk4::{
    glib, Accessible, Buildable, ConstraintTarget, EventControllerMotion, Orientable, Popover,
    Widget,
};
use gtk4::{prelude::*, Label};

async fn read_memory_info() -> Result<HashMap<String, i64>, Error> {
    let mut stat = File::open("/proc/meminfo").await?;
    let mut buf: String = String::default();
    stat.read_to_string(&mut buf).await?;
    Ok(buf
        .lines()
        .map(|line| line.split_once(":").unwrap())
        .map(|(k, v)| {
            (
                k.to_owned(),
                v.trim()
                    .split_ascii_whitespace()
                    .next()
                    .unwrap()
                    .parse::<i64>()
                    .unwrap(),
            )
        })
        .collect::<HashMap<String, i64>>())
}

// Object holding the state
#[derive(Default)]
pub struct RamUsageImpl {}

impl RamUsageImpl {}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for RamUsageImpl {
    const NAME: &'static str = "TwBarRamUsage";
    type Type = RamUsage;
    type ParentType = gtk4::Box;
}

// Trait shared by all GObjects
impl ObjectImpl for RamUsageImpl {
    fn constructed(&self) {
        self.parent_constructed();

        self.obj().add_css_class("ram_usage");
        let label = Label::new(Some(""));
        self.obj().append(&label);

        let label_ref = label.downgrade();

        let popup_label = Label::new(Some(""));
        let popup = Popover::new();
        popup.set_child(Some(&popup_label));
        popup.set_parent(self.obj().upcast_ref::<Widget>());
        popup.set_autohide(false);
        popup.set_focusable(false);
        popup.set_can_focus(false);

        let popup_label_ref = popup_label.downgrade();

        glib::spawn_future_local(async move {
            loop {
                let mem_info = read_memory_info().await;
                match mem_info {
                    Err(e) => {
                        log::error!("Failed to read mem info: {}", e);
                    }
                    Ok(mem_info) => {
                        // MemAvailable is effectively mem free
                        let memfree = mem_info.get("MemAvailable").cloned().unwrap_or(0);
                        let memtotal = mem_info.get("MemTotal").cloned().unwrap_or(0);
                        let memused = memtotal - memfree;
                        let memused_percent = (memused as f64) / (memtotal as f64).max(1.0);

                        match label_ref.upgrade() {
                            Some(label) => label.set_text(&format!(
                                "   {}%",
                                (memused_percent * 100.0).round() as i64
                            )),
                            None => {
                                log::trace!("Label ref upgrade failed");
                                return;
                            }
                        };

                        match popup_label_ref.upgrade() {
                            Some(label) => label.set_text(&format!(
                                "Total: {:.2} GB\nUsed: {:.2} GB",
                                (memtotal as f64) / (1024.0 * 1024.0),
                                (memused as f64) / (1024.0 * 1024.0)
                            )),
                            None => {
                                log::trace!("Popup label upgrade failed");
                                return;
                            }
                        };
                    }
                };
                sleep(Duration::from_secs(1)).await;
            }
        });

        let event_controller = EventControllerMotion::new();
        event_controller.connect_enter(clone!(
            #[weak]
            popup,
            move |_ec, _, _| {
                popup.popup();
            }
        ));
        event_controller.connect_leave(clone!(
            #[weak]
            popup,
            move |_| {
                popup.popdown();
            }
        ));
        self.obj().add_controller(event_controller);
        // Unparent to avoid the warning about a destroyed widget having children.
        self.obj().connect_destroy(move |_| {
            log::trace!("In destroy");
            popup.unparent();
        });
    }
}

// Trait shared by all widgets
impl WidgetImpl for RamUsageImpl {}

// Trait shared by all boxes
impl BoxImpl for RamUsageImpl {}

// Self encapsulated button that triggers the appropriate workspace on click
glib::wrapper! {
    pub struct RamUsage(ObjectSubclass<RamUsageImpl>)
        @extends gtk4::Box, Widget,
        @implements Accessible, Buildable, ConstraintTarget, Orientable;
}

impl RamUsage {
    pub fn new() -> Self {
        Object::builder().build()
    }
}
