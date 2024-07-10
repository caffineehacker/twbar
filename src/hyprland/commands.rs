use std::time::Duration;

use async_std::{io::{self, ReadExt, WriteExt}, os::unix::net::UnixStream, sync::{Arc, Mutex, Weak}};

use super::utils::Utils;

pub struct HyprlandCommands {
    socket: Mutex<UnixStream>,
}

impl HyprlandCommands {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<HyprlandCommands>> = Mutex::new(Weak::new());
        
        let mut mutex_guard = INSTANCE.lock().await;
        match mutex_guard.upgrade() {
            Some(instance) => instance,
            None => {
                let instance = Self::new().await;
                *mutex_guard = Arc::downgrade(&instance);
                instance
            }
        }
    }

    async fn new() -> Arc<Self> {
        Arc::new(Self {
            socket: Mutex::new(Utils::create_dispatch_socket().await.unwrap())
        })
    }

    pub async fn send_command(&self, command: &str) -> String {
        let mut socket = self.socket.lock().await;
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
}