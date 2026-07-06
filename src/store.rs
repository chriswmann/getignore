use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::errors::AppError;
use crate::github::{Index, build_index, load_from_github};
use crate::options::Opts;

fn save_cache(cache_file: &Path, index: &Index) -> Result<(), AppError> {
    let json = serde_json::to_string_pretty(index)?;
    fs::write(cache_file, json)?;
    Ok(())
}
pub fn load_cache(cache_file: &Path) -> Result<Index, AppError> {
    let file = fs::File::open(cache_file)?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader)?)
}

pub fn unix_now() -> Result<u64, AppError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

pub fn is_not_stale(index: &Index, ttl: Duration, now: u64) -> bool {
    now.saturating_sub(index.fetched_at) < ttl.as_secs()
}

pub fn fetch_and_cache(opts: &Opts, cache_file: &Path, now: u64) -> Result<Index, AppError> {
    let response = load_from_github(opts)?;
    let index = build_index(response, now)?;
    save_cache(cache_file, &index)?;
    Ok(index)
}
