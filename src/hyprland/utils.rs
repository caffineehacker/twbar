use async_std::{io, os::unix::net::UnixStream, path::PathBuf};
use std::env::var;

pub(super) struct Utils {
}

impl Utils {
    pub async fn create_event_socket() -> Result<UnixStream, io::Error> {
        let path =Self::get_hyprland_instance_path()
            .join(".socket2.sock");
        if !path.exists().await {
            panic!("Could not find Hyprland socket path");
        }

        UnixStream::connect(path).await
    }

    pub async fn create_dispatch_socket() -> Result<UnixStream, io::Error> {
        let path = Self::get_hyprland_instance_path()
            .join(".socket.sock");
        if !path.exists().await {
            panic!("Could not find Hyprland socket path");
        }

        UnixStream::connect(path).await
    }

    fn get_hyprland_instance_path() -> PathBuf {
        let instance = match var("HYPRLAND_INSTANCE_SIGNATURE") {
            Ok(var) => var,
            Err(_) => {
                panic!("Could not find HYPRLAND_INSTANCE_SIGNATURE variable, is Hyprland running?")
            }
        };

        let xdg_runtime_dir = match var("XDG_RUNTIME_DIR") {
            Ok(var) => var,
            Err(_) => {
                panic!("Could not find XDG_RUNTIME_DIR variable")
            }
        };

        PathBuf::from(xdg_runtime_dir)
            .join("hypr")
            .join(instance)
    }
}