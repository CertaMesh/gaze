use std::collections::HashMap;
use std::sync::Mutex;

use crate::detector::PiiClass;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub enum Scope {
    Ephemeral,
}

#[derive(Default)]
struct SessionState {
    next_by_class: HashMap<PiiClass, usize>,
    token_by_value: HashMap<(PiiClass, String), String>,
    value_by_token: HashMap<String, String>,
}

pub struct Session {
    _scope: Scope,
    state: Mutex<SessionState>,
}

impl Session {
    pub fn new(scope: Scope) -> Result<Self> {
        Ok(Self {
            _scope: scope,
            state: Mutex::new(SessionState::default()),
        })
    }

    pub fn tokenize(&self, class: &PiiClass, raw: &str) -> Result<String> {
        let mut state = self.state.lock().map_err(|_| Error::SessionPoisoned)?;
        let key = (class.clone(), raw.to_string());
        if let Some(token) = state.token_by_value.get(&key) {
            return Ok(token.clone());
        }

        let token = {
            let next = state.next_by_class.entry(class.clone()).or_insert(0);
            *next += 1;
            format!("{}_{}", class_name(class), next)
        };

        state.token_by_value.insert(key, token.clone());
        state
            .value_by_token
            .insert(token.clone(), raw.to_string());
        Ok(token)
    }

    pub fn restore_strict(&self, token: &str) -> Result<String> {
        let state = self.state.lock().map_err(|_| Error::SessionPoisoned)?;
        state
            .value_by_token
            .get(token)
            .cloned()
            .ok_or_else(|| Error::UnknownToken(token.to_string()))
    }

    pub fn restore(&self, token: &str) -> Option<String> {
        let state = self.state.lock().ok()?;
        state.value_by_token.get(token).cloned()
    }
}

fn class_name(class: &PiiClass) -> &'static str {
    match class {
        PiiClass::Email => "Email",
        PiiClass::Name => "Name",
    }
}
