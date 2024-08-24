use async_std::sync::{Arc, Mutex, Weak};
use gio::DesktopAppInfo;
use log::trace;

struct XdgApplication {
    name: String,
    file_path: String,
    icon: String,
    exec: String,
}

pub struct XdgApplicationsCache {
    applications: Vec<XdgApplication>,
}

impl XdgApplicationsCache {
    pub async fn get_instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<XdgApplicationsCache>> = Mutex::new(Weak::new());

        let mut mutex_guard = INSTANCE.lock().await;
        match mutex_guard.upgrade() {
            Some(instance) => instance,
            None => {
                let instance = Arc::new(Self::new());
                *mutex_guard = Arc::downgrade(&instance);
                instance
            }
        }
    }

    fn new() -> Self {
        Self {
            applications: Vec::new(),
        }
    }

    pub fn get_application_by_class(&self, class_name: &str) -> Option<DesktopAppInfo> {
        let matches = gio::DesktopAppInfo::search(class_name);

        for outer in matches {
            for desktop_id in outer {
                trace!("Found match {} -> {}", class_name, desktop_id);
                if let Some(info) = gio::DesktopAppInfo::new(desktop_id.as_str()) {
                    return Some(info);
                }
            }
        }

        None
    }
}
