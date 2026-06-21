//! 日付の供給。mcp が今日の日付を求め core へ引数で渡す。
//!
//! core は時計を読まず today を引数で受ける (決定論。`docs/decisions/20260613-Phase4-tool記録層.md`)。
//! serve (記録 ID) と hook (腐敗検知の古さ判定) が共用する。

/// 現在の UTC 日付を `YYYYMMDD` で返す。新規依存を足さず std から求める。
///
/// 記録 ID の日付・腐敗検知の古さ判定に使う。Codex の MCP 設定にパスや時刻を焼かない。
pub fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}{m:02}{d:02}")
}

/// epoch からの日数を西暦年月日へ変換する (Howard Hinnant の civil_from_days)。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_known_dates() {
        // 1970-01-01 は epoch day 0。
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2026-06-13 は epoch day 20617。
        assert_eq!(civil_from_days(20_617), (2026, 6, 13));
        // 閏日 2024-02-29。
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn today_utc_is_eight_digits() {
        let t = today_utc();
        assert_eq!(t.len(), 8);
        assert!(t.chars().all(|c| c.is_ascii_digit()));
    }
}
