//! SQL placeholder utilities

use regex::Regex;

/// Renumber PostgreSQL placeholders ($1, $2, ...) by adding an offset
/// Used for UNION queries where the right side needs offset placeholders
pub fn renumber_postgres_placeholders(sql: &str, offset: usize) -> String {
    // Match PostgreSQL placeholders: $1, $2, $3, etc.
    let re = Regex::new(r"\$(\d+)").unwrap();

    re.replace_all(sql, |caps: &regex::Captures| {
        let num: usize = caps[1].parse().unwrap();
        format!("${}", num + offset)
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renumber_placeholders() {
        let sql = "SELECT * FROM users WHERE id = $1 AND name = $2";
        let result = renumber_postgres_placeholders(sql, 2);
        assert_eq!(result, "SELECT * FROM users WHERE id = $3 AND name = $4");
    }

    #[test]
    fn test_renumber_no_placeholders() {
        let sql = "SELECT * FROM users";
        let result = renumber_postgres_placeholders(sql, 5);
        assert_eq!(result, "SELECT * FROM users");
    }

    #[test]
    fn test_renumber_zero_offset() {
        let sql = "SELECT * FROM users WHERE id = $1";
        let result = renumber_postgres_placeholders(sql, 0);
        assert_eq!(result, "SELECT * FROM users WHERE id = $1");
    }
}
