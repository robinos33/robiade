//! États des MR liées aux sessions, lus sur GitLab et gardés en mémoire : une MR mergée ou fermée
//! ne change plus, une MR ouverte est relue après quelques minutes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use robiade_core::forge::{Forge, ForgeError, MrOutcomes, MrRef, MrState};
use robiade_core::Millis;

const OPEN_TTL_MS: Millis = 5 * 60 * 1000;
/// Lectures simultanées sur GitLab.
const PARALLEL_READS: usize = 8;

pub struct MrStates {
    forge: Arc<dyn Forge>,
    known: Mutex<HashMap<MrRef, (MrState, Millis)>>,
}

impl MrStates {
    pub fn new(forge: Arc<dyn Forge>) -> Self {
        Self { forge, known: Mutex::default() }
    }

    fn cached(&self, mr: &MrRef, now: Millis) -> Option<MrState> {
        let known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        let (state, read_at) = *known.get(mr)?;
        let settled = matches!(state, MrState::Merged | MrState::Closed);
        (settled || now - read_at < OPEN_TTL_MS).then_some(state)
    }

    /// Bilan des MR données. Une MR illisible compte comme inconnue ; un token absent ou refusé
    /// fait échouer le tout, pour que l'interface le dise au lieu d'afficher 100 % d'inconnues.
    pub fn outcomes(&self, mrs: &[MrRef], now: Millis) -> Result<MrOutcomes, ForgeError> {
        let missing: Vec<&MrRef> = mrs.iter().filter(|mr| self.cached(mr, now).is_none()).collect();
        for chunk in missing.chunks(PARALLEL_READS) {
            let read: Vec<_> = std::thread::scope(|s| {
                let handles: Vec<_> = chunk.iter().map(|mr| s.spawn(|| self.forge.merge_request(mr))).collect();
                handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(ForgeError::Api("panique".into())))).collect()
            });
            let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
            for (mr, result) in chunk.iter().zip(read) {
                match result {
                    Ok(found) => {
                        known.insert((*mr).clone(), (found.state, now));
                    }
                    Err(e @ (ForgeError::NoToken | ForgeError::Unauthorized)) => return Err(e),
                    Err(_) => {}
                }
            }
        }
        let states: Vec<Option<MrState>> = mrs.iter().map(|mr| self.cached(mr, now)).collect();
        Ok(MrOutcomes::of(states.iter().map(Option::as_ref)))
    }
}
