use serde::Deserialize;
use tauri::Runtime;
use tauri::plugin::{PluginHandle, mobile::PluginInvokeError};

/// Storage access and the folder picker. Calls block until the user answers, so make them off
/// the main thread (async commands).
#[derive(Debug)]
pub struct Storage<R: Runtime>(pub(crate) PluginHandle<R>);

#[derive(Deserialize)]
struct Access {
    granted: bool,
}

#[derive(Deserialize)]
struct Folder {
    path: Option<String>,
}

impl<R: Runtime> Storage<R> {
    /// Whether Cadenza can read music folders.
    pub fn has_access(&self) -> Result<bool, PluginInvokeError> {
        Ok(self
            .0
            .run_mobile_plugin::<Access>("storageAccess", ())?
            .granted)
    }

    /// Asks for access (the system's settings page or permission prompt); true once granted.
    pub fn request_access(&self) -> Result<bool, PluginInvokeError> {
        Ok(self
            .0
            .run_mobile_plugin::<Access>("requestStorageAccess", ())?
            .granted)
    }

    /// The system folder picker. `None` if cancelled; an error if the folder isn't on local
    /// storage (a cloud provider's folder has no file path).
    pub fn pick_folder(&self) -> Result<Option<String>, PluginInvokeError> {
        Ok(self.0.run_mobile_plugin::<Folder>("pickFolder", ())?.path)
    }
}
