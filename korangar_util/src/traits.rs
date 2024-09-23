use derive_new::new;

/// Error that is thrown when a file loader can't find the requested file.
#[derive(Debug, new)]
#[repr(transparent)]
pub struct FileNotFoundError(String);

/// Trait for general file loading.
pub trait FileLoader: Send + Sync + 'static {
    /// Returns the file content of the requested file.
    fn get(&self, path: &str) -> Result<Vec<u8>, FileNotFoundError>;
}
