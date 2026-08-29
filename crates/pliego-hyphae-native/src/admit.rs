// SPDX-License-Identifier: AGPL-3.0-only

use crate::error::TransportErrorKind;
use crate::{ProductCapabilities, TransportError};

pub(crate) fn capabilities(
    value: ProductCapabilities,
) -> Result<ProductCapabilities, TransportError> {
    if value.product_api_version != 1 || value.native_directory_format != 1 {
        return Err(TransportErrorKind::IncompatibleCapabilities(value).into());
    }
    Ok(value)
}
