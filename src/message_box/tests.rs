#[cfg(test)]
mod tests {
    use super::super::terminal::*;
    use super::super::*;

    #[test]
    fn clamp_width_bounds() {
        assert_eq!(clamp_width(10), MIN_WIDTH);
        assert_eq!(clamp_width(80), 80);
        assert_eq!(clamp_width(200), MAX_WIDTH);
    }

    #[test]
    fn columns_env_reads_variable() {
        temp_env::with_var("COLUMNS", Some("100"), || {
            assert_eq!(columns_env(), Some(100));
        });
    }

    #[test]
    fn empty_box_prints_borders_only() {
        MessageBox::new().print_stderr();
    }

    #[test]
    fn mixed_entries() {
        MessageBox::new()
            .info()
            .line("Header")
            .paragraph("Body line one")
            .line("Footer")
            .print_stderr();
    }

    #[test]
    fn info_box() {
        MessageBox::new()
            .info()
            .line("Update available")
            .print_stderr();
    }

    #[test]
    fn warning_box() {
        MessageBox::new()
            .warning()
            .line("Something deprecated")
            .print_stderr();
    }

    #[test]
    fn error_box() {
        MessageBox::new()
            .error()
            .line("Fatal problem")
            .print_stderr();
    }
}
