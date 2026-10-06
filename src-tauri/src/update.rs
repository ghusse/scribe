//! Updates published as GitHub releases (tauri-plugin-updater, signed with the key in the release secrets).
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::overlay::ToastLevel;
use crate::services::{AvailableUpdate, Services};

/// The startup check waits a little: the network may not be up yet right after login.
pub const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(30);

/// What Réglages > Mises à jour shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateStatus {
    pub current: String,
    pub available: Option<AvailableUpdate>,
}

pub async fn status(svc: &Services) -> Result<UpdateStatus, String> {
    let available = svc.updater.check().await?;
    Ok(UpdateStatus { current: svc.updater.current_version(), available })
}

pub fn available_message(update: &AvailableUpdate) -> String {
    format!("Scribe {} est disponible : Réglages > Mises à jour", update.version)
}

/// At startup: a toast when a newer version exists. No network, no release yet or a broken manifest stay
/// silent (logged): the user did not ask.
pub async fn check_at_startup(svc: Arc<Services>) {
    match svc.updater.check().await {
        Ok(Some(update)) => svc.overlay.toast(ToastLevel::Info, available_message(&update), None, None),
        Ok(None) => {}
        Err(e) => tracing::info!("vérification des mises à jour impossible : {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::OverlayEvent;
    use crate::testing::Fixture;

    fn update(version: &str) -> AvailableUpdate {
        AvailableUpdate { version: version.into(), notes: Some("Corrections".into()) }
    }

    #[test]
    fn status_reports_the_running_and_the_published_versions() {
        let f = Fixture::new();
        assert_eq!(tauri::async_runtime::block_on(status(&f.svc)), Ok(UpdateStatus { current: "0.1.0".into(), available: None }));
        *f.updater.available.lock().unwrap() = Some(update("0.2.0"));
        assert_eq!(tauri::async_runtime::block_on(status(&f.svc)).unwrap().available, Some(update("0.2.0")));
        *f.updater.fail.lock().unwrap() = Some("réseau indisponible".into());
        assert_eq!(tauri::async_runtime::block_on(status(&f.svc)), Err("réseau indisponible".into()));
    }

    #[test]
    fn the_startup_check_announces_a_new_version_and_is_silent_otherwise() {
        let f = Fixture::new();
        tauri::async_runtime::block_on(check_at_startup(f.svc.clone()));
        assert_eq!(f.window.last_event(), None, "up to date");
        *f.updater.fail.lock().unwrap() = Some("réseau indisponible".into());
        tauri::async_runtime::block_on(check_at_startup(f.svc.clone()));
        assert_eq!(f.window.last_event(), None, "offline");
        *f.updater.fail.lock().unwrap() = None;
        *f.updater.available.lock().unwrap() = Some(update("0.2.0"));
        tauri::async_runtime::block_on(check_at_startup(f.svc.clone()));
        assert_eq!(
            f.window.last_event(),
            Some(OverlayEvent::Toast {
                level: ToastLevel::Info,
                message: "Scribe 0.2.0 est disponible : Réglages > Mises à jour".into(),
                preview: None,
                dictation_id: None,
            })
        );
    }
}
