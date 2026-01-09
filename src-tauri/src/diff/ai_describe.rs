//! AI-powered hunk description using goose.

use std::process::Command;

/// Result of describing a hunk - before and after descriptions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HunkDescription {
    /// Description of what the code did before the change
    pub before: String,
    /// Description of what the code does after the change  
    pub after: String,
}

/// Describes a code change using goose AI.
///
/// Takes the before/after content of a hunk and the file path,
/// calls `goose run` with a prompt to describe the change.
/// Returns structured before/after descriptions.
pub fn describe_hunk(
    file_path: &str,
    before_lines: &[String],
    after_lines: &[String],
) -> Result<HunkDescription, String> {
    let before_content = if before_lines.is_empty() {
        "(empty - new content)".to_string()
    } else {
        before_lines.join("\n")
    };

    let after_content = if after_lines.is_empty() {
        "(empty - deleted content)".to_string()
    } else {
        after_lines.join("\n")
    };

    let prompt = format!(
        r#"Describe this code change concisely. Output EXACTLY in this format with no other text:

BEFORE: <one line describing what the old code did>
AFTER: <one line describing what the new code does>

File: {}

Old code:
```
{}
```

New code:
```
{}
```"#,
        file_path, before_content, after_content
    );

    log::info!("=== GOOSE DESCRIBE HUNK ===");
    log::info!("File: {}", file_path);
    log::info!("Prompt:\n{}", prompt);

    let output = Command::new("goose")
        .args(["run", "-t", &prompt])
        .output()
        .map_err(|e| format!("Failed to run goose: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    log::info!("=== GOOSE RESPONSE ===");
    log::info!("Exit code: {:?}", output.status.code());
    log::info!("Stdout:\n{}", stdout);
    if !stderr.is_empty() {
        log::info!("Stderr:\n{}", stderr);
    }

    if !output.status.success() {
        return Err(format!(
            "goose exited with code {:?}: {}",
            output.status.code(),
            stderr
        ));
    }

    // Parse the response - look for BEFORE: and AFTER: lines
    let response = stdout.trim();
    let before_desc = extract_field(response, "BEFORE:")
        .unwrap_or_else(|| "Could not parse before description".to_string());
    let after_desc = extract_field(response, "AFTER:")
        .unwrap_or_else(|| "Could not parse after description".to_string());

    Ok(HunkDescription {
        before: before_desc,
        after: after_desc,
    })
}

/// Extract a field value from the response (e.g., "BEFORE: some text" -> "some text")
fn extract_field(response: &str, field: &str) -> Option<String> {
    for line in response.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix(field) {
            return Some(value.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_field() {
        let response = "BEFORE: old behavior\nAFTER: new behavior";
        assert_eq!(
            extract_field(response, "BEFORE:"),
            Some("old behavior".to_string())
        );
        assert_eq!(
            extract_field(response, "AFTER:"),
            Some("new behavior".to_string())
        );
    }

    #[test]
    #[ignore] // Requires goose to be installed
    fn test_describe_hunk() {
        let before = vec!["fn old() {}".to_string()];
        let after = vec!["fn new_name() {}".to_string()];

        let result = describe_hunk("test.rs", &before, &after);
        println!("Result: {:?}", result);
        assert!(result.is_ok());
    }
}
