//! Built-in model presets (SPEC §6.2, §6.3) and the config defaults.
//!
//! Paths are this author's machine (SPEC §1.1) and editable in Settings → Models.

use super::types::{AiConfig, ConnectionMode, Preset, Template, ThinkingSwitch};

pub const BONSAI_BIN: &str = "D:/MODELS/bonsai/runtime/prism-b12128-d7b2896/llama-server.exe";
pub const DEFAULT_PORT: u32 = 8080;
/// Bump when a built-in preset's template, thinking switch or stop tokens change.
/// 2: Gemma 4 uses `<|turn>` markers and `<turn|>` as its stop token.
/// 3: Bonsai 9B is the default (bake-off, 2026-11-05) with 2 slots of 16K; the Gemma and
///    Nemo presets are retired.
pub const PRESET_REVISION: u32 = 3;
pub const DEFAULT_PRESET: &str = "bonsai-9b-pq2";
/// Built-in presets retired in revision 3. Saved copies are dropped on upgrade.
const RETIRED_PRESETS: [&str; 3] = ["gemma4-26b-a4b", "gemma4-e4b", "nemo-12b"];

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[allow(clippy::too_many_arguments)]
fn preset(
    id: &str,
    name: &str,
    notes: &str,
    template: Template,
    thinking: ThinkingSwitch,
    stop: &[&str],
    context_size: u32,
    parallel: u32,
    model: &str,
    extra: &[&str],
) -> Preset {
    Preset {
        id: id.into(),
        name: name.into(),
        notes: notes.into(),
        template,
        thinking,
        stop: args(stop),
        context_size,
        parallel,
        binary: BONSAI_BIN.into(),
        model: model.into(),
        args: args(extra),
        sampling: vec![],
        builtin: true,
    }
}

pub fn builtin_presets() -> Vec<Preset> {
    vec![
        preset(
            "bonsai-27b-ptq1",
            "Ternary Bonsai 27B (PTQ1_0)",
            "PrismML fork. Smallest 27B file (5.9 GB). Thinking off by prefill.",
            Template::ChatMl,
            ThinkingSwitch::Prefill,
            &["<|im_end|>"],
            16384,
            2,
            "D:/MODELS/bonsai/Ternary-Bonsai-2-27B-PTQ1_0.gguf",
            &[
                "--jinja",
                "-ngl",
                "99",
                "-fa",
                "on",
                "--no-webui",
                "--reasoning",
                "off",
            ],
        ),
        preset(
            "bonsai-27b-pq2",
            "Ternary Bonsai 27B (PQ2_0)",
            "PrismML fork. 7.2 GB file.",
            Template::ChatMl,
            ThinkingSwitch::Prefill,
            &["<|im_end|>"],
            16384,
            2,
            "D:/MODELS/bonsai/Ternary-Bonsai-2-27B-PQ2_0.gguf",
            &[
                "--jinja",
                "-ngl",
                "99",
                "-fa",
                "on",
                "--no-webui",
                "--reasoning",
                "off",
            ],
        ),
        preset(
            "bonsai-9b-pq2",
            "Ternary Bonsai 9B (PQ2_0, default)",
            "PrismML fork. 2.4 GB file. Best bake-off result, chosen by the author. 2 slots of 16K.",
            Template::ChatMl,
            ThinkingSwitch::Prefill,
            &["<|im_end|>"],
            32768,
            2,
            "D:/MODELS/bonsai/Ternary-Bonsai-2-9B-PQ2_0.gguf",
            &[
                "--jinja",
                "-ngl",
                "99",
                "-fa",
                "on",
                "--no-webui",
                "--reasoning",
                "off",
            ],
        ),
    ]
}

pub fn default_config() -> AiConfig {
    AiConfig {
        schema_version: 1,
        enabled: true,
        mode: ConnectionMode::Managed,
        port: DEFAULT_PORT,
        external_url: "http://127.0.0.1:8081".into(),
        preset_id: DEFAULT_PRESET.into(),
        auto_start: false,
        idle_seconds: 20,
        context_log_limit: 500,
        presets: builtin_presets(),
        preset_revision: PRESET_REVISION,
    }
}

/// Merge saved presets over the built-ins: the author's edits win, and any
/// built-in missing from the saved list (a newer app version) is added.
pub fn merge_presets(saved: Vec<Preset>) -> Vec<Preset> {
    let mut out = saved;
    for b in builtin_presets() {
        if !out.iter().any(|p| p.id == b.id) {
            out.push(b);
        }
    }
    out
}

/// Bring a saved config up to the current built-in presets. The behavioural
/// fields (template, thinking switch, stop tokens, name, notes) follow the app,
/// so a fix here reaches existing configs. The author's paths, arguments,
/// context size, slots and sampling stay as they are.
pub fn upgrade(cfg: &mut AiConfig) {
    if cfg.preset_revision >= PRESET_REVISION {
        return;
    }
    if cfg.preset_revision < 3 {
        cfg.presets
            .retain(|p| !RETIRED_PRESETS.contains(&p.id.as_str()));
        if RETIRED_PRESETS.contains(&cfg.preset_id.as_str()) {
            cfg.preset_id = DEFAULT_PRESET.into();
        }
        // Ghost text sends up to ~8K tokens, so each of the two slots needs 16K.
        // Only the old built-in default is raised; a size the author chose stays.
        if let Some(p) = cfg.presets.iter_mut().find(|p| p.id == DEFAULT_PRESET)
            && p.context_size == 16384
        {
            p.context_size = 32768;
        }
    }
    for b in builtin_presets() {
        if let Some(p) = cfg.presets.iter_mut().find(|p| p.id == b.id) {
            p.template = b.template;
            p.thinking = b.thinking;
            p.stop = b.stop;
            p.name = b.name;
            p.notes = b.notes;
            p.builtin = true;
        }
    }
    cfg.preset_revision = PRESET_REVISION;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A saved copy of a preset that revision 3 retired.
    fn old_preset(id: &str, template: Template) -> Preset {
        Preset {
            id: id.into(),
            name: id.into(),
            notes: String::new(),
            template,
            thinking: ThinkingSwitch::None,
            stop: vec![],
            context_size: 16384,
            parallel: 2,
            binary: BONSAI_BIN.into(),
            model: "D:/MODELS/old.gguf".into(),
            args: vec![],
            sampling: vec![],
            builtin: true,
        }
    }

    #[test]
    fn builtins_are_the_bonsai_family_with_unique_ids() {
        let p = builtin_presets();
        let mut ids: Vec<_> = p.iter().map(|p| p.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), p.len());
        assert_eq!(p.len(), 3);
        assert!(p.iter().all(|p| p.template == Template::ChatMl));
        assert_eq!(DEFAULT_PRESET, "bonsai-9b-pq2");
        let d = p.iter().find(|p| p.id == DEFAULT_PRESET).unwrap();
        // Two slots of 16K: ghost text may send ~8K tokens.
        assert_eq!(d.context_size / d.parallel, 16384);
    }

    #[test]
    fn upgrade_retires_gemma_and_nemo_and_moves_the_selection() {
        let mut cfg = default_config();
        cfg.preset_revision = 2;
        cfg.preset_id = "gemma4-26b-a4b".into();
        cfg.presets
            .push(old_preset("gemma4-26b-a4b", Template::Gemma));
        cfg.presets.push(old_preset("gemma4-e4b", Template::Gemma));
        cfg.presets.push(old_preset("nemo-12b", Template::Mistral));
        // The author's own preset is not ours to remove.
        cfg.presets.push(old_preset("my-gemma", Template::Gemma));
        let nine = cfg
            .presets
            .iter_mut()
            .find(|p| p.id == DEFAULT_PRESET)
            .unwrap();
        nine.context_size = 16384;
        upgrade(&mut cfg);
        assert_eq!(cfg.preset_revision, PRESET_REVISION);
        assert_eq!(cfg.preset_id, DEFAULT_PRESET);
        let ids: Vec<_> = cfg.presets.iter().map(|p| p.id.as_str()).collect();
        assert!(!ids.contains(&"gemma4-26b-a4b"));
        assert!(!ids.contains(&"gemma4-e4b"));
        assert!(!ids.contains(&"nemo-12b"));
        assert!(ids.contains(&"my-gemma"));
        let nine = cfg.presets.iter().find(|p| p.id == DEFAULT_PRESET).unwrap();
        assert_eq!(nine.context_size, 32768);
        // Already current: untouched.
        let before = cfg.clone();
        upgrade(&mut cfg);
        assert_eq!(cfg, before);
    }

    #[test]
    fn upgrade_keeps_the_authors_paths_and_chosen_context_size() {
        let mut cfg = default_config();
        cfg.preset_revision = 2;
        let g = cfg
            .presets
            .iter_mut()
            .find(|p| p.id == DEFAULT_PRESET)
            .unwrap();
        g.stop = vec!["<wrong>".into()];
        g.thinking = ThinkingSwitch::None;
        g.model = "E:/my/model.gguf".into();
        g.args = vec!["-ngl".into(), "7".into()];
        g.context_size = 24576;
        upgrade(&mut cfg);
        let g = cfg.presets.iter().find(|p| p.id == DEFAULT_PRESET).unwrap();
        assert_eq!(g.stop, vec!["<|im_end|>".to_string()]);
        assert_eq!(g.thinking, ThinkingSwitch::Prefill);
        assert_eq!(g.model, "E:/my/model.gguf");
        assert_eq!(g.args, vec!["-ngl".to_string(), "7".to_string()]);
        assert_eq!(g.context_size, 24576);
    }

    #[test]
    fn merge_keeps_edits_and_adds_missing_builtins() {
        let mut edited = builtin_presets()[0].clone();
        edited.context_size = 4096;
        let merged = merge_presets(vec![edited]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].context_size, 4096);
    }
}
