// Copyright 2020 Allan Saddi <allan@saddi.com>
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::path::Path;
use std::io::prelude::*;
use std::fs::{File, rename, write};

use anyhow::{Context, Result};
use sha2::{Sha256, Digest};

const BACKUP_SUFFIX: &str = "~";

type MyHash = Sha256;

fn hash_file(path: &Path) -> Result<Option<Vec<u8>>> {
    let mut hash: Option<Vec<u8>> = None;

    if let Ok(mut f) = File::open(path) {
        // Read the file
        // FIXME Break it up into smaller chunks?
        let mut contents = Vec::new();
        let len = f.read_to_end(&mut contents)?;

        // And hash it
        let mut hasher = MyHash::new();
        hasher.input(&contents[0..len]);

        let mut h = Vec::new();
        h.extend_from_slice(&hasher.result()[..]);
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

pub fn output(path: &Path, contents: &[u8], verbosity: u8) -> Result<()> {
    if let Some(hash) = hash_file(path).with_context(|| format!("Error hashing file {}", path.display()))? {
        // Hash contents
        let mut hasher = MyHash::new();
        hasher.input(contents);
        let content_hash = &hasher.result()[..];

        // If unchanged, do nothing
        if *content_hash == hash[..] {
            if verbosity > 0 { println!("File {} unchanged", path.display()); }
            return Ok(());
        }
    }

    backup_file(path)
        .with_context(|| format!("Error backing up file {}", path.display()))?;

    write(path, contents)?;
    Ok(())
}
