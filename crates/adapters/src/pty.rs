//! Agents Claude Code dans un pseudo-terminal.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

pub struct LaunchSpec<'a> {
    pub claude: &'a str,
    pub cwd: &'a Path,
    pub path_env: &'a str,
    pub args: Vec<String>,
    pub env: Vec<(&'static str, String)>,
    pub cols: u16,
    pub rows: u16,
}

pub struct AgentProcess {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
}

impl AgentProcess {
    /// Lance le binaire directement, sans shell : aucun argument n'est interprété.
    pub fn spawn(
        spec: LaunchSpec,
        on_output: impl Fn(Vec<u8>) + Send + 'static,
        on_exit: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        let pair = native_pty_system()
            .openpty(PtySize { rows: spec.rows, cols: spec.cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(spec.claude);
        cmd.args(&spec.args);
        cmd.cwd(spec.cwd);
        cmd.env("PATH", spec.path_env);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in spec.env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let child = Arc::new(Mutex::new(child));

        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => on_output(buf[..n].to_vec()),
                }
            }
        });
        let waiter = child.clone();
        std::thread::spawn(move || {
            loop {
                let exited = waiter.lock().map(|mut c| matches!(c.try_wait(), Ok(Some(_)) | Err(_))).unwrap_or(true);
                if exited {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            on_exit();
        });

        Ok(Self { master: Mutex::new(pair.master), writer: Mutex::new(writer), child })
    }

    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut w = self.writer.lock().map_err(|e| e.to_string())?;
        w.write_all(data).and_then(|_| w.flush()).map_err(|e| e.to_string())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let master = self.master.lock().map_err(|e| e.to_string())?;
        master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())
    }

    pub fn kill(&self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
        }
    }
}
