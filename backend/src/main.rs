use std::{
    env,
    fs::File,
    io::{BufReader, Read},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use flate2::read::MultiGzDecoder;

use backend::{
    Database, api, crawlers, frontend, images,
    limiter::{self, IpLimiter},
    logging,
};
use tower_http::services::ServeDir;

/// Accepts a file, or a directory from which the newest `.jsonl.gz` (else `.jsonl`) is chosen.
fn resolve_database(path: PathBuf) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Ok(path);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&path)
        .map_err(|error| format!("Cannot read {}: {error}", path.display()))?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|file| {
            let name = file.to_string_lossy();
            name.ends_with(".jsonl.gz") || name.ends_with(".jsonl")
        })
        .collect();
    files.sort_by_key(|file| (file.to_string_lossy().ends_with(".gz"), file.clone()));
    files
        .pop()
        .ok_or_else(|| format!("No .jsonl.gz or .jsonl file found in {}", path.display()))
}

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

/// Start-up checks that fail fast, before the (long) database load.
fn preflight(database: &PathBuf, image_cache: &PathBuf, address: &str) -> Result<(), String> {
    let mut file = File::open(database)
        .map_err(|error| format!("Cannot open card database {}: {error}", database.display()))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size == 0 {
        return Err(format!("Card database {} is empty.", database.display()));
    }
    let mut head = [0u8; 2];
    file.read_exact(&mut head).map_err(|e| e.to_string())?;
    let gzip = database.extension().is_some_and(|ext| ext == "gz");
    if gzip && head != [0x1f, 0x8b] {
        return Err(format!("{} is not a gzip file.", database.display()));
    }
    if !gzip && head[0] != b'{' {
        return Err(format!(
            "{} does not look like JSON Lines (expected one JSON object per line); a JSON array is not supported.",
            database.display()
        ));
    }
    println!("[check] card database readable ({} MB)", size / 1_048_576);

    std::fs::create_dir_all(image_cache)
        .map_err(|e| format!("Cannot create image cache {}: {e}", image_cache.display()))?;
    let probe = image_cache.join(".write-test");
    std::fs::write(&probe, b"ok")
        .and_then(|()| std::fs::remove_file(&probe))
        .map_err(|e| format!("Image cache {} is not writable: {e}", image_cache.display()))?;
    println!("[check] image cache writable ({})", image_cache.display());

    std::net::TcpListener::bind(address).map_err(|e| format!("Cannot listen on {address}: {e}"))?;
    println!("[check] address {address} is available");

    let (files, has_index) = frontend::embedded();
    if has_index {
        println!("[check] frontend available ({files} files)");
    } else {
        eprintln!("[warn] frontend/dist was empty at build time; only the API will work.");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::var("CARD_DATABASE").map_err(
        |_| "Set CARD_DATABASE to the path of a local Scryfall All Cards JSONL(.gz) export.",
    )?;
    println!(
        "mtg_translator {} | pid {}",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );
    let path = resolve_database(PathBuf::from(path))?;
    let image_cache = env::var_os("IMAGE_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/images"));
    let address = env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3001".into());
    println!("[config] CARD_DATABASE={}", path.display());
    println!("[config] IMAGE_CACHE={}", image_cache.display());
    println!("[config] BIND_ADDRESS={address}");
    println!(
        "[config] max {} client IPs per {} s",
        limiter::MAX_CLIENT_IPS,
        limiter::WINDOW.as_secs()
    );
    preflight(&path, &image_cache, &address)?;
    let file = File::open(&path)
        .map_err(|error| format!("Cannot open card database {}: {error}", path.display()))?;
    let total = file.metadata()?.len();
    let file = BufReader::new(Progress {
        inner: file,
        total,
        read: 0,
        last_report: Instant::now(),
    });
    let reader: Box<dyn Read> = if path.extension().is_some_and(|ext| ext == "gz") {
        Box::new(BufReader::new(MultiGzDecoder::new(file)))
    } else {
        Box::new(file)
    };
    let started = Instant::now();
    let database = Database::from_reader(reader)
        .map_err(|error| format!("Cannot parse card database: {error}"))?;
    eprintln!();
    let stats = database.stats();
    println!(
        "[load] {} Oracle cards, {} printings, {} name keys in {:.1} s | RAM: {} MB",
        stats.oracle_cards,
        stats.printings,
        stats.name_keys,
        started.elapsed().as_secs_f64(),
        memory_mb().map_or_else(|| "n/a".into(), |mb| mb.to_string()),
    );
    println!("[load] languages: {}", stats.languages.join(", "));
    if stats.skipped_without_oracle_id > 0 {
        println!(
            "[load] skipped {} entries without oracle_id",
            stats.skipped_without_oracle_id
        );
    }
    if !stats.languages.iter().any(|lang| lang == "en") {
        return Err("The database contains no English printings.".into());
    }
    if stats.oracle_cards_without_english > 0 {
        eprintln!(
            "[warn] {} Oracle cards have no English printing and cannot be shown",
            stats.oracle_cards_without_english
        );
    }
    let image_start = Instant::now();
    let stored = tokio::task::block_in_place(|| images::sync(&database, &image_cache))?;
    println!(
        "[images] cache up to date ({stored} image(s) stored in {:.1} s)",
        image_start.elapsed().as_secs_f64()
    );
    let app = api(database)
        .nest_service("/images", ServeDir::new(image_cache))
        .fallback(frontend::serve)
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(IpLimiter::new(limiter::MAX_CLIENT_IPS, limiter::WINDOW)),
            limiter::enforce,
        ))
        .layer(axum::middleware::from_fn(crawlers::filter))
        .layer(axum::middleware::from_fn(logging::requests));
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!(
        "Card translator listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
