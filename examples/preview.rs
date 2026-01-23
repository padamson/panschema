// use warp::Filter; // Unused
use std::env;

#[tokio::main]
async fn main() {
    #[allow(clippy::unwrap_used)]
    let output_dir = env::current_dir().unwrap().join("target/doc-preview");

    // Serve files from target/doc-preview
    let routes = warp::fs::dir(output_dir.clone());

    println!("Preview serving at http://localhost:3030");
    println!("Serving files from: {}", output_dir.display());

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}
