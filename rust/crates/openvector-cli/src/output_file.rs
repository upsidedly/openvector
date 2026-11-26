use std::{
    fs,
    io::{self, Write},
    path::Path,
};

pub(crate) fn check(path: &Path, overwrite: bool) -> io::Result<Option<fs::Permissions>> {
    match fs::symlink_metadata(path) {
        Ok(_) if !overwrite => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Output already exists. Use --force to replace it.",
        )),
        Ok(metadata) if metadata.is_file() => Ok(Some(metadata.permissions())),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Cannot overwrite {}: not a regular file.", path.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn write(path: &Path, bytes: &[u8], overwrite: bool) -> io::Result<()> {
    let permissions = check(path, overwrite)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut builder = tempfile::Builder::new();
    builder.prefix(".openvector-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let mut temporary = builder.tempfile_in(parent)?;
    temporary.write_all(bytes)?;
    if let Some(permissions) = permissions {
        #[cfg(unix)]
        let permissions = {
            use std::os::unix::fs::PermissionsExt;
            fs::Permissions::from_mode(permissions.mode() & 0o777)
        };
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.as_file().sync_all()?;
    let result = if overwrite {
        temporary.persist(path)
    } else {
        temporary.persist_noclobber(path)
    };
    result.map(|_| ()).map_err(|error| error.error)
}
