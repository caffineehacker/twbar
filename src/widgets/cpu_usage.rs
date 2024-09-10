use std::sync::LazyLock;
use std::time::Duration;
use std::{io::Error, str::FromStr};

use async_std::sync::{Arc, Mutex};
use async_std::task::{self, sleep};
use async_std::{fs::File, io::ReadExt};

use futures::FutureExt as OtherFutureExt;
use gio::glib::{clone, random_int, WeakRef};
use gio::prelude::*;
use gtk4::glib::Object;
use gtk4::subclass::prelude::*;
use gtk4::{
    glib, Accessible, Buildable, ConstraintTarget, EventControllerMotion, Orientable, Popover,
    Widget,
};
use gtk4::{prelude::*, Label};

#[allow(dead_code)]
struct CpuStat {
    name: String,
    user: i64,
    nice: i64,
    system: i64,
    idle: i64,
    iowait: i64,
    irq: i64,
    softirq: i64,
    steal: i64,
    guest: i64,
    guest_nice: i64,
}

impl CpuStat {
    fn from_proc_stat_line(line: &str) -> Result<Self, <i64 as FromStr>::Err> {
        let parts = line.split_ascii_whitespace().collect::<Vec<&str>>();
        if parts.len() != 11 {
            log::error!("Expected 11 parts, got {:?}", parts);
        }

        Ok(Self {
            name: parts[0].to_owned(),
            user: parts[1].parse::<i64>()?,
            nice: parts[2].parse::<i64>()?,
            system: parts[3].parse::<i64>()?,
            idle: parts[4].parse::<i64>()?,
            iowait: parts[5].parse::<i64>()?,
            irq: parts[6].parse::<i64>()?,
            softirq: parts[7].parse::<i64>()?,
            steal: parts[8].parse::<i64>()?,
            guest: parts[9].parse::<i64>()?,
            guest_nice: parts[10].parse::<i64>()?,
        })
    }

    pub fn total_idle_time(&self) -> i64 {
        self.idle + self.iowait
    }

    fn total_system_time(&self) -> i64 {
        self.system + self.irq + self.softirq
    }

    #[allow(dead_code)]
    fn virtual_time(&self) -> i64 {
        self.guest + self.guest_nice
    }

    pub fn total_time(&self) -> i64 {
        // We don't include virtual time since guest is included in user and guest_nice is included in nice
        self.user + self.nice + self.total_system_time() + self.total_idle_time() + self.steal
    }
}

#[allow(dead_code)]
struct CpuStatDiff {
    total: i64,
    idle: i64,
    percent_usage: i64,
}

trait Callback: Send {
    fn call(&mut self, diffs: &Vec<CpuStatDiff>);
}

struct CpuStatMonitor {
    callbacks: Arc<Mutex<Vec<(u32, Box<dyn Callback>)>>>,
}

impl CpuStatMonitor {
    pub fn new() -> Self {
        let callbacks: Arc<Mutex<Vec<(u32, Box<dyn Callback>)>>> = Arc::new(Mutex::new(Vec::new()));
        let callbacks_ref = Arc::downgrade(&callbacks);
        task::spawn(async move {
            let mut prev_cpu_info: Vec<CpuStat> = Vec::new();
            loop {
                let cpu_info = Self::read_cpu_info().await;
                match cpu_info {
                    Err(e) => {
                        log::error!("Failed to read cpu info: {}", e);
                    }
                    Ok(cpu_info) => {
                        if prev_cpu_info.len() == cpu_info.len() && cpu_info.len() > 0 {
                            let diffs = prev_cpu_info
                                .iter()
                                .zip(cpu_info.iter())
                                .map(|(prev, current)| {
                                    let total = current.total_time() - prev.total_time();
                                    let idle = current.total_idle_time() - prev.total_idle_time();
                                    CpuStatDiff {
                                        total,
                                        idle,
                                        percent_usage: ((total - idle) * 100) / total.max(1),
                                    }
                                })
                                .collect();

                            match callbacks_ref.upgrade() {
                                Some(callbacks) => {
                                    glib::idle_add_once(move || {
                                        let mut callbacks = callbacks.lock().boxed();

                                        loop {
                                            match (&mut callbacks).now_or_never() {
                                                Some(mut callbacks) => {
                                                    callbacks.iter_mut().for_each(
                                                        |(_id, callback)| {
                                                            callback.as_mut().call(&diffs)
                                                        },
                                                    );
                                                    break;
                                                }
                                                None => {}
                                            }
                                        }
                                    });
                                }
                                _ => log::error!(
                                    "Failed to upgrade callbacks, this should not happen"
                                ),
                            }
                        }

                        prev_cpu_info = cpu_info;
                    }
                };
                sleep(Duration::from_secs(1)).await;
            }
        });

        Self { callbacks }
    }

    pub fn add_callback(&self, callback: Box<dyn Callback>) -> u32 {
        let callback_id = random_int();
        let callbacks = self.callbacks.clone();
        task::block_on(async move { callbacks.lock().await.push((callback_id, callback)) });

        callback_id
    }

    pub fn remove_callback(&self, callback_id: u32) {
        task::block_on(async move {
            let mut callbacks = self.callbacks.lock().await;
            let index_to_remove = callbacks.iter().enumerate().find_map(|(i, (cid, _c))| {
                if *cid == callback_id {
                    Some(i)
                } else {
                    None
                }
            });

            match index_to_remove {
                Some(index) => {
                    callbacks.remove(index);
                }
                _ => log::error!(
                    "Failed to find callback to remove. Looking for id {}",
                    callback_id
                ),
            };
        });
    }

    async fn read_cpu_info() -> Result<Vec<CpuStat>, Error> {
        let mut stat = File::open("/proc/stat").await?;
        let mut buf: String = String::default();
        stat.read_to_string(&mut buf).await?;
        Ok(buf
            .lines()
            .take_while(|line| line.starts_with("cpu"))
            .map(|line| CpuStat::from_proc_stat_line(line).unwrap())
            .collect::<Vec<CpuStat>>())
    }
}

// Because of this, once initialized we never stop the process of reading the CPU stats. This is probably OK since this is for a widget which is likely either always shown or never shown.
static MONITOR_INSTANCE: LazyLock<CpuStatMonitor> = LazyLock::new(|| CpuStatMonitor::new());

// Object holding the state
#[derive(Default)]
pub struct CpuUsageImpl {}

impl CpuUsageImpl {}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for CpuUsageImpl {
    const NAME: &'static str = "TwBarCpuUsage";
    type Type = CpuUsage;
    type ParentType = gtk4::Box;
}

// Trait shared by all GObjects
impl ObjectImpl for CpuUsageImpl {
    fn constructed(&self) {
        self.parent_constructed();

        self.obj().add_css_class("cpu_usage");
        let label = Label::new(Some(""));
        self.obj().append(&label);

        let label_ref = label.downgrade();

        let popup_label = Label::new(Some(""));
        let popup = Popover::new();
        popup.set_child(Some(&popup_label));
        popup.set_parent(self.obj().upcast_ref::<Widget>());
        popup.set_autohide(false);
        popup.set_focusable(false);
        popup.set_can_focus(false);

        let random_id = random_int();

        let popup_label_ref = popup_label.downgrade();

        let monitor = &MONITOR_INSTANCE;

        struct CallbackStruct {
            label_ref: WeakRef<Label>,
            popup_label_ref: WeakRef<Label>,
        }
        // This is OK since the callback will always be on the UI thread
        unsafe impl Send for CallbackStruct {}
        impl Callback for CallbackStruct {
            fn call(&mut self, diffs: &Vec<CpuStatDiff>) {
                let mut tooltip_text = "".to_owned();
                for i in 0..diffs.len() {
                    if i == 0 {
                        match self.label_ref.upgrade() {
                            Some(label) => {
                                label.set_text(&format!("   {}%", diffs[i].percent_usage))
                            }
                            None => {
                                log::trace!("Label ref upgrade failed");
                                return;
                            }
                        };
                        tooltip_text.push_str(&format!("Total: {}%", diffs[i].percent_usage));
                    } else {
                        tooltip_text
                            .push_str(&format!("\nCore {}: {}%", i, diffs[i].percent_usage));
                    }
                }
                match self.popup_label_ref.upgrade() {
                    Some(label) => label.set_text(&tooltip_text),
                    None => {
                        log::trace!("Popup label upgrade failed");
                        return;
                    }
                };
            }
        }

        let callback = CallbackStruct {
            label_ref,
            popup_label_ref,
        };

        let callback_id = monitor.add_callback(Box::new(callback));

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
            log::trace!("CPU Usage destroy: {}", random_id);
            popup.unparent();
            monitor.remove_callback(callback_id);
        });
    }
}

// Trait shared by all widgets
impl WidgetImpl for CpuUsageImpl {}

// Trait shared by all boxes
impl BoxImpl for CpuUsageImpl {}

// Self encapsulated button that triggers the appropriate workspace on click
glib::wrapper! {
    pub struct CpuUsage(ObjectSubclass<CpuUsageImpl>)
        @extends gtk4::Box, Widget,
        @implements Accessible, Buildable, ConstraintTarget, Orientable;
}

impl CpuUsage {
    pub fn new() -> Self {
        Object::builder().build()
    }
}
