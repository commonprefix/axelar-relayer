pub fn compare_op_code(expected_op_code: u32, op_code: &Vec<u8>) -> bool {
    if let Ok(bytes) = op_code.as_slice().try_into() {
        if u32::from_be_bytes(bytes) != expected_op_code {
            return false;
        }
    } else {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_op_code_matching() {
        let expected_op_code = 0x12345678;
        let op_code = vec![0x12, 0x34, 0x56, 0x78];

        assert!(compare_op_code(expected_op_code, &op_code));
    }

    #[test]
    fn test_compare_op_code_not_matching() {
        let expected_op_code = 0x12345678;
        let op_code = vec![0x87, 0x65, 0x43, 0x21];

        assert!(!compare_op_code(expected_op_code, &op_code));
    }

    #[test]
    fn test_compare_op_code_invalid_length() {
        let expected_op_code = 0x12345678;

        // Too short
        let op_code_short = vec![0x12, 0x34, 0x56];
        assert!(!compare_op_code(expected_op_code, &op_code_short));

        // Too long
        let op_code_long = vec![0x12, 0x34, 0x56, 0x78, 0x90];
        assert!(!compare_op_code(expected_op_code, &op_code_long));

        // Empty
        let op_code_empty = vec![];
        assert!(!compare_op_code(expected_op_code, &op_code_empty));
    }
}
