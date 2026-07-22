// SPDX-License-Identifier: Apache-2.0

use crate::DataError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_ROUTE_PARAMETERS: usize = 32;
const MAX_QUERY_VALUES: usize = 128;
const MAX_METADATA_VALUES: usize = 16;
const MAX_NAME_BYTES: usize = 256;
const MAX_VALUE_BYTES: usize = 4 * 1_024;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataRequestValues {
    route_parameters: BTreeMap<String, String>,
    query: BTreeMap<String, Vec<String>>,
    metadata: BTreeMap<String, String>,
}

impl DataRequestValues {
    pub fn new(
        route_parameters: BTreeMap<String, String>,
        query: BTreeMap<String, Vec<String>>,
        metadata: BTreeMap<String, String>,
    ) -> Result<Self, DataError> {
        if route_parameters.len() > MAX_ROUTE_PARAMETERS {
            return Err(invalid("too many route parameters"));
        }
        if query.values().map(Vec::len).sum::<usize>() > MAX_QUERY_VALUES {
            return Err(invalid("too many query values"));
        }
        if metadata.len() > MAX_METADATA_VALUES {
            return Err(invalid("too many request metadata values"));
        }
        for (name, value) in &route_parameters {
            validate_name(name)?;
            validate_value(value)?;
        }
        for (name, values) in &query {
            validate_name(name)?;
            if values.is_empty() {
                return Err(invalid("query keys must contain at least one value"));
            }
            for value in values {
                validate_value(value)?;
            }
        }
        for (name, value) in &metadata {
            validate_name(name)?;
            validate_value(value)?;
        }
        Ok(Self {
            route_parameters,
            query,
            metadata,
        })
    }

    pub fn route_parameter(&self, name: &str) -> Option<&str> {
        self.route_parameters.get(name).map(String::as_str)
    }

    pub fn route_parameters(&self) -> &BTreeMap<String, String> {
        &self.route_parameters
    }

    pub fn query_values(&self, name: &str) -> Option<&[String]> {
        self.query.get(name).map(Vec::as_slice)
    }

    pub fn query(&self) -> &BTreeMap<String, Vec<String>> {
        &self.query
    }

    pub fn metadata(&self, name: &str) -> Option<&str> {
        self.metadata.get(name).map(String::as_str)
    }
}

fn validate_name(value: &str) -> Result<(), DataError> {
    if value.is_empty()
        || value.len() > MAX_NAME_BYTES
        || value.chars().any(|character| character.is_control())
    {
        return Err(invalid("request value name is invalid"));
    }
    Ok(())
}

fn validate_value(value: &str) -> Result<(), DataError> {
    if value.len() > MAX_VALUE_BYTES
        || value
            .chars()
            .any(|character| matches!(character, '\0' | '\r' | '\n'))
    {
        return Err(invalid("request value is invalid or exceeds its bound"));
    }
    Ok(())
}

fn invalid(message: &str) -> DataError {
    DataError::RequestValues(message.to_owned())
}
