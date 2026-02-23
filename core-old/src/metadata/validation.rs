//! String validation and sanitization utilities.
//!
//! Functions for cleaning and validating strings, particularly for use
//! in filenames and file paths.

/// Sanitize a string for use in filenames by replacing uncommon characters with underscores.
/// Keeps: alphanumeric and periods (common in filenames).
/// Replaces everything else (including spaces and dashes) with underscores.
/// Collapses consecutive underscores into a single underscore.
pub fn sanitize(s: &str) -> String {
    let mut result = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();

    // Collapse consecutive underscores
    while result.contains("__") {
        result = result.replace("__", "_");
    }

    result.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_empty() {
        assert_eq!(sanitize(""), "");
    }

    #[test]
    fn test_sanitize_no_special() {
        assert_eq!(sanitize("Simple"), "Simple");
    }

    #[test]
    fn test_sanitize_spaces_and_dashes() {
        assert_eq!(sanitize("Hello World-Test"), "Hello_World_Test");
    }

    #[test]
    fn test_sanitize_special_characters() {
        assert_eq!(
            sanitize("File:Name?With*Special<Chars>"),
            "File_Name_With_Special_Chars"
        );
    }

    #[test]
    fn test_sanitize_leading_trailing_underscores() {
        assert_eq!(sanitize("_leading_and_trailing_"), "leading_and_trailing");
    }

    #[test]
    fn test_sanitize_keeps_periods() {
        assert_eq!(sanitize("file.name.txt"), "file.name.txt");
    }

    #[test]
    fn test_sanitize_unicode_characters() {
        assert_eq!(sanitize("café_résumé naïve"), "café_résumé_naïve");
    }

    #[test]
    fn test_sanitize_exclamation_marks() {
        assert_eq!(sanitize("Wow! This is great!"), "Wow_This_is_great");
    }

    #[test]
    fn test_sanitize_single_quotes() {
        assert_eq!(sanitize("Don't worry"), "Don_t_worry");
    }

    #[test]
    fn test_sanitize_collapses_consecutive_underscores() {
        assert_eq!(sanitize("a--b__c   d"), "a_b_c_d");
    }

    #[test]
    fn test_sanitize_only_invalid_chars() {
        assert_eq!(sanitize("!@#$%^&*()"), "");
    }

    #[test]
    fn test_sanitize_mixed_invalid_chars() {
        assert_eq!(sanitize("a!b@c#d$e%f^g&h*i(j)k"), "a_b_c_d_e_f_g_h_i_j_k");
    }

    #[test]
    fn test_sanitize_preserves_dots() {
        assert_eq!(sanitize("file.txt"), "file.txt");
    }

    #[test]
    fn test_sanitize_removes_invalid_chars() {
        assert_eq!(
            sanitize("file/name:with?invalid*chars"),
            "file_name_with_invalid_chars"
        );
    }

    #[test]
    fn test_sanitize_replaces_spaces_and_dashes() {
        assert_eq!(sanitize("hello world-test case"), "hello_world_test_case");
    }

    #[test]
    fn test_sanitize_trims_underscores() {
        assert_eq!(sanitize("_hello_world_"), "hello_world");
    }

    #[test]
    fn test_sanitize_handles_unicode() {
        assert_eq!(sanitize("naïve café résumé"), "naïve_café_résumé");
    }
}
