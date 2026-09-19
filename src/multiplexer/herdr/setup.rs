//! Adapter-owned setup lifetime. Shared orchestration does not own Herdr launches.
use super::*;

// Define backend primitives once. The private scope forwards those exact methods
// but inherits Multiplexer's setup_panes default. Only the real backend overrides
// setup_panes (the first item), so calling setup on the scope cannot recurse.
// New backend overrides are forwarded automatically; no second method list exists.
macro_rules! impl_backend {
    ($setup:item $(
        fn $name:ident(&$this:ident $(, $arg:ident: $ty:ty)* $(,)?) -> $result:ty $body:block
    )*) => {
        impl Multiplexer for HerdrBackend {
            $setup
            $(fn $name(&$this $(, $arg: $ty)*) -> $result $body)*
        }
        impl Multiplexer for setup::Setup<'_> {
            $(fn $name(&$this $(, $arg: $ty)*) -> $result {
                $this.backend.$name($($arg),*)
            })*
        }
    };
}
pub(super) use impl_backend;

pub(super) struct Setup<'a> {
    pub(super) backend: &'a HerdrBackend,
    initial: &'a str,
    existing: HashSet<String>,
    pending: Option<Arc<pane_launch::Launch>>,
}

impl<'a> Setup<'a> {
    pub(super) fn new(backend: &'a HerdrBackend, initial: &'a str) -> Self {
        Self {
            backend,
            initial,
            existing: backend.launches.lock().unwrap().keys().cloned().collect(),
            pending: backend.pending_launch.lock().unwrap().clone(),
        }
    }

    pub(super) fn finish(&self) -> Result<()> {
        self.backend.finish_pane_setup(&[self.initial.to_string()])
    }
}

impl Drop for Setup<'_> {
    fn drop(&mut self) {
        // Delivered commands have already left this map. Cancel only undelivered
        // commands allocated by this setup, including failures after readiness.
        let keys: Vec<_> = self
            .backend
            .launches
            .lock()
            .unwrap()
            .keys()
            .filter(|key| !self.existing.contains(*key))
            .cloned()
            .collect();
        for key in keys {
            let _ = self.backend.cancel_pane_launch(&key);
        }
        let mut pending = self.backend.pending_launch.lock().unwrap();
        if pending.as_ref().is_some_and(|launch| {
            !self
                .pending
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(old, launch))
        }) {
            pending.take().unwrap().cancel();
        }
        drop(pending);
        // Also end replacement permission after an error. An unchanged initial
        // shell remains usable, just as a created shell does without setup.
        let _ = self.finish();
    }
}

impl HerdrBackend {
    pub(super) fn cancel_pane_launch(&self, key: &str) -> Result<()> {
        if let Some(launch) = self.launches.lock().unwrap().remove(key) {
            launch.cancel();
            if let Ok(pane) = self.pane(key) {
                self.client
                    .request("pane.close", json!({"pane_id":pane.pane_id}))?;
            }
        }
        Ok(())
    }

    pub(super) fn finish_pane_setup(&self, pane_ids: &[String]) -> Result<()> {
        for key in pane_ids {
            self.fresh.lock().unwrap().remove(key);
            if let Some(launch) = self.initial_launches.lock().unwrap().remove(key) {
                launch.release()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_cancels_only_its_undelivered_launches() -> Result<()> {
        let backend = HerdrBackend::for_socket("");
        let directory = tempfile::tempdir()?;
        let old = pane_launch::Launch::new("/bin/sh", directory.path())?;
        let new = pane_launch::Launch::new("/bin/sh", directory.path())?;
        backend
            .launches
            .lock()
            .unwrap()
            .insert("old".into(), old.clone());
        backend.fresh.lock().unwrap().insert("initial".into());
        {
            let _setup = Setup::new(&backend, "initial");
            backend
                .launches
                .lock()
                .unwrap()
                .insert("new".into(), new.clone());
            *backend.pending_launch.lock().unwrap() = Some(new.clone());
        }
        assert!(backend.launches.lock().unwrap().contains_key("old"));
        assert!(!backend.launches.lock().unwrap().contains_key("new"));
        assert!(!backend.fresh.lock().unwrap().contains("initial"));
        assert!(backend.pending_launch.lock().unwrap().is_none());
        assert!(
            new.deliver("exit 99")
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        old.cancel();
        Ok(())
    }

    #[test]
    fn scope_preserves_a_preexisting_pending_launch() -> Result<()> {
        let backend = HerdrBackend::for_socket("");
        let directory = tempfile::tempdir()?;
        let old = pane_launch::Launch::new("/bin/sh", directory.path())?;
        *backend.pending_launch.lock().unwrap() = Some(old.clone());
        drop(Setup::new(&backend, "initial"));
        assert!(Arc::ptr_eq(
            backend.pending_launch.lock().unwrap().as_ref().unwrap(),
            &old
        ));
        old.cancel();
        Ok(())
    }
}
