use super::*;
use crate::provider::lm_studio::{default_model, normalize_base_url};

/// While connected and idle, the server is checked this often so a stopped server is noticed.
const HEALTH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);
const MAX_RETRY_DELAY_SECONDS: u64 = 30;

pub(super) struct Connection {
    pub connected: bool,
    pub available: bool,
    pub checking: bool,
    pub connect_requested: bool,
    pub request_id: u64,
    pub error: Option<String>,
    /// A quiet health check of a connected provider is in flight.
    pub probing: bool,
    pub checked_at: std::time::Instant,
    /// Set after an explicit connection was lost; cleared by success or Disconnect.
    pub reconnect: Option<Reconnect>,
}

pub(super) struct Reconnect {
    /// Attempts made so far.
    pub attempt: u32,
    pub next_at: std::time::Instant,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            connected: false,
            available: false,
            checking: false,
            connect_requested: false,
            request_id: 0,
            error: None,
            probing: false,
            checked_at: std::time::Instant::now(),
            reconnect: None,
        }
    }
}

impl Connection {
    pub fn disconnect(&mut self) {
        self.request_id += 1;
        self.connected = false;
        self.available = false;
        self.checking = false;
        self.connect_requested = false;
        self.probing = false;
        self.reconnect = None;
    }

    /// Seconds until the next automatic reconnection attempt.
    pub fn retry_in(&self) -> Option<u64> {
        self.reconnect.as_ref().map(|reconnect| {
            reconnect
                .next_at
                .saturating_duration_since(std::time::Instant::now())
                .as_secs()
        })
    }
}

fn retry_delay(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs((2u64 << attempt.min(4)).min(MAX_RETRY_DELAY_SECONDS))
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

    /// The composer's button label while sending is impossible, or None when it can send.
    pub(super) fn connect_label(&self) -> Option<&'static str> {
        if self.run_pending || (self.connection.connected && self.connection.reconnect.is_none()) {
            None
        } else if self.connection.checking {
            Some("Connecting…")
        } else if self.connection.reconnect.is_some() {
            Some("Reconnect")
        } else {
            Some("Connect")
        }
    }

    /// Connect from the composer and send the waiting message once connected.
    pub(super) fn connect_from_composer(&mut self) {
        self.send_blocked = None;
        self.send_after_connect = !self.composer.trim().is_empty();
        if self.connection.reconnect.is_some() {
            self.retry_connection();
        } else {
            self.refresh_models(true);
        }
    }

    /// After a Connect from the composer: send the message, or stop waiting if it failed.
    pub(super) fn finish_connect_and_send(&mut self) {
        if !self.send_after_connect || self.connection.checking {
            return;
        }
        self.send_after_connect = false;
        if self.connection.connected {
            self.send_message();
        }
    }

    /// A connection the user made has dropped: say so and start retrying in the background.
    pub(super) fn connection_lost(&mut self, reason: String) {
        let explicit = self.connection.connected || self.connection.reconnect.is_some();
        let attempt = self
            .connection
            .reconnect
            .as_ref()
            .map_or(0, |reconnect| reconnect.attempt);
        self.connection.disconnect();
        self.connection.error = Some(reason);
        if explicit {
            self.connection.reconnect = Some(Reconnect {
                attempt,
                next_at: std::time::Instant::now() + retry_delay(attempt),
            });
            // The header banner explains the loss; clear older notices that might contradict it.
            self.notice = None;
        }
    }

    /// Runs every frame: quiet health checks while connected, and due reconnection attempts.
    pub(super) fn tick_connection(&mut self, context: &egui::Context) {
        if self.run_pending || self.connection.checking || self.connection.probing {
            return;
        }
        if let Some(reconnect) = &self.connection.reconnect {
            if std::time::Instant::now() >= reconnect.next_at {
                self.retry_connection();
            }
            context.request_repaint_after(std::time::Duration::from_millis(500));
        } else if self.connection.connected {
            if self.connection.checked_at.elapsed() >= HEALTH_INTERVAL {
                self.probe_connection();
            }
            context.request_repaint_after(HEALTH_INTERVAL);
        }
    }

    /// Try to reconnect now; keeps the retry state so failures back off.
    pub(super) fn retry_connection(&mut self) {
        let Some(mut reconnect) = self.connection.reconnect.take() else {
            return;
        };
        reconnect.attempt += 1;
        let error = self.connection.error.take();
        self.refresh_models(true);
        if !self.connection.checking {
            // The URL itself is invalid; retrying cannot help.
            return;
        }
        self.connection.error = error;
        self.connection.reconnect = Some(reconnect);
    }

    fn probe_connection(&mut self) {
        let Ok(url) = normalize_base_url(&self.settings.lm_studio_url) else {
            return;
        };
        self.connection.checked_at = std::time::Instant::now();
        self.connection.probing = true;
        self.command(AppCommand::RefreshModels {
            request_id: self.connection.request_id,
            base_url: url,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
    }

    pub(super) fn models_loaded(&mut self, request_id: u64, result: Result<Vec<String>, String>) {
        if request_id != self.connection.request_id {
            return;
        }
        if self.connection.probing && !self.connection.checking {
            self.connection.probing = false;
            match result {
                Ok(models) if !models.is_empty() => self.models = models,
                Ok(_) => {}
                Err(error) => self.connection_lost(error),
            }
            return;
        }
        if !self.connection.checking {
            return;
        }
        self.connection.checking = false;
        self.connection.checked_at = std::time::Instant::now();
        if let Some(reconnect) = self.connection.reconnect.take() {
            match result {
                Ok(models) => {
                    self.models = models;
                    self.connection.available = true;
                    self.connection.connected = true;
                    self.connection.error = None;
                    self.notice = Some("Reconnected to the model server.".to_owned());
                }
                Err(error) => {
                    self.connection.error = Some(error);
                    self.connection.reconnect = Some(Reconnect {
                        next_at: std::time::Instant::now() + retry_delay(reconnect.attempt),
                        attempt: reconnect.attempt,
                    });
                }
            }
            self.connection.connect_requested = false;
            return;
        }
        match result {
            Ok(models) => {
                self.connection.available = true;
                self.connection.connected |= self.connection.connect_requested;
                self.models = models;
                // Preserve an exact model ID even if discovery doesn't list it.
                if self.settings.selected_model.is_none() {
                    self.settings.selected_model = default_model(&self.models);
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
