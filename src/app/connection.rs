use super::*;
use crate::provider::lm_studio::normalize_base_url;

#[derive(Default)]
pub(super) struct Connection {
    pub connected: bool,
    pub available: bool,
    pub checking: bool,
    pub connect_requested: bool,
    pub request_id: u64,
    pub error: Option<String>,
}

impl Connection {
    pub fn disconnect(&mut self) {
        self.request_id += 1;
        self.connected = false;
        self.available = false;
        self.checking = false;
        self.connect_requested = false;
    }
}

impl MoxiBoxApp {
    pub(super) fn refresh_models(&mut self, connect: bool) {
        if self.connection.checking || self.run_pending {
            return;
        }
        let url = match normalize_base_url(&self.settings.lm_studio_url) {
            Ok(url) => url,
            Err(error) => {
                self.connection.error = Some(error);
                return;
            }
        };
        self.cancelled = Arc::new(AtomicBool::new(false));
        self.connection.request_id += 1;
        self.connection.checking = true;
        self.connection.connect_requested = connect;
        self.connection.error = None;
        self.command(AppCommand::RefreshModels {
            request_id: self.connection.request_id,
            base_url: url,
            cancelled: self.cancelled.clone(),
        });
    }

    pub(super) fn models_loaded(&mut self, request_id: u64, result: Result<Vec<String>, String>) {
        if request_id != self.connection.request_id || !self.connection.checking {
            return;
        }
        self.connection.checking = false;
        match result {
            Ok(models) => {
                self.connection.available = true;
                self.connection.connected |= self.connection.connect_requested;
                self.models = models;
                // Preserve an exact model ID even if discovery doesn't list it.
                if self.settings.selected_model.is_none() {
                    self.settings.selected_model = self.models.first().cloned();
                    self.command(AppCommand::SaveSettings(self.settings.clone()));
                }
                self.connection.error = None;
            }
            Err(error) => {
                self.connection.connected = false;
                self.connection.available = false;
                self.connection.error = Some(error);
            }
        }
        self.connection.connect_requested = false;
    }

    pub(super) fn save_provider(&mut self, draft: Settings) -> Result<(), String> {
        let url = normalize_base_url(&draft.lm_studio_url)?;
        self.connection.disconnect();
        self.connection.error = None;
        self.models.clear();
        self.settings.provider_name = draft.provider_name;
        self.settings.lm_studio_url = url;
        self.settings.selected_model = draft
            .selected_model
            .filter(|m| !m.trim().is_empty())
            .map(|m| m.trim().to_owned());
        self.command(AppCommand::SaveSettings(self.settings.clone()));
        self.refresh_models(false);
        Ok(())
    }
}
