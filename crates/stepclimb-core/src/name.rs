// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one rule for organization, account and role names (spec 02 B13, B16, Q6; spec 09 B8):
//! trimmed, not empty, at most [`MAX_CHARS`] characters, no NUL.

/// The longest organization, account or role name, in characters (Unicode scalar values; spec
/// 02 Q6).
pub const MAX_CHARS: usize = 200;

/// Why a name was refused. Each repository maps it to its own error type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Invalid {
    Empty,
    TooLong,
    /// PostgreSQL `text` cannot store NUL.
    ContainsNul,
}

/// `name` trimmed, or why it is refused. The length limit counts characters after trimming.
pub(crate) fn valid(name: &str) -> Result<&str, Invalid> {
    if name.contains('\0') {
        return Err(Invalid::ContainsNul);
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(Invalid::Empty);
    }
    if name.chars().count() > MAX_CHARS {
        return Err(Invalid::TooLong);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed() {
        assert_eq!(valid("  Tower Cab \n").unwrap(), "Tower Cab");
    }

    #[test]
    fn an_empty_or_blank_name_is_refused() {
        for name in ["", "   ", "\t\n"] {
            assert_eq!(valid(name), Err(Invalid::Empty), "{name:?}");
        }
    }

    #[test]
    fn a_name_with_nul_is_refused() {
        assert_eq!(valid("Tower\0Cab"), Err(Invalid::ContainsNul));
    }

    #[test]
    fn the_length_limit_counts_characters_after_trimming() {
        let at_limit = "é".repeat(MAX_CHARS);
        assert_eq!(valid(&format!(" {at_limit} ")).unwrap(), at_limit);
        let over = "a".repeat(MAX_CHARS + 1);
        assert_eq!(valid(&over), Err(Invalid::TooLong));
    }
}
