use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::telemetry::system::sanitize_log_message,
};

pub trait RefreshState {
    fn last_refresh(&self) -> Option<Instant>;
    fn set_last_refresh(&mut self, instant: Option<Instant>);
    fn refresh_in_flight(&self) -> bool;
    fn set_refresh_in_flight(&mut self, in_flight: bool);
    fn last_error(&self) -> Option<&str>;
    fn set_last_error(&mut self, error: Option<String>);
}

pub fn refresh_cached_value<TState, TStoredValue, TQueryValue, TQuery, TApply, TRead, TNeedsSync>(
    paths: &ServicePaths,
    component_name: &str,
    cache: &Arc<Mutex<TState>>,
    refresh_interval: Duration,
    needs_sync_bootstrap: TNeedsSync,
    query: TQuery,
    apply: TApply,
    read: TRead,
) -> TStoredValue
where
    TState: RefreshState + Send + 'static,
    TStoredValue: Clone,
    TQueryValue: Send + 'static,
    TQuery: Fn() -> Result<TQueryValue, Box<dyn std::error::Error + Send + Sync>>
        + Send
        + Copy
        + 'static,
    TApply: Fn(&mut TState, &Result<TQueryValue, Box<dyn std::error::Error + Send + Sync>>)
        + Send
        + Copy
        + 'static,
    TRead: Fn(&TState) -> TStoredValue,
    TNeedsSync: Fn(&TState) -> bool,
{
    let mut should_spawn = false;
    let mut should_run_sync = false;

    {
        let mut guard = cache.lock().expect("refresh cache lock poisoned");
        let stale = guard
            .last_refresh()
            .map(|instant| instant.elapsed() >= refresh_interval)
            .unwrap_or(true);

        if stale && !guard.refresh_in_flight() {
            guard.set_refresh_in_flight(true);
            if needs_sync_bootstrap(&guard) {
                should_run_sync = true;
            } else {
                should_spawn = true;
            }
        }
    }

    if should_run_sync {
        let result = query();
        let mut guard = cache.lock().expect("refresh cache lock poisoned");
        apply(&mut guard, &result);
        finalize_refresh(paths, component_name, &mut *guard, &result);
        return read(&guard);
    }

    if should_spawn {
        let cache = cache.clone();
        let paths = paths.clone();
        let component_name = component_name.to_string();
        std::thread::spawn(move || {
            let result = query();
            let mut guard = cache.lock().expect("refresh cache lock poisoned");
            apply(&mut guard, &result);
            finalize_refresh(&paths, &component_name, &mut *guard, &result);
        });
    }

    let guard = cache.lock().expect("refresh cache lock poisoned");
    read(&guard)
}

fn finalize_refresh<TState, TQueryValue>(
    paths: &ServicePaths,
    component_name: &str,
    state: &mut TState,
    result: &Result<TQueryValue, Box<dyn std::error::Error + Send + Sync>>,
) where
    TState: RefreshState,
{
    let is_first_refresh = state.last_refresh().is_none();
    match result {
        Ok(_) => {
            if is_first_refresh {
                let _ = write_log_line(
                    &paths.component_log(component_name),
                    "INFO",
                    "Initial refresh succeeded.",
                );
            } else if state.last_error().is_some() {
                let _ = write_log_line(
                    &paths.component_log(component_name),
                    "INFO",
                    "Recovered after prior refresh failure.",
                );
            }
            state.set_last_error(None);
        }
        Err(error) => {
            let summary = sanitize_log_message(&error.to_string());
            if state.last_error() != Some(summary.as_str()) {
                let _ = write_log_line(&paths.component_log(component_name), "ERROR", &summary);
            }
            state.set_last_error(Some(summary));
        }
    }

    state.set_last_refresh(Some(Instant::now()));
    state.set_refresh_in_flight(false);
}
