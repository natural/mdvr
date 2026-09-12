use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::contracts::{DocumentId, Generation, MAX_RESOURCE_BYTES, ResourceId};

pub const DEFAULT_LOCAL_RESOURCE_BYTES: u64 = MAX_RESOURCE_BYTES as u64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextError {
    Missing,
    Unreadable,
    NotADirectory,
    NotAFile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceDenied {
    Missing,
    Unreadable,
    NotAFile,
    OutsideRoot,
    TooLarge { bytes: u64, max: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceAuthorization {
    Allowed(ResourceId),
    Denied(ResourceDenied),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceReadError {
    UnknownResource,
    StaleContext,
    Missing,
    Unreadable,
    NotAFile,
    ResourceChanged,
    TooLarge { bytes: u64, max: u64 },
}

#[derive(Clone, Debug)]
struct Grant {
    path: PathBuf,
}

/// Native-only local resource authority.
///
/// It returns opaque IDs and keeps filesystem paths private. Callers can read
/// only through a currently valid ID, document, and generation.
pub struct ResourcePolicy {
    root: PathBuf,
    document: PathBuf,
    document_id: DocumentId,
    generation: Generation,
    max_bytes: u64,
    next_id: u64,
    grants: HashMap<ResourceId, Grant>,
}

impl ResourcePolicy {
    pub fn new(
        root: &Path,
        document: &Path,
        document_id: DocumentId,
        generation: Generation,
    ) -> Result<Self, ContextError> {
        Self::with_limit(
            root,
            document,
            document_id,
            generation,
            DEFAULT_LOCAL_RESOURCE_BYTES,
        )
    }

    pub fn with_limit(
        root: &Path,
        document: &Path,
        document_id: DocumentId,
        generation: Generation,
        max_bytes: u64,
    ) -> Result<Self, ContextError> {
        let root = canonical_directory(root)?;
        let document = canonical_file(document)?;
        Ok(Self {
            root,
            document,
            document_id,
            generation,
            max_bytes,
            next_id: 1,
            grants: HashMap::new(),
        })
    }

    /// Replace document authority. Successful context changes revoke every ID.
    pub fn set_context(
        &mut self,
        root: &Path,
        document: &Path,
        document_id: DocumentId,
        generation: Generation,
    ) -> Result<(), ContextError> {
        let root = canonical_directory(root)?;
        let document = canonical_file(document)?;
        if self.root != root
            || self.document != document
            || self.document_id != document_id
            || self.generation != generation
        {
            self.grants.clear();
        }
        self.root = root;
        self.document = document;
        self.document_id = document_id;
        self.generation = generation;
        Ok(())
    }

    /// Allow one resource only when its resolved path stays under current root.
    pub fn authorize(&mut self, reference: &Path) -> ResourceAuthorization {
        let (path, bytes) = match self.resolve(reference) {
            Ok(value) => value,
            Err(error) => return ResourceAuthorization::Denied(error),
        };
        if !is_within(&self.root, &path) {
            return ResourceAuthorization::Denied(ResourceDenied::OutsideRoot);
        }
        match self.check_size(bytes) {
            Ok(()) => self.allocate(path),
            Err(error) => ResourceAuthorization::Denied(error),
        }
    }

    /// Explicitly allow exactly one resolved outside-root resource.
    pub fn authorize_explicit(&mut self, reference: &Path) -> ResourceAuthorization {
        let (path, bytes) = match self.resolve(reference) {
            Ok(value) => value,
            Err(error) => return ResourceAuthorization::Denied(error),
        };
        match self.check_size(bytes) {
            Ok(()) => self.allocate(path),
            Err(error) => ResourceAuthorization::Denied(error),
        }
    }

    /// Read bytes only through an opaque grant in its original context.
    pub fn read_granted_resource(
        &self,
        document_id: DocumentId,
        generation: Generation,
        resource: ResourceId,
    ) -> Result<Vec<u8>, ResourceReadError> {
        if self.document_id != document_id || self.generation != generation {
            return Err(ResourceReadError::StaleContext);
        }
        let Some(grant) = self.grants.get(&resource) else {
            return Err(ResourceReadError::UnknownResource);
        };
        let current = fs::canonicalize(&grant.path).map_err(map_read_error)?;
        if current != grant.path {
            return Err(ResourceReadError::ResourceChanged);
        }
        let metadata = fs::metadata(&current).map_err(map_read_error)?;
        if !metadata.is_file() {
            return Err(ResourceReadError::NotAFile);
        }
        let bytes = metadata.len();
        if bytes > self.max_bytes {
            return Err(ResourceReadError::TooLarge {
                bytes,
                max: self.max_bytes,
            });
        }
        fs::read(current).map_err(map_read_error)
    }

    fn resolve(&self, reference: &Path) -> Result<(PathBuf, u64), ResourceDenied> {
        let candidate = if reference.is_absolute() {
            reference.to_owned()
        } else {
            self.document
                .parent()
                .expect("canonical document has a parent")
                .join(reference)
        };
        let path = fs::canonicalize(candidate).map_err(map_resource_error)?;
        let metadata = fs::metadata(&path).map_err(map_resource_error)?;
        if !metadata.is_file() {
            return Err(ResourceDenied::NotAFile);
        }
        Ok((path, metadata.len()))
    }

    fn check_size(&self, bytes: u64) -> Result<(), ResourceDenied> {
        if bytes > self.max_bytes {
            Err(ResourceDenied::TooLarge {
                bytes,
                max: self.max_bytes,
            })
        } else {
            Ok(())
        }
    }

    fn allocate(&mut self, path: PathBuf) -> ResourceAuthorization {
        let raw = self.next_id;
        self.next_id = self.next_id.checked_add(1).unwrap_or(1);
        let resource = ResourceId::new(raw).expect("resource IDs start nonzero");
        self.grants.insert(resource, Grant { path });
        ResourceAuthorization::Allowed(resource)
    }
}

fn canonical_directory(path: &Path) -> Result<PathBuf, ContextError> {
    let path = fs::canonicalize(path).map_err(map_context_error)?;
    if fs::metadata(&path).map_err(map_context_error)?.is_dir() {
        Ok(path)
    } else {
        Err(ContextError::NotADirectory)
    }
}

fn canonical_file(path: &Path) -> Result<PathBuf, ContextError> {
    let path = fs::canonicalize(path).map_err(map_context_error)?;
    if fs::metadata(&path).map_err(map_context_error)?.is_file() {
        Ok(path)
    } else {
        Err(ContextError::NotAFile)
    }
}

fn is_within(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}

fn map_context_error(error: std::io::Error) -> ContextError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ContextError::Missing,
        std::io::ErrorKind::NotADirectory => ContextError::NotADirectory,
        _ if error.kind() == std::io::ErrorKind::PermissionDenied => ContextError::Unreadable,
        _ => ContextError::Unreadable,
    }
}

fn map_resource_error(error: std::io::Error) -> ResourceDenied {
    match error.kind() {
        std::io::ErrorKind::NotFound => ResourceDenied::Missing,
        std::io::ErrorKind::PermissionDenied => ResourceDenied::Unreadable,
        _ => ResourceDenied::Unreadable,
    }
}

fn map_read_error(error: std::io::Error) -> ResourceReadError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ResourceReadError::Missing,
        std::io::ErrorKind::PermissionDenied => ResourceReadError::Unreadable,
        _ => ResourceReadError::Unreadable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let number = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mdvr-resource-policy-{}-{number}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn ids(document: u64, generation: u64) -> (DocumentId, Generation) {
        (
            DocumentId::new(document).unwrap(),
            Generation::new(generation).unwrap(),
        )
    }

    fn policy(root: &Path, document: &Path) -> ResourcePolicy {
        let (document_id, generation) = ids(1, 1);
        ResourcePolicy::new(root, document, document_id, generation).unwrap()
    }

    fn allowed(result: ResourceAuthorization) -> ResourceId {
        let ResourceAuthorization::Allowed(resource) = result else {
            panic!("expected allowed resource")
        };
        resource
    }

    #[test]
    fn same_root_allowed_outside_denied_and_explicit_grant_is_one_resource() {
        let base = temp_dir();
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let document = root.join("document.md");
        let inside = root.join("assets/inside.png");
        let secret = outside.join("secret.png");
        let adjacent = outside.join("adjacent.png");
        fs::write(&document, "# doc").unwrap();
        fs::write(&inside, b"inside").unwrap();
        fs::write(&secret, b"secret").unwrap();
        fs::write(&adjacent, b"adjacent").unwrap();

        let (document_id, generation) = ids(1, 1);
        let mut policy = policy(&root, &document);
        let inside_id = allowed(policy.authorize(Path::new("assets/inside.png")));
        assert_eq!(
            policy.read_granted_resource(document_id, generation, inside_id),
            Ok(b"inside".to_vec())
        );
        assert_eq!(
            policy.authorize(&secret),
            ResourceAuthorization::Denied(ResourceDenied::OutsideRoot)
        );
        let secret_id = allowed(policy.authorize_explicit(&secret));
        assert_eq!(
            policy.read_granted_resource(document_id, generation, secret_id),
            Ok(b"secret".to_vec())
        );
        assert_eq!(
            policy.authorize(&adjacent),
            ResourceAuthorization::Denied(ResourceDenied::OutsideRoot)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_denied_after_resolution() {
        use std::os::unix::fs::symlink;

        let base = temp_dir();
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let document = root.join("document.md");
        fs::write(&document, "# doc").unwrap();
        fs::write(outside.join("secret.png"), b"secret").unwrap();
        symlink(&outside, root.join("linked-assets")).unwrap();

        let mut policy = policy(&root, &document);
        assert_eq!(
            policy.authorize(Path::new("linked-assets/secret.png")),
            ResourceAuthorization::Denied(ResourceDenied::OutsideRoot)
        );
    }

    #[test]
    fn traversal_is_denied_after_resolution() {
        let base = temp_dir();
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let document = root.join("document.md");
        let secret = outside.join("secret.png");
        fs::write(&document, "# doc").unwrap();
        fs::write(&secret, b"secret").unwrap();

        let mut policy = policy(&root, &document);
        assert_eq!(
            policy.authorize(Path::new("../outside/secret.png")),
            ResourceAuthorization::Denied(ResourceDenied::OutsideRoot)
        );
    }

    #[test]
    fn document_and_generation_changes_revoke_stale_grants() {
        let base = temp_dir();
        let root = base.join("root");
        let next_root = base.join("next-root");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&next_root).unwrap();
        let document = root.join("document.md");
        let next_document = next_root.join("document.md");
        let resource = root.join("asset.png");
        fs::write(&document, "# doc").unwrap();
        fs::write(&next_document, "# next").unwrap();
        fs::write(&resource, b"asset").unwrap();
        let (document_id, generation) = ids(1, 1);
        let mut policy = policy(&root, &document);
        let resource_id = allowed(policy.authorize(Path::new("asset.png")));

        policy
            .set_context(&root, &document, document_id, Generation::new(2).unwrap())
            .unwrap();
        assert_eq!(
            policy.read_granted_resource(document_id, generation, resource_id),
            Err(ResourceReadError::StaleContext)
        );
        assert_eq!(
            policy.read_granted_resource(document_id, Generation::new(2).unwrap(), resource_id),
            Err(ResourceReadError::UnknownResource)
        );

        policy
            .set_context(
                &next_root,
                &next_document,
                DocumentId::new(2).unwrap(),
                Generation::new(1).unwrap(),
            )
            .unwrap();
        assert_eq!(
            policy.read_granted_resource(
                DocumentId::new(2).unwrap(),
                Generation::new(1).unwrap(),
                resource_id,
            ),
            Err(ResourceReadError::UnknownResource)
        );
    }

    #[test]
    fn missing_and_unreadable_resources_are_denied() {
        let base = temp_dir();
        let root = base.join("root");
        fs::create_dir_all(&root).unwrap();
        let document = root.join("document.md");
        let parent_file = root.join("not-a-directory");
        fs::write(&document, "# doc").unwrap();
        fs::write(&parent_file, b"file").unwrap();
        let mut policy = policy(&root, &document);

        assert_eq!(
            policy.authorize(Path::new("missing.png")),
            ResourceAuthorization::Denied(ResourceDenied::Missing)
        );
        assert_eq!(
            policy.authorize(Path::new("not-a-directory/child.png")),
            ResourceAuthorization::Denied(ResourceDenied::Unreadable)
        );
    }

    #[test]
    fn size_limit_is_enforced_before_grant_and_before_read() {
        let base = temp_dir();
        let root = base.join("root");
        fs::create_dir_all(&root).unwrap();
        let document = root.join("document.md");
        let resource = root.join("asset.png");
        fs::write(&document, "# doc").unwrap();
        fs::write(&resource, b"1234").unwrap();
        let (document_id, generation) = ids(1, 1);
        let mut policy =
            ResourcePolicy::with_limit(&root, &document, document_id, generation, 3).unwrap();

        assert_eq!(
            policy.authorize(Path::new("asset.png")),
            ResourceAuthorization::Denied(ResourceDenied::TooLarge { bytes: 4, max: 3 })
        );

        fs::write(&resource, b"12").unwrap();
        let resource_id = allowed(policy.authorize(Path::new("asset.png")));
        fs::write(&resource, b"1234").unwrap();
        assert_eq!(
            policy.read_granted_resource(document_id, generation, resource_id),
            Err(ResourceReadError::TooLarge { bytes: 4, max: 3 })
        );
    }
}
