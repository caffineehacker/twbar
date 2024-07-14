use std::env::var;

use async_std::{path::Path, sync::{Arc, Mutex, Weak}};
use async_std::fs;
use async_std::prelude::*;
use gio::DesktopAppInfo;


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
    // pub fn get_instance() -> Arc<Self> {
    //     static INSTANCE: Mutex<Weak<XdgApplicationsCache>> = Mutex::new(Weak::new());

    //     let mut mutex_guard = INSTANCE.lock().await;
    //     match mutex_guard.upgrade() {
    //         Some(instance) => instance,
    //         None => {
    //             let instance = Self::new();
    //             *mutex_guard = Arc::downgrade(&instance);
    //             instance
    //         }
    //     }
    // }

    // async fn new() -> Arc<Self> {
    //     let xdg_data_dirs = var("XDG_DATA_DIRS").unwrap();
    //     let mut applications = Vec::new();
    //     for data_dir in xdg_data_dirs.split(":") {
    //         let path = Path::new(data_dir).join("applications");
    //         if path.exists().await {
    //             let entries = fs::read_dir(path).await.unwrap();
    //             while let Some(file) = entries.next().await {
    //                 let contents = fs::read_to_string(file.unwrap().path()).await.unwrap();

    //             }
    //         }
    //     }
    //     Arc::new(Self {

    //     })
    // }

    pub fn new() -> Self {
        Self {
            applications: Vec::new(),
        }
    }

    pub fn get_application_by_class(&self, class_name: &str) -> Option<DesktopAppInfo> {
        let matches = gio::DesktopAppInfo::search(class_name);

        for outer in matches {
            for desktop_id in outer {
                println!("Found match {} -> {}", class_name, desktop_id);
                match gio::DesktopAppInfo::new(desktop_id.as_str()) {
                    Some(info) => { return Some(info); },
                    _ => {},
                }
            }
        }

        None
    }
}