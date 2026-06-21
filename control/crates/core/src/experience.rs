//! 経験の export / import (`docs/decisions/20260614-Phase7-経験IOと二層ルール.md`)。
//!
//! 汎用経験だけを別プロジェクトへ持ち出す。経験束は複数 kind を持ち、skill (SKILL.md + scripts) と
//! 成長層 (practices.md) を運ぶ。memory.md・tests・owox.toml・固定層 (brand/rules/glossary)・調査知識
//! (knowledge) は型で除外する (ドメイン固有・個性は持ち出さない)。本文は秘密走査し、export は持ち出しなので
//! 人間ゲートを通す。

use std::path::Path;

use serde_json::json;

use crate::envelope::{Envelope, Gate};
use crate::model::Practices;
use crate::secret::{self, SecretFinding};
use crate::skill::{Skill, load_skills};

/// 束のレイアウト: `<bundle>/skills/<id>/SKILL.md` + `scripts/*`、`<bundle>/practices.md`。
const PRACTICES_FILE: &str = "practices.md";

/// experience.export。汎用経験 (skill の SKILL.md + scripts と practices.md) を out_path へ束ねる。
///
/// 本文に秘密を検出したら failed で書かない。クリーンなら書き出し、持ち出しは人間ゲート (needs_human)。
pub fn export(owox_dir: &Path, out_path: &Path) -> Envelope {
    let skills = match load_skills(owox_dir) {
        Ok(s) => s,
        Err(err) => return Envelope::failed(err),
    };
    let practices_text = std::fs::read_to_string(owox_dir.join(PRACTICES_FILE)).unwrap_or_default();

    // 持ち出し前に本文を秘密走査する (skill 本文 + scripts + practices)。
    if let Some(env) = secret_block(&skills, &practices_text, "export") {
        return env;
    }

    // クリーン: 束を書き出す (skill の SKILL.md + scripts のみ・memory/tests/owox.toml は除外)。
    for skill in &skills {
        if let Err(err) = write_skill_bundle(out_path, skill) {
            return Envelope::failed(err);
        }
    }
    if !practices_text.trim().is_empty()
        && let Err(err) = std::fs::write(out_path.join(PRACTICES_FILE), &practices_text)
    {
        return Envelope::failed(format!("practices を書けない: {err}"));
    }

    Envelope::needs_human(
        format!(
            "Wrote a generic experience bundle ({} skill(s) + practices) to {}. Excluded memory, tests, owox.toml, and the brand-fixed canon. A human should review it before sharing it outside the project.",
            skills.len(),
            out_path.display()
        ),
        Gate {
            kind: "experience-export".to_string(),
            subject: out_path.display().to_string(),
            requires: "A human reviews the bundle and decides whether to carry it to another project."
                .to_string(),
        },
    )
    .with_data(json!({
        "skills": skills.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
        "practices": !practices_text.trim().is_empty(),
        "excluded": ["memory.md", "tests", "owox.toml", "brand", "rules", "glossary", "knowledge"],
    }))
}

/// experience.import。束 (in_path) から汎用経験を読み、秘密走査して `.owox/` へ取り込む。
///
/// 秘密を検出したら needs_human で止め取り込まない。クリーンなら skill を draft (owox.toml 無し) で
/// `.owox/skills/<id>/` へ書き、practices.md を既存へマージする。
pub fn import(owox_dir: &Path, in_path: &Path) -> Envelope {
    // 束も `<root>/skills/<id>/` の形なので load_skills を流用する。
    let skills = match load_skills(in_path) {
        Ok(s) => s,
        Err(err) => return Envelope::failed(format!("束を読めない: {err}")),
    };
    let practices_text = std::fs::read_to_string(in_path.join(PRACTICES_FILE)).unwrap_or_default();

    if skills.is_empty() && practices_text.trim().is_empty() {
        return Envelope::failed(format!("{} に経験束が無い", in_path.display()));
    }

    // 取り込み前に本文を秘密走査する。検出時は人間ゲートで止め、何も書かない。
    if let Some(env) = secret_block(&skills, &practices_text, "import") {
        return env;
    }

    // クリーン: skill を draft で書き (owox.toml を持たせない = 移植先で人間が登録・昇格)、practices をマージする。
    for skill in &skills {
        if let Err(err) = write_skill_bundle(owox_dir, skill) {
            return Envelope::failed(err);
        }
    }
    let merged = match merge_practices(owox_dir, &practices_text) {
        Ok(n) => n,
        Err(err) => return Envelope::failed(err),
    };

    Envelope::ok(
        format!(
            "Imported {} skill(s) as drafts and merged {merged} practice(s). Re-run their tests and promote them here.",
            skills.len()
        ),
        json!({
            "skills": skills.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            "practices_merged": merged,
        }),
    )
}

/// skill 本文 + scripts + practices を秘密走査し、検出時に止める封筒を返す (無ければ None)。
fn secret_block(skills: &[Skill], practices_text: &str, op: &str) -> Option<Envelope> {
    let mut hits: Vec<String> = Vec::new();
    for skill in skills {
        collect_secret_ids(
            &skill.skill_md,
            &format!("skills/{}/SKILL.md", skill.id),
            &mut hits,
        );
        for sc in &skill.scripts {
            collect_secret_ids(
                &sc.contents,
                &format!("skills/{}/{}", skill.id, sc.rel),
                &mut hits,
            );
        }
    }
    collect_secret_ids(practices_text, PRACTICES_FILE, &mut hits);
    if hits.is_empty() {
        return None;
    }

    if op == "import" {
        // import は止めて人間ゲートへ (取り込まない)。
        Some(Envelope::needs_human(
            format!(
                "Secrets detected in the bundle; not importing. {}",
                hits.join("; ")
            ),
            Gate {
                kind: "experience-import".to_string(),
                subject: "secret in bundle".to_string(),
                requires: "A human removes the secrets from the bundle and re-runs import."
                    .to_string(),
            },
        ))
    } else {
        // export は書かずに失敗 (持ち出しに秘密を載せない)。
        Some(Envelope::failed(format!(
            "Secrets detected; not exporting. Remove them first. {}",
            hits.join("; ")
        )))
    }
}

/// テキストの秘密を走査し、検出があれば `path [id]` を hits へ足す (値は載せない)。
fn collect_secret_ids(text: &str, path: &str, hits: &mut Vec<String>) {
    for SecretFinding { id, .. } in secret::scan(text) {
        hits.push(format!("{path} [{id}]"));
    }
}

/// skill を `<root>/skills/<id>/` へ書く (SKILL.md + scripts のみ)。memory/tests/owox.toml は出さない。
fn write_skill_bundle(root: &Path, skill: &Skill) -> Result<(), String> {
    let dir = root.join("skills").join(&skill.id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{} を作れない: {e}", dir.display()))?;
    let skill_md = dir.join("SKILL.md");
    std::fs::write(&skill_md, &skill.skill_md)
        .map_err(|e| format!("{} を書けない: {e}", skill_md.display()))?;
    for sc in &skill.scripts {
        // rel は "scripts/<file>"。束/正本ともこの相対で書く。
        let path = dir.join(&sc.rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("{} を作れない: {e}", parent.display()))?;
        }
        std::fs::write(&path, &sc.contents)
            .map_err(|e| format!("{} を書けない: {e}", path.display()))?;
        set_executable(&path, sc.executable);
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _executable: bool) {}

/// 取り込んだ practices を既存 practices.md へ日付を保ったまま追記する。重複行は足さない。
fn merge_practices(owox_dir: &Path, incoming_text: &str) -> Result<usize, String> {
    let incoming = Practices::from_markdown(incoming_text).map_err(|e| e.to_string())?;
    if incoming.entries.is_empty() {
        return Ok(0);
    }
    let path = owox_dir.join(PRACTICES_FILE);
    let mut body = std::fs::read_to_string(&path).unwrap_or_default();
    if !body.lines().any(|l| l.trim_start().starts_with("## ")) {
        body = "# Practices\n\n## Practices\n\n".to_string();
    } else if !body.ends_with('\n') {
        body.push('\n');
    }
    let mut merged = 0;
    for p in &incoming.entries {
        let line = format!("- {}: {}\n", p.date, p.text);
        if !body.contains(line.trim_end()) {
            body.push_str(&line);
            merged += 1;
        }
    }
    std::fs::write(&path, body).map_err(|e| format!("{} へ書けない: {e}", path.display()))?;
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Status;
    use std::path::PathBuf;

    fn tempdir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("owox-exp-{tag}-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_skill(owox: &Path, id: &str, body: &str) {
        let dir = owox.join("skills").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    #[test]
    fn export_clean_writes_bundle_and_needs_human() {
        let owox = tempdir("src");
        write_skill(
            &owox,
            "tidy",
            "---\nname: tidy\ndescription: tidy code\n---\nSteps.\n",
        );
        // memory/tests は束へ出さない。
        std::fs::write(
            owox.join("skills/tidy/memory.md"),
            "- 20260601: project secret note\n",
        )
        .unwrap();
        std::fs::write(
            owox.join("practices.md"),
            "## Practices\n- 20260614: add a test\n",
        )
        .unwrap();

        let out = tempdir("out");
        let env = export(&owox, &out);
        assert_eq!(env.status, Status::NeedsHuman);
        assert!(out.join("skills/tidy/SKILL.md").is_file());
        assert!(!out.join("skills/tidy/memory.md").exists());
        assert!(out.join("practices.md").is_file());
    }

    #[test]
    fn export_excludes_knowledge() {
        let owox = tempdir("src-k");
        write_skill(
            &owox,
            "tidy",
            "---\nname: tidy\ndescription: tidy code\n---\nSteps.\n",
        );
        // 調査知識はドメイン固有。束へ出さない。
        let kdir = owox.join("knowledge");
        std::fs::create_dir_all(&kdir).unwrap();
        std::fs::write(
            kdir.join("20260616-topic.md"),
            "# topic\n\n## Researched on\n\n20260616\n\n## Summary\n\nfinding\n\n## Status\n\ncurrent\n",
        )
        .unwrap();

        let out = tempdir("out-k");
        let env = export(&owox, &out);
        assert_eq!(env.status, Status::NeedsHuman);
        // knowledge ディレクトリは束に作られない。
        assert!(!out.join("knowledge").exists());
        let excluded = env.data.unwrap()["excluded"].to_string();
        assert!(excluded.contains("knowledge"));
    }

    #[test]
    fn export_with_secret_fails_and_writes_nothing() {
        let owox = tempdir("src2");
        write_skill(
            &owox,
            "leaky",
            "---\nname: leaky\ndescription: x\n---\npassword = \"s3cr3t_value_1234567890\"\n",
        );
        let out = tempdir("out2");
        let env = export(&owox, &out);
        assert_eq!(env.status, Status::Failed);
        assert!(!out.join("skills/leaky/SKILL.md").exists());
    }

    #[test]
    fn import_clean_writes_drafts_and_merges_practices() {
        // 束を作る。
        let bundle = tempdir("bundle");
        write_skill(
            &bundle,
            "tidy",
            "---\nname: tidy\ndescription: x\n---\nSteps.\n",
        );
        std::fs::write(
            bundle.join("practices.md"),
            "## Practices\n- 20260610: prefer small diffs\n",
        )
        .unwrap();

        let owox = tempdir("dst");
        let env = import(&owox, &bundle);
        assert_eq!(env.status, Status::Ok);
        // draft で入る (owox.toml は持たせない)。
        assert!(owox.join("skills/tidy/SKILL.md").is_file());
        assert!(!owox.join("skills/tidy/owox.toml").exists());
        // practices がマージされ日付が保たれる。
        let p =
            Practices::from_markdown(&std::fs::read_to_string(owox.join("practices.md")).unwrap())
                .unwrap();
        assert!(p.entries.iter().any(|e| e.date == "20260610"));
    }

    #[test]
    fn import_with_secret_is_human_gate_and_imports_nothing() {
        let bundle = tempdir("bundle2");
        write_skill(
            &bundle,
            "leaky",
            "---\nname: leaky\ndescription: x\n---\ntoken: ghp_abcdefghijklmnopqrstuvwxyz0123456789\n",
        );
        let owox = tempdir("dst2");
        let env = import(&owox, &bundle);
        assert_eq!(env.status, Status::NeedsHuman);
        assert!(!owox.join("skills/leaky/SKILL.md").exists());
    }
}
