use bar_identity::EnvironmentId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

/// One gate per environment. Reading, pairing and forgetting that environment's
/// saved grant run one at a time, while other environments and unrelated
/// network work proceed.
#[derive(Default)]
pub struct CredentialGates(Mutex<HashMap<EnvironmentId, Arc<Mutex<()>>>>);

impl CredentialGates {
    pub fn serialize<R>(&self, environment: &EnvironmentId, operation: impl FnOnce() -> R) -> R {
        let gate = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(environment.clone())
            .or_default()
            .clone();
        let _held = gate.lock().unwrap_or_else(PoisonError::into_inner);
        operation()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn same_environment_runs_one_at_a_time_and_others_are_independent() {
        let gates = Arc::new(CredentialGates::default());
        let a = EnvironmentId::new("a").unwrap();
        let b = EnvironmentId::new("b").unwrap();
        let inside = Arc::new(AtomicUsize::new(0));
        let overlapped = Arc::new(AtomicUsize::new(0));
        let workers: Vec<_> = (0..6)
            .map(|_| {
                let (gates, a, inside, overlapped) =
                    (gates.clone(), a.clone(), inside.clone(), overlapped.clone());
                std::thread::spawn(move || {
                    gates.serialize(&a, || {
                        if inside.fetch_add(1, Ordering::SeqCst) != 0 {
                            overlapped.fetch_add(1, Ordering::SeqCst);
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        inside.fetch_sub(1, Ordering::SeqCst);
                    })
                })
            })
            .collect();
        gates.serialize(&a, || {
            // Holding `a` must not block `b`.
            let independent = gates.clone();
            let b = b.clone();
            std::thread::spawn(move || independent.serialize(&b, || ()))
                .join()
                .unwrap();
        });
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(overlapped.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_panicking_operation_does_not_wedge_the_environment() {
        let gates = Arc::new(CredentialGates::default());
        let id = EnvironmentId::new("a").unwrap();
        let panicking = {
            let (gates, id) = (gates.clone(), id.clone());
            std::thread::spawn(move || gates.serialize(&id, || panic!("operation failed")))
        };
        assert!(panicking.join().is_err());
        assert_eq!(gates.serialize(&id, || 7), 7);
    }
}
