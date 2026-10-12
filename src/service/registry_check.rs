/// The registry paths represented by the in-memory service model.
/// Share this classification between decoding and evaluation: accepting a
/// broader prefix at load time turns unsupported paths into silent failures.
pub(crate) enum CachedRegistryKey<'a> {
    Root,
    Service(&'a str),
}

pub(crate) fn cached_registry_key(key: &str) -> Option<CachedRegistryKey<'_>> {
    if matches!(key, "Machine\\System\\Services" | "Machine\\System\\Init") {
        return Some(CachedRegistryKey::Root);
    }
    let service = key.strip_prefix("Machine\\System\\Services\\")?;
    if service.is_empty() || service.contains('\\') {
        return None;
    }
    Some(CachedRegistryKey::Service(service))
}
