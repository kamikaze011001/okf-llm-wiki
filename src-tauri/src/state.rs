use crate::core::settings::Settings;
use crate::core::retrieval::IndexEntry;
use std::sync::Mutex;

#[derive(Default)]
pub struct AppState {
    pub settings: Mutex<Settings>,
    pub index: Mutex<Vec<IndexEntry>>,
}
