use std::collections::BTreeSet;

use crate::{PersistError, Result};

use super::{
    hard_denied, valid_name, CONNECTION_NAMES, MAX_SHELL_ENVIRONMENT_BYTES,
    MAX_SHELL_ENVIRONMENT_VALUE_BYTES, MAX_SHELL_ENVIRONMENT_VARIABLES,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellLaunchEnvironment {
    saved_set: Vec<(String, String)>,
    saved_unset: Vec<String>,
    connection: Vec<(String, String)>,
    private: Vec<(String, String)>,
}

impl ShellLaunchEnvironment {
    pub fn new(
        saved_set: Vec<(String, String)>,
        saved_unset: Vec<String>,
        connection: Vec<(String, String)>,
        private: Vec<(String, String)>,
    ) -> Result<Self> {
        let environment = Self {
            saved_set,
            saved_unset,
            connection,
            private,
        };
        environment.validate()?;
        Ok(environment)
    }

    pub fn legacy(environment: Vec<(String, String)>) -> Result<Self> {
        Self::new(Vec::new(), Vec::new(), Vec::new(), environment)
    }

    pub fn saved_set(&self) -> &[(String, String)] {
        &self.saved_set
    }

    pub fn saved_unset(&self) -> &[String] {
        &self.saved_unset
    }

    pub fn connection(&self) -> &[(String, String)] {
        &self.connection
    }

    pub fn private(&self) -> &[(String, String)] {
        &self.private
    }

    pub fn legacy_set_environment(&self) -> Vec<(String, String)> {
        self.saved_set
            .iter()
            .chain(&self.connection)
            .chain(&self.private)
            .cloned()
            .collect()
    }

    pub fn entry_count(&self) -> usize {
        self.saved_set.len() + self.saved_unset.len() + self.connection.len() + self.private.len()
    }

    fn validate(&self) -> Result<()> {
        if self.entry_count() > MAX_SHELL_ENVIRONMENT_VARIABLES {
            return Err(invalid("launch environment has too many entries"));
        }

        let mut names = BTreeSet::new();
        let mut bytes = 0_usize;
        for (name, value) in &self.saved_set {
            validate_pair(name, value)?;
            if hard_denied(name) {
                return Err(invalid("saved environment contains a forbidden name"));
            }
            insert_name(&mut names, name)?;
            bytes = add_bytes(bytes, name.len() + value.len())?;
        }
        for name in &self.saved_unset {
            if !valid_name(name) || hard_denied(name) {
                return Err(invalid("saved unset contains an invalid or forbidden name"));
            }
            insert_name(&mut names, name)?;
            bytes = add_bytes(bytes, name.len())?;
        }
        for (name, value) in &self.connection {
            validate_pair(name, value)?;
            if !CONNECTION_NAMES.contains(&name.as_str()) {
                return Err(invalid("connection environment name is not allowed"));
            }
            insert_name(&mut names, name)?;
            bytes = add_bytes(bytes, name.len() + value.len())?;
        }
        for (name, value) in &self.private {
            validate_pair(name, value)?;
            insert_name(&mut names, name)?;
            bytes = add_bytes(bytes, name.len() + value.len())?;
        }
        Ok(())
    }
}

fn validate_pair(name: &str, value: &str) -> Result<()> {
    if !valid_name(name)
        || value.len() > MAX_SHELL_ENVIRONMENT_VALUE_BYTES
        || value.as_bytes().contains(&0)
    {
        return Err(invalid("launch environment entry is invalid"));
    }
    Ok(())
}

fn insert_name(names: &mut BTreeSet<String>, name: &str) -> Result<()> {
    if !names.insert(name.to_owned()) {
        return Err(invalid("launch environment contains duplicate names"));
    }
    Ok(())
}

fn add_bytes(current: usize, added: usize) -> Result<usize> {
    let total = current
        .checked_add(added)
        .ok_or_else(|| invalid("launch environment size overflow"))?;
    if total > MAX_SHELL_ENVIRONMENT_BYTES {
        return Err(invalid("launch environment exceeds byte limit"));
    }
    Ok(total)
}

fn invalid(message: &'static str) -> PersistError {
    PersistError::invalid_argument(message)
}
