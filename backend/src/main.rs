use std::{env, fs::File, io::BufReader, path::PathBuf};

use backend::{Database, api};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::var("CARD_DATABASE")
        .map_err(|_| "Set CARD_DATABASE to the path of a local Scryfall All Cards JSON export.")?;
    let database = Database::from_reader(BufReader::new(File::open(&path)?))?;
    let address = env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3001".into());
    let frontend = env::var_os("FRONTEND_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../frontend/dist"));
    let app = api(database).fallback_service(ServeDir::new(frontend));
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!(
        "Card translator listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, app).await?;
    Ok(())
}
