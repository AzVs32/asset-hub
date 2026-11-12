/// The exact bytes stored in a symbolic link. Reading a link preserves its
/// contents, including bytes that are invalid UTF-8.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkTarget(Vec<u8>);

impl LinkTarget {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}
