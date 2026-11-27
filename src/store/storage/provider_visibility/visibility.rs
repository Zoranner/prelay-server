use prelay_protocol::ProviderVisibility;

use super::StorageError;

pub(super) fn parse_visibility(value: &str) -> Result<ProviderVisibility, StorageError> {
    match value {
        "private" => Ok(ProviderVisibility::Private),
        "selected" => Ok(ProviderVisibility::Selected),
        "all" => Ok(ProviderVisibility::All),
        other => Err(StorageError::InvalidProviderSharing(format!(
            "unknown visibility {other}"
        ))),
    }
}

pub(super) const fn visibility_value(visibility: ProviderVisibility) -> &'static str {
    match visibility {
        ProviderVisibility::Private => "private",
        ProviderVisibility::Selected => "selected",
        ProviderVisibility::All => "all",
    }
}
