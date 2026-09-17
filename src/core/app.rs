use log::{error, info, warn};

use crate::core::cli::{
    AppArgs, BitForgeConfig, LockManager, VERSION, run_add_layer, run_dependency_add,
    run_dependency_remove, run_init, run_remove_layer, run_update,
};
use crate::core::web_server;
use crate::core::{self};

pub async fn run(args: AppArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.version_requested {
        info!("BitForge {VERSION}");
        return Ok(());
    }

    if args.update_requested {
        run_update(args.beta)?;
        return Ok(());
    }

    if args.init_requested {
        run_init(
            &args.working_directory,
            args.init_dir_name.as_deref(),
            args.force,
            args.poky_version.as_deref(),
        )?;
        return Ok(());
    }

    if let Err(error) = core::tracking::require_git_project(&args.working_directory) {
        error!("{error}");
        std::process::exit(1);
    }

    if let Some(spec) = &args.dependency_add_spec {
        run_dependency_add(&args.working_directory, spec)?;
        return Ok(());
    }

    if let Some(name) = &args.dependency_remove_name {
        run_dependency_remove(&args.working_directory, name)?;
        return Ok(());
    }

    if let Some(name) = &args.add_layer_name {
        run_add_layer(&args.working_directory, name)?;
        return Ok(());
    }

    if let Some(name) = &args.remove_layer_name {
        run_remove_layer(&args.working_directory, name)?;
        return Ok(());
    }

    info!("BitForge started in {}", args.working_directory.display());

    if !BitForgeConfig::exists_in(&args.working_directory) {
        error!("No BitForge project config found in the directory run `BitForge --init`");
        return Ok(());
    }

    let config = BitForgeConfig::load_from(&args.working_directory)?;
    info!("Loaded project '{}'", config.display_name());

    core::workspace::ensure_project_layout(&args.working_directory)?;
    if let Err(error) = core::tracking::record_revision(&args.working_directory, "launch") {
        warn!("failed to record git revision: {error}");
    }
    core::conf::ensure_build_conf(&args.working_directory, &config)?;

    let bitbake_version = config
        .workspace
        .as_ref()
        .map(|workspace| workspace.bitbake.as_str())
        .filter(|version| !version.is_empty())
        .unwrap_or(core::cli::BITBAKE_VERSION);
    core::cli::ensure_bitbake(&args.working_directory, bitbake_version)?;

    let lock = LockManager::load(&args.working_directory)?;
    lock.flush()?;

    let build_target = if args.build_requested {
        let target = match args.build_target.clone() {
            Some(target) => target,
            None => match config.default_image() {
                Some(image) => image.to_string(),
                None => {
                    error!(
                        "No default image set. Add a [default] image to BitForge.toml or run `BitForge --build <image|layer>`"
                    );
                    std::process::exit(1);
                }
            },
        };
        Some(target)
    } else {
        None
    };

    let server_section = config.server_section();
    let server_settings = web_server::ServerSettings {
        host: server_section.host.clone(),
        port: server_section.port,
        shutdown_grace: std::time::Duration::from_secs(server_section.shutdown_grace_secs),
        activity_timeout: std::time::Duration::from_secs(server_section.activity_timeout * 60),
        http1_keep_alive: server_section.http1_keep_alive,
        http2_keep_alive: server_section
            .http2_keep_alive_secs
            .map(std::time::Duration::from_secs),
    };
    let server = web_server::spawn(args.working_directory.clone(), server_settings).await?;
    info!("BitForge is running at {}", server.url());

    if let Some(target) = build_target {
        match server.start_build(target.clone()) {
            Ok(_) => info!("Started build for '{target}'"),
            Err(error) => error!("failed to start build: {error}"),
        }
    } else {
        server.warm_up(config.default_image().map(str::to_string));
    }

    info!("Press Ctrl+C to exit BitForge");
    wait_for_shutdown_signal().await;

    info!("Shutting down BitForge (press Ctrl+C again to force exit)");
    tokio::select! {
        _ = server.shutdown() => info!("BitForge stopped"),
        _ = wait_for_shutdown_signal() => {
            warn!("Forced shutdown requested; exiting immediately");
            std::process::exit(130);
        }
    }

    Ok(())
}

async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = match signal(SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(error) => {
            warn!("failed to install SIGTERM handler: {error}");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    let mut interrupt = match signal(SignalKind::interrupt()) {
        Ok(stream) => stream,
        Err(error) => {
            warn!("failed to install SIGINT handler: {error}");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };

    tokio::select! {
        _ = terminate.recv() => info!("Received SIGTERM"),
        _ = interrupt.recv() => info!("Received SIGINT (Ctrl+C)"),
    }
}
