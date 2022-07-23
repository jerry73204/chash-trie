#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Error {
    NotFound,
    Retry,
}
