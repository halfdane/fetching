//! Command-line interface parsing and validation.
//!
//! Handles argument parsing, validation, and help text generation
//! for the Spotify player application.

/// Source of Spotify URIs to process
#[derive(Debug)]
pub enum InputSource {
    SingleUri(String),
    File(std::path::PathBuf),
}

/// Validate command line arguments
pub fn validate_args(args: &[String]) -> anyhow::Result<InputSource> {
    let mut input_source = None;
    let mut iter = args.iter().skip(1).peekable();

    while let Some(arg) = iter.next() {
        if arg == "--file" {
            if input_source.is_some() {
                anyhow::bail!("Cannot specify both --file and a URI");
            }
            let path = iter.next()
                .ok_or_else(|| anyhow::anyhow!("--file requires a path argument"))?;
            input_source = Some(InputSource::File(std::path::PathBuf::from(path)));
        } else if input_source.is_none() {
            input_source = Some(InputSource::SingleUri(arg.clone()));
        } else {
            anyhow::bail!("Unexpected argument: {}", arg);
        }
    }

    match input_source {
        Some(source) => Ok(source),
        None => anyhow::bail!("Expected either a Spotify URI or --file <path>"),
    }
}

/// Print usage information and exit
pub fn print_usage_and_exit(args: &[String]) -> ! {
    eprintln!("Usage: {} <spotify_uri>", args[0]);
    eprintln!("       {} --file <path>", args[0]);
    eprintln!("Options:");
    eprintln!("  --file       Read URIs from file (one per line, # for comments)");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {} spotify:track:4uLU6hMCjMI75M1A2tKUQC", args[0]);
    eprintln!("  {} spotify:album:1A2GTWGtFfWp7KSQTwWOyo", args[0]);
    eprintln!("  {} spotify:playlist:37i9dQZF1DX0XUsuxWHRQd", args[0]);
    eprintln!("  {} 4uLU6hMCjMI75M1A2tKUQC  (assumes track)", args[0]);
    eprintln!("  {} --file my_uris.txt", args[0]);
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_args_correct_count() {
        let args = vec!["program".to_string(), "spotify:track:123".to_string()];
        let result = validate_args(&args);
        assert!(result.is_ok());
        let input_source = result.unwrap();
        match input_source {
            InputSource::SingleUri(uri) => assert_eq!(uri, "spotify:track:123"),
            _ => panic!("Expected SingleUri"),
        }
    }

    #[test]
    fn test_validate_args_too_few() {
        let args = vec!["program".to_string()];
        let result = validate_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_args_unexpected_arg() {
        let args = vec![
            "program".to_string(),
            "spotify:track:123".to_string(),
            "extra".to_string(),
        ];
        let result = validate_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_args_with_file() {
        let args = vec!["program".to_string(), "--file".to_string(), "uris.txt".to_string()];
        let result = validate_args(&args);
        assert!(result.is_ok());
        let input_source = result.unwrap();
        match input_source {
            InputSource::File(path) => assert_eq!(path.to_str().unwrap(), "uris.txt"),
            _ => panic!("Expected File"),
        }
    }

    #[test]
    fn test_validate_args_file_without_path() {
        let args = vec!["program".to_string(), "--file".to_string()];
        let result = validate_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_args_both_file_and_uri() {
        let args = vec![
            "program".to_string(),
            "--file".to_string(),
            "uris.txt".to_string(),
            "spotify:track:123".to_string(),
        ];
        let result = validate_args(&args);
        assert!(result.is_err());
    }
}