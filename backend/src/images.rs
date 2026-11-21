use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use image::{ImageFormat, imageops::FilterType};

use crate::Database;

const WORKERS: usize = 8;

/// One image to cache: `index` is the card face (0 for single-faced cards).
pub struct ImageJob {
    pub oracle_id: String,
    pub index: usize,
    pub url: String,
}

fn image_path(dir: &Path, job: &ImageJob) -> PathBuf {
    dir.join(format!("{}-{}.jpg", job.oracle_id, job.index))
}

fn url_path(dir: &Path, job: &ImageJob) -> PathBuf {
    dir.join(format!("{}-{}.url", job.oracle_id, job.index))
}

/// Scryfall appends a timestamp to the image URL, so a changed URL means a changed image.
fn is_current(dir: &Path, job: &ImageJob) -> bool {
    image_path(dir, job).exists()
        && fs::read_to_string(url_path(dir, job)).is_ok_and(|stored| stored == job.url)
}

fn download(client: &reqwest::blocking::Client, job: &ImageJob) -> Result<Vec<u8>, String> {
    let bytes = client
        .get(&job.url)
        .send()
        .and_then(|response| response.error_for_status())
        .and_then(|response| response.bytes())
        .map_err(|error| error.to_string())?;
    let image = image::load_from_memory_with_format(&bytes, ImageFormat::Jpeg)
        .map_err(|error| error.to_string())?;
    let half = image.resize_exact(
        (image.width() / 2).max(1),
        (image.height() / 2).max(1),
        FilterType::Lanczos3,
    );
    let mut out = Cursor::new(Vec::new());
    half.write_to(&mut out, ImageFormat::Jpeg)
        .map_err(|error| error.to_string())?;
    Ok(out.into_inner())
}

fn store(dir: &Path, job: &ImageJob, jpeg: &[u8]) -> std::io::Result<()> {
    fs::write(image_path(dir, job), jpeg)?;
    fs::write(url_path(dir, job), &job.url)
}

/// Stores every image at half resolution in `dir`, skipping images whose URL is unchanged.
/// Prints each stored image URL to the console and returns the number of stored images.
pub fn sync(database: &Database, dir: &Path) -> std::io::Result<usize> {
    fs::create_dir_all(dir)?;
    let jobs: Vec<_> = database
        .image_jobs()
        .into_iter()
        .filter(|job| !is_current(dir, job))
        .collect();
    println!(
        "Image cache {}: {} image(s) to download",
        dir.display(),
        jobs.len()
    );
    let client = reqwest::blocking::Client::builder()
        .user_agent("mtg_translator/0.1")
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(std::io::Error::other)?;
    let next = AtomicUsize::new(0);
    let stored = AtomicUsize::new(0);
    let console = Mutex::new(());
    thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                while let Some(job) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let result = download(&client, job)
                        .and_then(|jpeg| store(dir, job, &jpeg).map_err(|e| e.to_string()));
                    let _guard = console.lock().unwrap();
                    match result {
                        Ok(()) => {
                            stored.fetch_add(1, Ordering::Relaxed);
                            println!("Saved image {}", job.url);
                        }
                        Err(error) => eprintln!("Failed image {}: {error}", job.url),
                    }
                }
            });
        }
    });
    Ok(stored.into_inner())
}
