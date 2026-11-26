use crate::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff"]
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn images(directory: &Path, recursive: bool) -> Result<Vec<String>> {
    let mut remaining = vec![directory.to_path_buf()];
    let mut paths: Vec<PathBuf> = Vec::new();
    while let Some(directory) = remaining.pop() {
        let describe = |error| format!("{}: {error}", directory.display());
        for entry in fs::read_dir(&directory).map_err(describe)? {
            let entry = entry.map_err(describe)?;
            if entry.file_name().as_encoded_bytes().starts_with(b".") {
                continue;
            }
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if kind.is_dir() && recursive {
                remaining.push(path);
            } else if kind.is_file() && supported_image(&path) {
                paths.push(path);
            }
        }
    }

    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            path.into_os_string().into_string().map_err(|path| {
                format!(
                    "{}: filename must be valid Unicode.",
                    path.to_string_lossy()
                )
            })
        })
        .collect()
}

pub(crate) fn expand(
    inputs: &[String],
    output_dir: Option<&str>,
    recursive: bool,
) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let mut empty_directory = None;
    for input in inputs {
        let path = Path::new(input);
        if !path.is_dir() {
            files.push(input.clone());
            continue;
        }
        let output_dir = output_dir.ok_or("Folder inputs require --output-dir <directory>.")?;
        if !Path::new(output_dir).is_dir() {
            return Err(format!(
                "--output-dir must be an existing folder: {output_dir}"
            ));
        }
        let discovered = images(path, recursive)?;
        if discovered.is_empty() {
            empty_directory = Some(input);
        }
        files.extend(discovered);
    }

    if let Some(directory) = empty_directory.filter(|_| files.is_empty()) {
        let hint = if recursive {
            ""
        } else {
            " Add --recursive to search subfolders."
        };
        return Err(format!("No supported images found in {directory}.{hint}"));
    }
    Ok(files)
}
