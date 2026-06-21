//! 正本の Markdown パーサ。見出しと箇条書きだけの限定文法。
//!
//! `## Section` を節の区切り、`- item` を箇条書きとして読む。
//! 行ベースで自作し、新規依存を足さない。型検証は読込後に各正本が行う。

/// Markdown 文書を `## 見出し` 単位の節へ分けたもの。
pub struct Doc {
    sections: Vec<Section>,
}

/// 1 つの節。`## 見出し` とその直下の本文行。
pub struct Section {
    heading: String,
    /// 本文行 (空行を除きトリム済み)。箇条書きは `- ` で始まる。
    lines: Vec<String>,
}

impl Doc {
    /// 文書を節へ分ける。最初の `## ` より前 (タイトル等) は捨てる。
    pub fn parse(text: &str) -> Doc {
        let mut sections: Vec<Section> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if is_section_heading(line) {
                let heading = line["## ".len()..].trim().to_string();
                sections.push(Section {
                    heading,
                    lines: Vec::new(),
                });
            } else if line.starts_with("# ") {
                // 文書タイトル (レベル1) は無視する。
                continue;
            } else if let Some(section) = sections.last_mut() {
                section.lines.push(line.to_string());
            }
            // 最初の節より前の本文は捨てる。
        }
        Doc { sections }
    }

    /// 見出しに一致する節を取り出す (大文字小文字は無視)。重複時は最初。
    pub fn take(&mut self, heading: &str) -> Option<Section> {
        let pos = self
            .sections
            .iter()
            .position(|s| s.heading.eq_ignore_ascii_case(heading))?;
        Some(self.sections.remove(pos))
    }

    /// 取り出されずに残った見出し。未知見出しの検出に使う。
    pub fn remaining_headings(&self) -> Vec<&str> {
        self.sections.iter().map(|s| s.heading.as_str()).collect()
    }

    /// 全節を取り出す。見出し自体がエントリになる正本 (context) 向け。
    pub fn into_sections(self) -> Vec<Section> {
        self.sections
    }
}

impl Section {
    /// 見出し文字列。
    pub fn heading(&self) -> &str {
        &self.heading
    }

    /// 箇条書き項目 (`- ` を外したもの)。
    pub fn list(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter_map(|l| l.strip_prefix("- "))
            .map(|s| s.trim().to_string())
            .collect()
    }

    /// 本文行 (トリム済み)。箇条書きと属性行を順序つきで読む正本向け。
    ///
    /// `- ` で始まる行がエントリ、それ以外の行は直前エントリの属性、という
    /// 読み方をする正本 (rules の不可逆操作など) はこれを使う。
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// 本文テキスト (箇条書きでない行を空白でつなぐ)。Vision など単一値向け。
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .filter(|l| !l.starts_with("- "))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// `## ` で始まるが `### ` ではない (レベル2 見出し)。
fn is_section_heading(line: &str) -> bool {
    line.starts_with("## ") && !line.starts_with("### ")
}

/// `名前: 説明` の行を 2 つへ分ける。区切りは最初のコロン。無ければ説明は空。
///
/// コロンは全キーボードで打て、`用語: 説明` と自然に書ける。
pub fn split_pair(item: &str) -> (String, String) {
    match item.split_once(':') {
        Some((name, rest)) => (name.trim().to_string(), rest.trim().to_string()),
        None => (item.trim().to_string(), String::new()),
    }
}
