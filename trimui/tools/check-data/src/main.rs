//! Read-only archive integrity check; no extraction tree or duplicate game data.
use fnv_bsa::Archive;
use fnv_inflate::Trailer;
use std::{path::PathBuf, time::Instant};

fn check(path: PathBuf) -> Result<bool, Box<dyn std::error::Error>> {
    let start = Instant::now();
    fnv_bsa::check(&path)?;
    let mut archive = Archive::open(&path)?;
    let mut records: Vec<_> = archive.entries().map(|e| (e.path(), e.record.clone())).collect();
    records.sort_by_key(|(_, r)| r.offset);
    let (mut bytes, mut errors, mut missing, mut mismatch, mut extra) = (0u64, 0, 0, 0, 0);
    println!("START {}: {} files", path.display(), records.len());
    for (i, (name, record)) in records.iter().enumerate() {
        match archive.extract_checked(record) {
            Ok((data, trailer)) => {
                bytes += data.len() as u64;
                match trailer {
                    Trailer::Verified => (),
                    Trailer::Missing => { missing += 1; println!("MISSING_TRAILER {name}"); }
                    Trailer::Mismatch { stored, computed } => {
                        mismatch += 1;
                        println!("CHECKSUM_MISMATCH {name}: {stored:08x} != {computed:08x}");
                    }
                    Trailer::Extra { bytes } => { extra += 1; println!("EXTRA_TRAILER {name}: {bytes}"); }
                }
            }
            Err(e) => { errors += 1; println!("DECODE_ERROR {name}: {e}"); }
        }
        if (i + 1) % 2000 == 0 { println!("PROGRESS {}: {}/{}", path.display(), i + 1, records.len()); }
    }
    println!("RESULT {}: files={} decoded_bytes={bytes} decode_errors={errors} checksum_mismatches={mismatch} missing_trailers={missing} extra_trailers={extra} seconds={:.1}", path.display(), records.len(), start.elapsed().as_secs_f32());
    Ok(errors == 0 && mismatch == 0)
}

fn main() {
    let paths: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() { eprintln!("Usage: nvf-check-data ARCHIVE.bsa ..."); std::process::exit(2); }
    let mut ok = true;
    for path in paths {
        match check(path.clone()) {
            Ok(good) => ok &= good,
            Err(e) => { eprintln!("ARCHIVE_ERROR {}: {e}", path.display()); ok = false; }
        }
    }
    if !ok { std::process::exit(1); }
}
