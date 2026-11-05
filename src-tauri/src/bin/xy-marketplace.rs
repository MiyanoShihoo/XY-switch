use std::{io::{IsTerminal, Read}, process::ExitCode};
use xy_switch_lib::marketplace::{self, Marketplace, MarketplaceConfig, MarketplaceError};
use zeroize::Zeroizing;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xy-marketplace: {}",error.code());
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(),MarketplaceError> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.as_slice() == ["--hash-password-stdin"] {
        if std::io::stdin().is_terminal() { return Err(MarketplaceError::InvalidRequest); }
        let mut bytes = Zeroizing::new(Vec::new());
        std::io::stdin().take(1024).read_to_end(&mut bytes).map_err(|_| MarketplaceError::InvalidRequest)?;
        let password = Zeroizing::new(String::from_utf8(bytes.to_vec()).map_err(|_| MarketplaceError::InvalidRequest)?);
        let hash = marketplace::hash_password_for_deployment(password.trim_end_matches(['\r','\n']))?;
        println!("{}",serde_json::to_string(&hash).map_err(|_| MarketplaceError::Unavailable)?);
        return Ok(());
    }
    if !arguments.is_empty() && arguments.as_slice() != ["--check-config"] && arguments.as_slice() != ["--verify-state"] {
        return Err(MarketplaceError::InvalidRequest);
    }
    let config = MarketplaceConfig::from_env()?;
    if arguments.as_slice() == ["--verify-state"] {
        Marketplace::verify_database(config)?;
        println!("marketplace stored packages, approvals, release signatures and withdrawal links verified; no listener started");
        return Ok(());
    }
    if arguments.as_slice() == ["--check-config"] {
        println!("marketplace configuration validated; no listener started");
        return Ok(());
    }
    let listen = config.listen;
    let market = Marketplace::open(config)?;
    let listener = tokio::net::TcpListener::bind(listen).await.map_err(|_| MarketplaceError::Unavailable)?;
    eprintln!("xy-marketplace: ready");
    axum::serve(listener,marketplace::router(market)).with_graceful_shutdown(shutdown()).await.map_err(|_| MarketplaceError::Unavailable)
}

async fn shutdown() {
    let interrupt = async { let _ = tokio::signal::ctrl_c().await; };
    #[cfg(unix)]
    {
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut signal) => { signal.recv().await; },
                Err(_) => std::future::pending::<()>().await,
            }
        };
        tokio::select! { _ = interrupt => {}, _ = terminate => {} }
    }
    #[cfg(not(unix))]
    interrupt.await;
}
