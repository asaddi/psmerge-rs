use std::path::Path;
use std::io::prelude::*;
use std::fs::{File, rename, write};

use anyhow::{Context, Result};
use sha2::{Sha256, Digest};

const BUFFER_SIZE: usize = 10240;
const BACKUP_SUFFIX: &str = "~";

type MyHash = Sha256;

fn hash_file<D: Digest + Default>(path: &Path) -> Result<Option<Vec<u8>>> {
    let mut hash: Option<Vec<u8>> = None;

    if let Ok(mut f) = File::open(path) {
        // Read the file & hash it
        let mut contents: Vec<u8> = vec![0u8; BUFFER_SIZE];

        let mut hasher = D::new();

        loop {
            let len = f.read(&mut contents)?;
            hasher.update(&contents[..len]);

            if len == 0 || len < BUFFER_SIZE {
                break;
            }
        }

        let mut h = Vec::new();
        h.extend_from_slice(&hasher.finalize()[..]);
        hash = Some(h);
    }
    // TODO Possible to check if error is because it doesn't exist?

    Ok(hash)
}

fn backup_file(path: &Path) -> Result<()> {
    if let Ok(f) = File::open(path) {
        drop(f); // FIXME Does this actually close the file?

        // FIXME This is pretty shady. Possible to do entirely in Path/PathBuf?
        let mut backup_path: String = (*path.to_str().unwrap()).to_owned();
        backup_path.push_str(BACKUP_SUFFIX);

        rename(path, backup_path)?;
    }
    // TODO Possible to check if error is because it doesn't exist?

    Ok(())
}

pub fn output(path: &Path, contents: &[u8], nobackup: bool, verbosity: u8) -> Result<()> {
    if let Some(hash) = hash_file::<MyHash>(path).with_context(|| format!("Error hashing file {}", path.display()))? {
        // Hash contents
        let mut hasher = MyHash::new();
        hasher.update(contents);
        let content_hash = &hasher.finalize()[..];

        // If unchanged, do nothing
        if *content_hash == hash[..] {
            if verbosity > 0 { println!("File {} unchanged", path.display()); }
            return Ok(());
        }
    }

    if !nobackup {
        backup_file(path)
            .with_context(|| format!("Error backing up file {}", path.display()))?;
    }

    write(path, contents)?;
    Ok(())
}
