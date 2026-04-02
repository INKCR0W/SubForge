use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use app_http_server::{ApiEvent, ServerContext, build_router as build_http_router};
use app_secrets::{
    EnvSecretStore, FileSecretStore, KeyringSecretStore, MemorySecretStore, SecretStore,
};
use app_storage::Database;
use clap::Parser;

use crate::cli::{
    APP_VERSION, CheckArgs, Cli, Command, DEFAULT_DB_FILE_NAME, DEFAULT_HOST, DEFAULT_PORT,
    DEFAULT_SECRETS_FILE_NAME, GuiBootstrap, RefreshArgs, RunArgs, SecretBackendKind,
    SecretStoreArgs,
};
use crate::security::{
    acquire_single_instance_lock, ensure_data_dir, is_loopback_host, load_or_create_admin_token,
    resolve_data_dir, set_owner_only_file_permissions,
};
use crate::settings_seed::seed_default_settings;

pub(crate) async fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Run(RunArgs {
        host: DEFAULT_HOST.to_string(),
        port: DEFAULT_PORT,
        gui_mode: false,
        data_dir: None,
        secrets: SecretStoreArgs {
            secrets_backend: SecretBackendKind::Keyring,
            secret_key: None,
            secrets_file: None,
        },
    })) {
        Command::Run(args) => run_server(args).await,
        Command::Check(args) => run_check(args),
        Command::Refresh(args) => run_refresh(args),
        Command::Version => {
            println!("subforge-core {APP_VERSION}");
            Ok(())
        }
    }
}

async fn run_server(args: RunArgs) -> Result<()> {
    let data_dir = resolve_data_dir(args.data_dir.clone())?;
    ensure_data_dir(&data_dir)?;
    let lock_file = acquire_single_instance_lock(&data_dir)?;
    let admin_token = load_or_create_admin_token(&data_dir)?;
    let database = initialize_database(&data_dir)?;
    let (secret_backend, secret_store) = initialize_secret_store(&args.secrets, &data_dir)?;

    seed_default_settings(database.as_ref(), &args)?;

    if !is_loopback_host(&args.host) {
        eprintln!(
            "WARNING: 当前监听地址为 {}，这不是回环地址，请确认安全风险。",
            args.host
        );
    }

    let (event_sender, _event_receiver) = tokio::sync::broadcast::channel::<ApiEvent>(256);
    let app = build_http_router(ServerContext::new(
        admin_token.clone(),
        Arc::clone(&database),
        Arc::clone(&secret_store),
        data_dir.join("plugins"),
        args.port,
        event_sender,
    ));

    if args.gui_mode {
        let bootstrap = GuiBootstrap {
            version: APP_VERSION,
            listen_addr: args.host.clone(),
            port: args.port,
            admin_token: admin_token.clone(),
            secrets_backend: secret_backend.as_str(),
        };
        let json = serde_json::to_string(&bootstrap)?;
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{json}")?;
        stdout.flush()?;
    }

    let socket: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .with_context(|| format!("无效监听地址: {}:{}", args.host, args.port))?;
    let listener = tokio::net::TcpListener::bind(socket).await?;

    println!(
        "SubForge Core 已启动: http://{}:{}（secrets backend: {}）",
        args.host,
        args.port,
        secret_backend.as_str()
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    drop(lock_file);
    Ok(())
}

fn run_check(args: CheckArgs) -> Result<()> {
    let data_dir = resolve_data_dir(args.data_dir)?;
    ensure_data_dir(&data_dir)?;
    load_or_create_admin_token(&data_dir)?;
    let database = initialize_database(&data_dir)?;
    initialize_secret_store(&args.secrets, &data_dir)?;
    seed_default_settings(
        database.as_ref(),
        &RunArgs {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            gui_mode: false,
            data_dir: Some(data_dir.clone()),
            secrets: args.secrets.clone(),
        },
    )?;
    println!(
        "配置检查通过，数据目录: {}，密钥后端: {}",
        data_dir.display(),
        args.secrets.secrets_backend.as_str()
    );
    Ok(())
}

pub(crate) fn run_refresh(args: RefreshArgs) -> Result<()> {
    if let Some(source_id) = args.source_id {
        println!("收到手动刷新请求，来源: {source_id}（功能将在后续阶段实现）");
    } else {
        println!("收到全量刷新请求（功能将在后续阶段实现）");
    }
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("WARNING: 注册 SIGTERM 监听失败: {err:#}");
                let _ = tokio::signal::ctrl_c().await;
                println!("收到退出信号，正在优雅关闭...");
                return;
            }
        };

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }

    println!("收到退出信号，正在优雅关闭...");
}

fn initialize_database(data_dir: &Path) -> Result<Arc<Database>> {
    let database_path = data_dir.join(DEFAULT_DB_FILE_NAME);
    let database = Database::open(&database_path)
        .with_context(|| format!("初始化数据库失败: {}", database_path.display()))?;
    set_owner_only_file_permissions(&database_path)?;
    Ok(Arc::new(database))
}

fn initialize_secret_store(
    args: &SecretStoreArgs,
    data_dir: &Path,
) -> Result<(SecretBackendKind, Arc<dyn SecretStore>)> {
    let backend = args.secrets_backend;
    let store: Arc<dyn SecretStore> = match backend {
        SecretBackendKind::Keyring => Arc::new(KeyringSecretStore::new()),
        SecretBackendKind::Env => Arc::new(EnvSecretStore::new()),
        SecretBackendKind::Memory => Arc::new(MemorySecretStore::new()),
        SecretBackendKind::File => {
            let secret_key = args
                .secret_key
                .clone()
                .or_else(|| std::env::var("SUBFORGE_SECRET_KEY").ok())
                .ok_or_else(|| {
                    anyhow!("file 密钥后端需要 --secret-key 或环境变量 SUBFORGE_SECRET_KEY")
                })?;
            let secrets_file = args
                .secrets_file
                .clone()
                .unwrap_or_else(|| data_dir.join(DEFAULT_SECRETS_FILE_NAME));
            let store = FileSecretStore::new(&secrets_file, secret_key)
                .with_context(|| format!("初始化 file 密钥后端失败: {}", secrets_file.display()))?;
            Arc::new(store)
        }
    };

    Ok((backend, store))
}
