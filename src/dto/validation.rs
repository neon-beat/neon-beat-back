//! Validation helpers for DTOs.

use validator::ValidationError;

/// Validates that a buzzer ID is exactly 12 lowercase hexadecimal characters.
///
/// # Examples
///
/// ```ignore
/// validate_buzzer_id("deadbeef0001") // Ok
/// validate_buzzer_id("DeadBeef0001") // Err - uppercase
/// validate_buzzer_id("deadbeef001")  // Err - too short
/// ```
pub fn validate_buzzer_id(id: &str) -> Result<(), ValidationError> {
    if id.len() != 12 {
        let mut err = ValidationError::new("buzzer_id_length");
        err.message =
            Some(format!("Buzzer ID must be exactly 12 characters (got {})", id.len()).into());
        return Err(err);
    }

    if !id
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        let mut err = ValidationError::new("buzzer_id_format");
        err.message = Some("Buzzer ID must contain only lowercase hexadecimal characters".into());
        return Err(err);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_buzzer_id_valid() {
        assert!(validate_buzzer_id("deadbeef0001").is_ok());
        assert!(validate_buzzer_id("123456789abc").is_ok());
        assert!(validate_buzzer_id("000000000000").is_ok());
    }

    #[test]
    fn test_validate_buzzer_id_invalid_length() {
        assert!(validate_buzzer_id("deadbeef001").is_err()); // too short
        assert!(validate_buzzer_id("deadbeef00001").is_err()); // too long
        assert!(validate_buzzer_id("").is_err()); // empty
    }

    #[test]
    fn test_validate_buzzer_id_invalid_format() {
        assert!(validate_buzzer_id("DeadBeef0001").is_err()); // uppercase
        assert!(validate_buzzer_id("DEADBEEF0001").is_err()); // uppercase
        assert!(validate_buzzer_id("deadbeef000g").is_err()); // invalid hex
        assert!(validate_buzzer_id("deadbeef 001").is_err()); // space
    }
}
