use std::fs;
use std::io::copy;
use std::path::Path;

fn extract_from_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|e| e.to_string())?;
    let tar = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        if entry.header().entry_type() != tar::EntryType::Regular {
            continue;
        }
        let path = entry.path().map_err(|e| e.to_string())?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
        
        let looks_like_binary = name == "cliproxyapi"
            || name == "cli-proxy-api"
            || name.starts_with("cliproxyapi");

        if looks_like_binary {
            let mut out = fs::File::create(dest).map_err(|e| e.to_string())?;
            copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err("Not found".to_string())
}

fn main() {
    let archive = Path::new("test.tar.gz");
    let dest = Path::new("extracted_binary");
    println!("{:?}", extract_from_tar_gz(archive, dest));
}
