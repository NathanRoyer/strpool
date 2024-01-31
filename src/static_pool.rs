use std::sync::Mutex;
use super::Pool;

#[derive(Debug)]
pub struct PoolCell<const P: usize> {
    pool: Mutex<Option<Pool<P>>>,
    pub(crate) subpools: usize,
}

impl<const P: usize> PoolCell<P> {
    pub const fn new() -> Self {
        Self {
            pool: Mutex::new(None),
            subpools: P,
        }
    }

    /// Swaps the inner pool with a new one
    pub fn swap(&self, pool: Pool<P>) {
        let mut guard = self.pool.lock().expect("Couldn't lock Mutex");
        *guard = Some(pool);
    }

    /// Retrieves the inner pool
    pub fn pool(&self) -> Pool<P> {
        let mut guard = self.pool.lock().expect("Couldn't lock Mutex");

        if guard.is_none() {
            *guard = Some(Pool::new());
        }

        guard.as_ref().unwrap().clone()
    }
}
