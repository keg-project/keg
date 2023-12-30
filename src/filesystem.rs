use std::fs;
use std::io;
use std::path::Path;

pub fn iterate<F>(add_var: bool, mut f: F) -> io::Result<()>
where
    F: FnMut(&Path, Option<&Path>), // F(file_name, symlink)
{
    let mut required = ["bin", "etc", "lib", "opt", "sbin", "usr"].as_ref();
    if add_var {
        required = &["bin", "etc", "lib", "opt", "sbin", "usr", "var"];
    }
    let optional = ["lib64"];
    for file_name in required {
        let rooted_file_name = Path::new("/").join(file_name);
        if let Ok(symlink) = fs::read_link(rooted_file_name) {
            // symlink
            f(Path::new(file_name), Some(symlink.as_path()));
        } else {
            f(Path::new(file_name), None);
        }
    }
    for file_name in optional {
        let rooted_file_name = Path::new("/").join(file_name);
        match fs::symlink_metadata(&rooted_file_name) {
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        }
        if let Ok(symlink) = fs::read_link(rooted_file_name) {
            // symlink
            f(Path::new(file_name), Some(symlink.as_path()));
        } else {
            f(Path::new(file_name), None);
        }
    }
    Ok(())
}
