use std::{
    env,
    fs::File,
    io::{BufReader, Read},
    path::PathBuf,
    time::{Duration, Instant},
};

use flate2::read::MultiGzDecoder;

use backend::{Database, api, frontend, images};
use tower_http::services::ServeDir;

fn memory_mb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    Some(line.split_whitespace().nth(1)?.parse::<u64>().ok()? / 1024)
}

/// Counts bytes read from the (compressed) file and periodically prints progress and RAM use.
struct Progress<R> {
    inner: R,
    total: u64,
    read: u64,
    last_report: Instant,
}

impl<R> Progress<R> {
    fn report(&self) {
        let percent = if self.total == 0 {
            100.0
        } else {
            self.read as f64 * 100.0 / self.total as f64
        };
        eprint!(
            "\rLoading card database: {percent:5.1}% ({} / {} MB) | RAM: {} MB   ",
            self.read / 1_048_576,
            self.total / 1_048_576,
            memory_mb().map_or_else(|| "n/a".into(), |mb| mb.to_string()),
        );
    }
}

impl<R: Read> Read for Progress<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        if n == 0 || self.last_report.elapsed() >= Duration::from_millis(500) {
            self.report();
            self.last_report = Instant::now();
        }
        Ok(n)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::var("CARD_DATABASE").map_err(
        |_| "Set CARD_DATABASE to the path of a local Scryfall All Cards JSONL(.gz) export.",
    )?;
    let file = File::open(&path)?;
    let total = file.metadata()?.len();
    let file = BufReader::new(Progress {
        inner: file,
        total,
        read: 0,
        last_report: Instant::now(),
    });
    let reader: Box<dyn Read> = if path.ends_with(".gz") {
        Box::new(BufReader::new(MultiGzDecoder::new(file)))
    } else {
        Box::new(file)
    };
    let database = Database::from_reader(reader)?;
    eprintln!();
    let image_cache = env::var_os("IMAGE_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/images"));
    let stored = tokio::task::block_in_place(|| images::sync(&database, &image_cache))?;
    println!("Image cache up to date ({stored} image(s) stored).");
    let address = env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3001".into());
    let app = api(database)
        .nest_service("/images", ServeDir::new(image_cache))
        .fallback(frontend::serve);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!(
        "Card translator listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, app).await?;
    Ok(())
}
