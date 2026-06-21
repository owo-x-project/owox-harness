//! トークン量の軽い推定 (`docs/decisions/20260614-Phase7-測定可視化とブランド検証.md`)。
//!
//! 旗「最小コンテキスト」を主張でなく測定可能な事実にするため、文脈出力の情報量を数値で示す。
//! 正確なトークナイザは重く新規依存・言語非依存方針に反するため入れない。桁感を示す推定に留める。
//!
//! 日本語混在で `文字数/4` は実数より大幅に小さく出る。CJK 文字を別勘定にして過小評価を抑える。

/// テキストの推定トークン数を返す。決定論・新規依存なし。
///
/// CJK 文字 (漢字・かな・全角など) は 1 文字 ≒ 1 トークン、それ以外は 4 文字 ≒ 1 トークンで合算する。
/// 厳密値でなく桁感を示す推定。pass/fail ゲートには使わない。
pub fn estimate_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for c in text.chars() {
        if is_cjk(c) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    // 端数の切り上げで、短い ASCII でも 0 にしない (1 文字でも 1 トークン)。
    cjk + other.div_ceil(4)
}

/// CJK 文字か (漢字・ひらがな・カタカナ・全角記号など、1 文字あたりのトークン密度が高い帯)。
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F   // CJK 記号・句読点
        | 0x3040..=0x309F // ひらがな
        | 0x30A0..=0x30FF // カタカナ
        | 0x3400..=0x4DBF // CJK 拡張A
        | 0x4E00..=0x9FFF // CJK 統合漢字
        | 0xF900..=0xFAFF // CJK 互換漢字
        | 0xFF00..=0xFFEF // 全角・半角形
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn ascii_is_quartered_and_rounded_up() {
        // 4 文字 = 1、5 文字 = 2 (切り上げ)。
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        // 1 文字でも 0 にしない。
        assert_eq!(estimate_tokens("a"), 1);
    }

    #[test]
    fn cjk_counts_per_char() {
        // 漢字・かなは 1 文字 1 トークン。
        assert_eq!(estimate_tokens("日本語"), 3);
        assert_eq!(estimate_tokens("あいう"), 3);
    }

    #[test]
    fn mixed_sums_both_bands() {
        // CJK 3 + ASCII 4 文字 (1) = 4。
        assert_eq!(estimate_tokens("日本語abcd"), 4);
    }

    #[test]
    fn japanese_estimate_exceeds_naive_quarter() {
        // 単純な 文字数/4 より大きく出る (過小評価を避ける目的)。
        let jp = "これは日本語の文章です";
        let naive = jp.chars().count() / 4;
        assert!(estimate_tokens(jp) > naive);
    }
}
