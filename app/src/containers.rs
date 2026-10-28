//! Suivis de logs de conteneurs ouverts par l'interface, relayés en événements `container:logs`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use base64::Engine;
use robiade_core::containers::{ContainerError, ContainerId, Containers, StopLogs};
use tauri::Emitter;

use crate::dto::ContainerLogs;

/// Au-delà, le suivi le plus ancien est arrêté : un panneau oublié par un rechargement de
/// l'interface ne tourne pas indéfiniment.
const MAX_STREAMS: usize = 4;

pub struct LogStreams {
    app: tauri::AppHandle,
    docker: Arc<dyn Containers>,
    streams: Mutex<(u64, BTreeMap<u64, StopLogs>)>,
}

impl LogStreams {
    pub fn new(app: tauri::AppHandle, docker: Arc<dyn Containers>) -> Self {
        Self { app, docker, streams: Mutex::default() }
    }

    /// Ouvre un suivi ; ses octets arrivent en base64, et `data: null` signale la fin du flux.
    pub fn follow(&self, id: &ContainerId, tail: u32) -> Result<u64, ContainerError> {
        let mut guard = self.streams.lock().unwrap_or_else(|e| e.into_inner());
        let (next, streams) = &mut *guard;
        *next += 1;
        let stream = *next;
        let app = self.app.clone();
        let stop = self.docker.follow_logs(
            id,
            tail,
            Box::new(move |bytes| {
                let data = bytes.map(|b| base64::engine::general_purpose::STANDARD.encode(b));
                let _ = app.emit("container:logs", ContainerLogs { stream, data });
            }),
        )?;
        streams.insert(stream, stop);
        while streams.len() > MAX_STREAMS {
            if let Some((_, oldest)) = streams.pop_first() {
                oldest();
            }
        }
        Ok(stream)
    }

    pub fn stop(&self, stream: u64) {
        let stop = self.streams.lock().unwrap_or_else(|e| e.into_inner()).1.remove(&stream);
        if let Some(stop) = stop {
            stop();
        }
    }
}
