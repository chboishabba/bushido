//! Explicit debug fixture native-code load boundary. The supplied hash pins
//! bytes selected by the operator; it does NOT establish a trustworthy build
//! or prevent same-UID mutation between validation and WebKit dlopen.
use sha2::{Digest,Sha256};
use std::{io::Read, os::unix::fs::MetadataExt, path::{Path,PathBuf}};

const NAME: &str = "libbushido-scope-fixture.so";
const LIMIT: u64 = 8 * 1024 * 1024;

pub(crate) struct ExtensionFixture { directory:PathBuf, sha256:String }

impl ExtensionFixture {
    pub(crate) fn from_environment(enabled:bool)->Result<Option<Self>,String> {
        match (std::env::var_os("BUSHIDO_LINUX_EXTENSION_FIXTURE_DIR"),
               std::env::var_os("BUSHIDO_LINUX_EXTENSION_FIXTURE_SHA256")) {
            (None,None)=>Ok(None),
            (Some(dir),Some(hash)) if enabled && cfg!(debug_assertions) =>
                Self::checked(Path::new(&dir),hash.to_str().ok_or("Invalid extension hash")?).map(Some),
            _=>Err("Extension requires explicit debug egress scope and both settings".into()),
        }
    }

    fn checked(dir:&Path, hash:&str)->Result<Self,String> {
        if hash.len()!=64 || !hash.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err("Expected lowercase SHA-256".into());
        }
        if !dir.is_absolute() || dir.canonicalize().ok().as_deref()!=Some(dir) {
            return Err("Canonical absolute extension directory required".into());
        }
        let m=std::fs::symlink_metadata(dir).map_err(|_|"Extension directory missing")?;
        if !m.is_dir() || m.uid()!=unsafe{libc::geteuid()} || m.mode()&0o277!=0 {
            return Err("Extension directory must be owned, private and nonwritable".into());
        }
        let paths=std::fs::read_dir(dir).map_err(|_|"Cannot inspect extension directory")?
            .collect::<Result<Vec<_>,_>>().map_err(|_|"Cannot enumerate extension directory")?;
        if paths.len()!=1 || paths[0].file_name()!=NAME {
            return Err("Only the single fixture library may be present".into());
        }
        let path=dir.join(NAME);
        let m=std::fs::symlink_metadata(&path).map_err(|_|"Extension library missing")?;
        if !m.is_file() || m.uid()!=unsafe{libc::geteuid()} || m.mode()&0o277!=0
            || m.nlink()!=1 || m.len()==0 || m.len()>LIMIT {
            return Err("Extension library must be owned, private, nonwritable and bounded".into());
        }
        let file=std::fs::File::open(path).map_err(|_|"Cannot open extension library")?;
        let mut bytes=Vec::new();
        file.take(LIMIT+1).read_to_end(&mut bytes).map_err(|_|"Cannot read extension library")?;
        if bytes.len() as u64!=m.len() || !bytes.starts_with(b"\x7fELF") ||
            format!("{:x}",Sha256::digest(&bytes))!=hash {
            return Err("Extension byte identity mismatch".into());
        }
        Ok(Self{directory:dir.to_owned(),sha256:hash.to_owned()})
    }

    pub(crate) fn path(&self)->Result<PathBuf,String> {
        Self::checked(&self.directory,&self.sha256).map(|f|f.directory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn exact_single_library_hash_and_permissions() {
        let root=std::env::temp_dir().join(format!("bushido-extension-test-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let file=root.join(NAME);
        let bytes=b"\x7fELFsynthetic-not-loadable";
        std::fs::write(&file,bytes).unwrap();
        std::fs::set_permissions(&file,std::fs::Permissions::from_mode(0o500)).unwrap();
        std::fs::set_permissions(&root,std::fs::Permissions::from_mode(0o500)).unwrap();
        let hash=format!("{:x}",Sha256::digest(bytes));
        assert!(ExtensionFixture::checked(&root,&hash).is_ok());
        assert!(ExtensionFixture::checked(&root,&"0".repeat(64)).is_err());
        std::fs::set_permissions(&root,std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(ExtensionFixture::checked(&root,&hash).is_err());
        std::fs::write(root.join("extra.so"),bytes).unwrap();
        std::fs::set_permissions(&root,std::fs::Permissions::from_mode(0o500)).unwrap();
        assert!(ExtensionFixture::checked(&root,&hash).is_err());
        std::fs::set_permissions(&root,std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
