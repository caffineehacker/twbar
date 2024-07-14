use std::time::Duration;

use async_std::{io::{self, ReadExt, WriteExt}, os::unix::net::UnixStream, sync::{Arc, Mutex, Weak}};

use super::utils::Utils;

pub struct HyprlandCommands {
    socket: Mutex<UnixStream>,
}

impl HyprlandCommands {
    pub async fn send_command(command: &str) -> String {
        let mut socket = Utils::create_dispatch_socket().await.unwrap();
        socket.write_all(&command.as_bytes()).await.unwrap();
        io::timeout(Duration::from_secs(1), async {
            let mut buf = vec![0; 1024];
            let mut final_buffer = Vec::new();
            let mut bytes_read = 1024;
            while bytes_read == 1024 {
                bytes_read = socket.read(&mut buf).await?;
                if bytes_read > 0 {
                    final_buffer.extend_from_slice(&buf[..bytes_read]);
                }
            }

            let response = String::from_utf8(final_buffer).unwrap();
            Ok(response)
        }).await.unwrap()
    }

    pub async fn set_active_window(window_address: &str) {
        Self::send_command(&format!("dispatch focuswindow address:{}", window_address)).await;
    }
}