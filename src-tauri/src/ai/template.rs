//! Raw-prompt rendering for the instruct templates Margin supports (SPEC §6.3).
//!
//! Ghost text needs an assistant turn that is already started (SPEC §7.4 [6]),
//! so Margin builds the prompt string itself and sends it to `/completion`.
//! The server adds the BOS token. `Capabilities::template_warning` flags any
//! model whose own template renders differently from ours.

use super::types::{Preset, Prompt, Template, ThinkingSwitch};

/// The text that closes an empty reasoning block, so the model answers directly.
pub const EMPTY_THINK: &str = "<think>\n\n</think>\n\n";

/// A chat as the server's template renderer would see it, without the
/// assistant prefix: used to compare with `/apply-template`.
pub fn render_chat_only(template: Template, system: &str, user: &str) -> String {
    match template {
        Template::ChatMl => format!(
            "<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n"
        ),
        // Gemma 4 (verified against llama-server /apply-template, 2026-11-05).
        // Thinking is off simply by leaving out the `<|think|>` line it would add.
        Template::Gemma => {
            format!("<|turn>system\n{system}<turn|>\n<|turn>user\n{user}<turn|>\n<|turn>model\n")
        }
        Template::Mistral => format!("[INST] {system}\n\n{user}[/INST]"),
    }
}

/// The full raw prompt: chat, thinking switch, then the prefilled assistant text.
pub fn render(preset: &Preset, prompt: &Prompt) -> String {
    let mut out = render_chat_only(preset.template, &prompt.system, &prompt.user);
    if preset.thinking == ThinkingSwitch::Prefill {
        out.push_str(EMPTY_THINK);
    }
    out.push_str(&prompt.assistant_prefix);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::presets::builtin_presets;

    fn preset(id: &str) -> Preset {
        builtin_presets()
            .into_iter()
            .find(|p| p.id == id)
            .expect("preset exists")
    }

    fn prompt() -> Prompt {
        Prompt {
            system: "Be quiet.".into(),
            user: "Scene.".into(),
            assistant_prefix: "She opened the".into(),
            sections: vec![],
        }
    }

    #[test]
    fn chatml_with_thinking_prefill_ends_with_the_assistant_prefix() {
        let s = render(&preset("bonsai-9b-pq2"), &prompt());
        assert!(s.starts_with("<|im_start|>system\nBe quiet.<|im_end|>\n"));
        assert!(s.ends_with("<|im_start|>assistant\n<think>\n\n</think>\n\nShe opened the"));
    }

    #[test]
    fn templates_without_thinking_add_no_think_block() {
        // A preset whose model does not think (e.g. one the author adds for Gemma).
        let mut p = preset("bonsai-9b-pq2");
        p.thinking = ThinkingSwitch::None;
        let s = render(&p, &prompt());
        assert!(!s.contains("<think>"));
        assert!(s.ends_with("She opened the"));
    }

    #[test]
    fn gemma_and_mistral_open_the_model_turn() {
        let g = render_chat_only(Template::Gemma, "S", "U");
        assert_eq!(
            g,
            "<|turn>system\nS<turn|>\n<|turn>user\nU<turn|>\n<|turn>model\n"
        );
        let m = render_chat_only(Template::Mistral, "S", "U");
        assert_eq!(m, "[INST] S\n\nU[/INST]");
    }
}
