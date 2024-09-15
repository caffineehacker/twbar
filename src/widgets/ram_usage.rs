use std::cell::OnceCell;
use std::collections::HashMap;
use std::io::Error;
use std::time::Duration;

use async_std::sync::{Arc, Mutex, Weak};
use async_std::task::{self, sleep};
use async_std::{fs::File, io::ReadExt};

use gio::glib::{clone, SendWeakRef, WeakRef};
use gio::prelude::*;
use gtk4::glib::Object;
use gtk4::subclass::prelude::*;
use gtk4::{
    glib, Accessible, Buildable, ConstraintTarget, EventControllerMotion, Orientable, Popover,
    Widget,
};
use gtk4::{prelude::*, Label};

struct RamInfo {
    controls: Mutex<Vec<SendWeakRef<RamUsage>>>,
}

impl RamInfo {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<RamInfo>> = Mutex::new(Weak::new());

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
        let instance = Arc::new(Self {
            controls: Mutex::new(Vec::new()),
        });

        let me = instance.clone();
        glib::spawn_future_local(async move {
            loop {
                let mem_info = Self::read_memory_info().await;
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

                        let mut controls = me.controls.lock().await;

                        for (index, control) in controls.clone().iter().enumerate().rev() {
                            match control.upgrade() {
                                Some(control) => {
                                    control.imp().update_labels(
                                        &format!(
                                            "   {}%",
                                            (memused_percent * 100.0).round() as i64
                                        ),
                                        &format!(
                                            "Total: {:.2} GB\nUsed: {:.2} GB",
                                            (memtotal as f64) / (1024.0 * 1024.0),
                                            (memused as f64) / (1024.0 * 1024.0)
                                        ),
                                    );
                                }
                                None => {
                                    controls.remove(index);
                                }
                            }
                        }
                    }
                };
                sleep(Duration::from_secs(1)).await;
            }
        });

        instance
    }

    pub async fn register_control(&self, control: SendWeakRef<RamUsage>) {
        self.controls.lock().await.push(control);
    }

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
}

// Object holding the state
#[derive(Default)]
pub struct RamUsageImpl {
    label_ref: OnceCell<WeakRef<Label>>,
    popup_label_ref: OnceCell<WeakRef<Label>>,
}

impl RamUsageImpl {
    pub fn update_labels(&self, text: &str, popup_text: &str) {
        match self.label_ref.get().and_then(|l| l.upgrade()) {
            Some(label) => label.set_text(text),
            None => {
                log::trace!("Label ref upgrade failed");
                return;
            }
        };

        match self.popup_label_ref.get().and_then(|l| l.upgrade()) {
            Some(label) => label.set_text(popup_text),
            None => {
                log::trace!("Popup label upgrade failed");
                return;
            }
        };
    }
}

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

        self.label_ref.set(label.downgrade()).unwrap();

        let popup_label = Label::new(Some(""));
        let popup = Popover::new();
        popup.set_child(Some(&popup_label));
        popup.set_parent(self.obj().upcast_ref::<Widget>());
        popup.set_autohide(false);
        popup.set_focusable(false);
        popup.set_can_focus(false);

        self.popup_label_ref.set(popup_label.downgrade()).unwrap();

        let weak_me: SendWeakRef<RamUsage> = self.obj().downgrade().into();
        task::block_on(async move {
            RamInfo::instance().await.register_control(weak_me).await;
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
