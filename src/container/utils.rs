use std::ffi::OsString;
use std::io;
use std::path::Path;

pub fn ro_bind_subentries_keep_symlinks<S, D>(src: S, dest: D) -> io::Result<Vec<OsString>>
where
    S: AsRef<Path>,
    D: AsRef<Path>,
{
    let src: &Path = src.as_ref();
    let dest: &Path = dest.as_ref();
    let mut files: Vec<(OsString, Option<OsString>)> = Vec::new();
    for entry in src.read_dir()? {
        let entry = entry?;
        if !entry.file_type()?.is_symlink() {
            files.push((entry.file_name(), None));
        } else {
            files.push((entry.file_name(), Some(entry.path().read_link()?.into())));
        }
    }
    files.sort_unstable();

    let mut result = Vec::new();
    for (file_name, symlink) in files {
        match symlink {
            None => {
                result.push("--ro-bind".into());
                result.push(src.join(&file_name).into());
                result.push(dest.join(&file_name).into());
            }
            Some(symlink) => {
                result.push("--symlink".into());
                result.push(symlink.into());
                result.push(dest.join(&file_name).into());
            }
        }
    }
    Ok(result)
}
