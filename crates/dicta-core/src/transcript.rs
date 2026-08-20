pub fn format_timestamp(seconds: f64) -> String {
    let total_tenths = (seconds.max(0.0) * 10.0).round() as u64;
    let hours = total_tenths / 36_000;
    let minutes = (total_tenths / 600) % 60;
    let seconds = (total_tenths / 10) % 60;
    let tenths = total_tenths % 10;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}.{tenths}")
    } else {
        format!("{minutes:02}:{seconds:02}.{tenths}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_use_tenths_and_optional_hours() {
        assert_eq!(format_timestamp(18.24), "00:18.2");
        assert_eq!(format_timestamp(138.0), "02:18.0");
        assert_eq!(format_timestamp(3_723.94), "01:02:03.9");
    }
}
