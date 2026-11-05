//! Built-in model presets (SPEC §6.2, §6.3) and the config defaults.
//!
//! Paths are this author's machine (SPEC §1.1) and editable in Settings → Models.

use super::types::{AiConfig, ConnectionMode, Preset, Template, ThinkingSwitch};

pub const BONSAI_BIN: &str = "D:/MODELS/bonsai/runtime/prism-b12128-d7b2896/llama-server.exe";
pub const DEFAULT_PORT: u32 = 8080;
/// Bump when a built-in preset's template, thinking switch or stop tokens change.
/// 2: Gemma 4 uses `<|turn>` markers and `<turn|>` as its stop token.
pub const PRESET_REVISION: u32 = 2;
pub const DEFAULT_PRESET: &str = "gemma4-26b-a4b";

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
            "gemma4-26b-a4b",
            "Gemma 4 26B-A4B (default)",
            "MoE, ~4B active. Experts partly offloaded to RAM (--n-cpu-moe) to fit 12 GB.",
            Template::Gemma,
            ThinkingSwitch::TemplateKwarg,
            &["<turn|>"],
            32768,
            2,
            "D:/MODELS/gemma4/gemma-4-26B-A4B-it-UD-Q4_K_M.gguf",
            &[
                "--jinja",
                "-ngl",
                "99",
                "--n-cpu-moe",
                "12",
                "-fa",
                "on",
                "--cache-type-k",
                "q8_0",
                "--cache-type-v",
                "q8_0",
            ],
        ),
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
            "Ternary Bonsai 9B (PQ2_0)",
            "PrismML fork. 2.4 GB file; the fast end of the Bonsai family.",
            Template::ChatMl,
            ThinkingSwitch::Prefill,
            &["<|im_end|>"],
            16384,
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
        preset(
            "nemo-12b",
            "Mistral Nemo 12B (Q5_K_M)",
            "Fully on the GPU. The most predictable latency.",
            Template::Mistral,
            ThinkingSwitch::None,
            &["</s>"],
            16384,
            2,
            "D:/MODELS/nemo/Mistral-Nemo-Instruct-2407-Q5_K_M.gguf",
            &[
                "--jinja",
                "-ngl",
                "99",
                "-fa",
                "on",
                "--cache-type-k",
                "q8_0",
                "--cache-type-v",
                "q8_0",
                "--no-webui",
            ],
        ),
        preset(
            "gemma4-e4b",
            "Gemma 4 E4B (Q4_K_M)",
            "Fastest; weakest prose and memory quality.",
            Template::Gemma,
            ThinkingSwitch::TemplateKwarg,
            &["<turn|>"],
            16384,
            2,
            "D:/MODELS/gemma4/gemma-4-E4B-it-Q4_K_M.gguf",
            &["--jinja", "-ngl", "99", "-fa", "on", "--no-webui"],
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

    #[test]
    fn builtins_cover_the_spec_candidates_with_unique_ids() {
        let p = builtin_presets();
        let mut ids: Vec<_> = p.iter().map(|p| p.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), p.len());
        assert!(p.iter().any(|p| p.id == DEFAULT_PRESET));
        assert_eq!(p.len(), 6);
    }

    #[test]
    fn upgrade_fixes_behaviour_but_keeps_the_authors_paths() {
        let mut cfg = default_config();
        cfg.preset_revision = 1;
        let g = &mut cfg.presets[0];
        g.stop = vec!["<end_of_turn>".into()];
        g.thinking = ThinkingSwitch::None;
        g.model = "E:/my/model.gguf".into();
        g.args = vec!["-ngl".into(), "7".into()];
        upgrade(&mut cfg);
        assert_eq!(cfg.preset_revision, PRESET_REVISION);
        let g = &cfg.presets[0];
        assert_eq!(g.stop, vec!["<turn|>".to_string()]);
        assert_eq!(g.thinking, ThinkingSwitch::TemplateKwarg);
        assert_eq!(g.model, "E:/my/model.gguf");
        assert_eq!(g.args, vec!["-ngl".to_string(), "7".to_string()]);
        // Already current: untouched.
        let before = cfg.clone();
        upgrade(&mut cfg);
        assert_eq!(cfg, before);
    }

    #[test]
    fn merge_keeps_edits_and_adds_missing_builtins() {
        let mut edited = builtin_presets()[0].clone();
        edited.context_size = 4096;
        let merged = merge_presets(vec![edited]);
        assert_eq!(merged.len(), 6);
        assert_eq!(merged[0].context_size, 4096);
    }
}
