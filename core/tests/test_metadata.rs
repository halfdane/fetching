use spotify_player_core::metadata::sanitize;

#[test]
fn test_sanitize_removes_invalid_chars() {
    assert_eq!(sanitize("Track/Name"), "Track_Name");
    assert_eq!(sanitize("Track:Name"), "Track_Name");
    assert_eq!(sanitize("Track\\Name"), "Track_Name");
    assert_eq!(sanitize("Track*Name"), "Track_Name");
    assert_eq!(sanitize("Track?Name"), "Track_Name");
    assert_eq!(sanitize("Track\"Name"), "Track_Name");
    assert_eq!(sanitize("Track<Name>"), "Track_Name");
    assert_eq!(sanitize("Track|Name"), "Track_Name");
}

#[test]
fn test_sanitize_replaces_spaces_and_dashes() {
    // Spaces and dashes are also replaced with underscores
    assert_eq!(sanitize("Valid Track Name"), "Valid_Track_Name");
    assert_eq!(sanitize("Track-01"), "Track_01");
    assert_eq!(sanitize("Track (Remix)"), "Track_Remix");
    assert_eq!(sanitize("Track_123"), "Track_123");
}

#[test]
fn test_sanitize_handles_unicode() {
    assert_eq!(sanitize("Café"), "Café");
    assert_eq!(sanitize("Ñoño"), "Ñoño");
    assert_eq!(sanitize("音楽"), "音楽");
}

#[test]
fn test_sanitize_empty_string() {
    assert_eq!(sanitize(""), "");
}

#[test]
fn test_sanitize_only_invalid_chars() {
    // Only invalid chars get trimmed after collapse
    assert_eq!(sanitize("///"), "");
    assert_eq!(sanitize("***"), "");
    assert_eq!(sanitize("<>|"), "");
}

#[test]
fn test_sanitize_trims_underscores() {
    // Leading and trailing underscores are trimmed
    assert_eq!(sanitize("  Track Name  "), "Track_Name");
    assert_eq!(sanitize("__Track__"), "Track");
}

#[test]
fn test_sanitize_mixed_invalid_chars() {
    assert_eq!(sanitize("Track/Name:Remix?"), "Track_Name_Remix");
}

#[test]
fn test_sanitize_preserves_dots() {
    assert_eq!(sanitize("Track.Name.v2"), "Track.Name.v2");
}

#[test]
fn test_sanitize_collapses_consecutive_underscores() {
    assert_eq!(sanitize("Track___Name"), "Track_Name");
    assert_eq!(sanitize("A  B  C"), "A_B_C");
}
