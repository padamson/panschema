use std::path::{Path, PathBuf};

use axum::Router;
use notify::{Event, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tower_http::services::ServeDir;
use tower_livereload::LiveReloadLayer;

use panschema::io::FormatRegistry;

/// The `host:port` the dev server binds. Defaults to loopback
/// (`127.0.0.1`) so generated docs aren't exposed to the local network;
/// `external == true` opts into `0.0.0.0` (all interfaces) when the user
/// explicitly asks for it.
fn bind_address(external: bool, port: u16) -> String {
    let host = if external { "0.0.0.0" } else { "127.0.0.1" };
    format!("{host}:{port}")
}

/// Regenerate documentation from input ontology
fn regenerate(input: &Path, output: &Path) -> anyhow::Result<()> {
    let registry = FormatRegistry::with_defaults();

    let reader = registry
        .reader_for_path(input)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let schema = reader.read(input).map_err(|e| anyhow::anyhow!("{}", e))?;

    let writer = registry
        .writer_for_format("html")
        .ok_or_else(|| anyhow::anyhow!("HTML writer not found"))?;
    writer
        .write(&schema, output)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(())
}

/// Start the development server with hot reload
// `#[mutants::skip]`: this awaits `axum::serve` forever, so it has no
// unit-testable return path (a test can't await it without spawn +
// timeout + cancel). The one piece of decision logic — the bind address
// — is extracted into `bind_address`, which is unit-tested.
#[mutants::skip]
pub async fn serve(input: &Path, output: &Path, port: u16, external: bool) -> anyhow::Result<()> {
    // Generate initial documentation
    regenerate(input, output)?;
    println!("Generated initial documentation in {}", output.display());

    // Create channel for file change notifications
    let (tx, mut rx) = mpsc::channel::<()>(1);

    // Set up file watcher
    let input_clone = input.to_path_buf();
    let output_clone = output.to_path_buf();
    let tx_clone = tx.clone();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
        if let Ok(event) = res
            && (event.kind.is_modify() || event.kind.is_create())
        {
            // Notify the regeneration task
            let _ = tx_clone.blocking_send(());
        }
    })?;

    // Watch the input file's parent directory
    let watch_path = input
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

    // Spawn regeneration task
    let input_for_regen = input_clone.clone();
    let output_for_regen = output_clone.clone();
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            // Debounce: wait a bit for rapid changes to settle
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            // Drain any additional notifications
            while rx.try_recv().is_ok() {}

            match regenerate(&input_for_regen, &output_for_regen) {
                Ok(()) => println!("Regenerated documentation"),
                Err(e) => eprintln!("Error regenerating: {e}"),
            }
        }
    });

    // Create live reload layer
    let livereload = LiveReloadLayer::new();
    let reloader = livereload.reloader();

    // Set up file watcher for output directory to trigger browser reload
    let mut output_watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
        if let Ok(event) = res
            && (event.kind.is_modify() || event.kind.is_create())
        {
            reloader.reload();
        }
    })?;
    output_watcher.watch(output, RecursiveMode::Recursive)?;

    // Build the router
    let app = Router::new()
        .fallback_service(ServeDir::new(output))
        .layer(livereload);

    let addr = bind_address(external, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    println!("Development server running at http://{addr}");
    println!("Watching {} for changes...", input.display());
    println!("Press Ctrl+C to stop");

    // Keep watchers alive
    let _watcher = watcher;
    let _output_watcher = output_watcher;

    axum::serve(listener, app).await?;

    Ok(())
}

/// Start a simple static file server with live reload (no input file watching).
// `#[mutants::skip]`: same untestable-server-loop rationale as `serve`.
#[cfg(feature = "dev")]
#[mutants::skip]
pub async fn serve_static(output: &Path, port: u16) -> anyhow::Result<()> {
    // Create live reload layer
    let livereload = LiveReloadLayer::new();
    let reloader = livereload.reloader();

    // Set up file watcher for output directory to trigger browser reload
    let mut output_watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
        if let Ok(event) = res
            && (event.kind.is_modify() || event.kind.is_create())
        {
            reloader.reload();
        }
    })?;
    output_watcher.watch(output, RecursiveMode::Recursive)?;

    // Build the router
    let app = Router::new()
        .fallback_service(ServeDir::new(output))
        .layer(livereload);

    // Styleguide preview is a local dev tool; always loopback.
    let addr = bind_address(false, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    println!("Server running at http://{addr}");
    println!("Watching {} for changes...", output.display());
    println!("Press Ctrl+C to stop");

    // Keep watcher alive
    let _output_watcher = output_watcher;

    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::bind_address;

    #[test]
    fn defaults_to_loopback() {
        assert_eq!(bind_address(false, 3000), "127.0.0.1:3000");
    }

    #[test]
    fn external_opt_in_binds_all_interfaces() {
        assert_eq!(bind_address(true, 8080), "0.0.0.0:8080");
    }
}
