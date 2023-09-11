#[derive(Debug)]
pub struct Store {}

impl Store {
    pub fn new() -> Self {
        Store {}
    }
}

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}
