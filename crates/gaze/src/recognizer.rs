use std::sync::Arc;

use crate::Detector;

pub trait RecognizerRegistry: Send + Sync {
    fn detectors(&self) -> Vec<Arc<dyn Detector>>;
}

impl RecognizerRegistry for Vec<Arc<dyn Detector>> {
    fn detectors(&self) -> Vec<Arc<dyn Detector>> {
        self.clone()
    }
}
