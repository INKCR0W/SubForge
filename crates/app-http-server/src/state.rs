use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use app_common::{ErrorResponse, Profile, ProxyNode};
use app_secrets::SecretStore;
use app_storage::Database;
use app_transform::RoutingTemplateExportContext;
use axum::Json;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};

mod security_limits;

pub(crate) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const MAX_PLUGIN_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_ZIP_ENTRIES: usize = 100;
pub(crate) const MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES: u64 = 50 * 1024 * 1024;
pub(crate) const MANAGEMENT_RATE_LIMIT_PER_SECOND: u32 = 30;
pub(crate) const SUBSCRIPTION_RATE_LIMIT_PER_SECOND: u32 = 10;
pub(crate) const PROFILE_CACHE_TTL_SECONDS: u64 = 60;

pub(crate) use security_limits::{AuthFailures, RateLimiter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEvent {
    pub event: String,
    pub message: String,
    pub source_id: Option<String>,
    pub timestamp: String,
}

#[derive(Clone)]
pub struct ServerContext {
    pub(crate) admin_token: Arc<RwLock<String>>,
    pub(crate) admin_token_path: Arc<PathBuf>,
    pub(crate) database: Arc<Database>,
    pub(crate) secret_store: Arc<dyn SecretStore>,
    pub(crate) plugins_dir: PathBuf,
    pub(crate) host_validation: HostValidationState,
    pub(crate) event_sender: broadcast::Sender<ApiEvent>,
    pub(crate) shutdown_signal: watch::Sender<bool>,
    pub(crate) rate_limiter: Arc<RateLimiter>,
    pub(crate) auth_failures: Arc<AuthFailures>,
    pub(crate) profile_cache: Arc<ProfileCache>,
    pub(crate) source_userinfo_cache: Arc<SourceUserinfoCache>,
}

impl ServerContext {
    pub fn new(
        admin_token: String,
        admin_token_path: PathBuf,
        database: Arc<Database>,
        secret_store: Arc<dyn SecretStore>,
        plugins_dir: PathBuf,
        listen_host: &str,
        listen_port: u16,
        event_sender: broadcast::Sender<ApiEvent>,
    ) -> Self {
        let (shutdown_signal, _shutdown_receiver) = watch::channel(false);
        Self {
            admin_token: Arc::new(RwLock::new(admin_token)),
            admin_token_path: Arc::new(admin_token_path),
            database,
            secret_store,
            plugins_dir,
            host_validation: HostValidationState::new(listen_host, listen_port),
            event_sender,
            shutdown_signal,
            rate_limiter: Arc::new(RateLimiter::default()),
            auth_failures: Arc::new(AuthFailures::default()),
            profile_cache: Arc::new(ProfileCache::default()),
            source_userinfo_cache: Arc::new(SourceUserinfoCache::default()),
        }
    }

    pub fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown_signal.subscribe()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProfileCacheEntry {
    pub(crate) profile: Profile,
    pub(crate) source_ids: Vec<String>,
    pub(crate) nodes: Vec<ProxyNode>,
    pub(crate) generated_at: String,
    pub(crate) subscription_userinfo: Option<String>,
    pub(crate) routing_template_export_context: Option<RoutingTemplateExportContext>,
    cached_at: Instant,
}

impl ProfileCacheEntry {
    pub(crate) fn with_cached_at(
        profile: Profile,
        source_ids: Vec<String>,
        nodes: Vec<ProxyNode>,
        generated_at: String,
        subscription_userinfo: Option<String>,
        routing_template_export_context: Option<RoutingTemplateExportContext>,
    ) -> Self {
        Self {
            profile,
            source_ids,
            nodes,
            generated_at,
            subscription_userinfo,
            routing_template_export_context,
            cached_at: Instant::now(),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ProfileCache {
    inner: Mutex<HashMap<String, ProfileCacheEntry>>,
}

impl ProfileCache {
    pub(crate) fn get_fresh(&self, profile_id: &str, ttl: Duration) -> Option<ProfileCacheEntry> {
        let mut inner = self.inner.lock().ok()?;
        let now = Instant::now();
        let entry = inner.get(profile_id).cloned()?;
        if now.duration_since(entry.cached_at) <= ttl {
            Some(entry)
        } else {
            inner.remove(profile_id);
            None
        }
    }

    pub(crate) fn insert(&self, profile_id: &str, entry: ProfileCacheEntry) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.insert(profile_id.to_string(), entry);
        }
    }

    pub(crate) fn invalidate(&self, profile_id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.remove(profile_id);
        }
    }

    pub(crate) fn invalidate_many(&self, profile_ids: &[String]) {
        if let Ok(mut inner) = self.inner.lock() {
            for profile_id in profile_ids {
                inner.remove(profile_id);
            }
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct SourceUserinfoCache {
    inner: Mutex<HashMap<String, String>>,
}

impl SourceUserinfoCache {
    pub(crate) fn set(&self, source_id: &str, userinfo: Option<String>) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(value) = userinfo {
                inner.insert(source_id.to_string(), value);
            } else {
                inner.remove(source_id);
            }
        }
    }

    pub(crate) fn get(&self, source_id: &str) -> Option<String> {
        self.inner.lock().ok()?.get(source_id).cloned()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct HostValidationState {
    allowed_hosts: Arc<HashSet<String>>,
}

impl HostValidationState {
    pub(crate) fn new(listen_host: &str, port: u16) -> Self {
        Self::from_hosts(Self::build_allowed_hosts(listen_host, port))
    }

    pub(crate) fn from_hosts<I>(hosts: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        Self {
            allowed_hosts: Arc::new(hosts.into_iter().collect()),
        }
    }

    fn build_allowed_hosts(listen_host: &str, port: u16) -> HashSet<String> {
        let mut hosts = HashSet::new();
        for host in ["127.0.0.1", "localhost", "[::1]"] {
            hosts.insert(host.to_string());
            hosts.insert(format!("{host}:{port}"));
        }

        let normalized_listen_host = listen_host.trim().to_ascii_lowercase();
        match normalized_listen_host.as_str() {
            "" | "127.0.0.1" | "localhost" | "[::1]" => {}
            "0.0.0.0" | "::" | "[::]" => {
                for interface in if_addrs::get_if_addrs().unwrap_or_default() {
                    let ip = interface.ip().to_string().to_ascii_lowercase();
                    hosts.insert(ip.clone());
                    hosts.insert(format!("{ip}:{port}"));
                }
            }
            _ => {
                hosts.insert(normalized_listen_host.clone());
                hosts.insert(format!("{normalized_listen_host}:{port}"));
            }
        }

        hosts
    }

    pub(crate) fn is_allowed(&self, host_header: &str) -> bool {
        self.allowed_hosts.contains(host_header)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub(crate) status: &'static str,
    pub(crate) version: &'static str,
}

pub(crate) type ApiResult<T> = Result<(StatusCode, Json<T>), (StatusCode, Json<ErrorResponse>)>;
