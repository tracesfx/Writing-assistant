//! Shared AI types (SPEC §6). Everything here crosses the IPC boundary, so
//! numbers are `u32`/`f64` (specta refuses 64-bit integers).

use serde::{Deserialize, Serialize};
use specta::Type;

/// What a request is for. Drives sampling defaults and the log label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Ghost,
    Ideas,
    Ask,
    /// Margin panel → Phrasing: alternatives for a selection.
    Phrasing,
    Extraction,
    /// Chapter and book summaries (idle work).
    Summary,
    /// Model panel "try it", bake-off and other non-book requests.
    Test,
}

/// Scheduler priority (SPEC §6.4). Lower number = more urgent.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
pub enum Priority {
    /// Ghost text: slot 0, cancelled by any keystroke.
    P0,
    /// On request: slot 1, pre-empts P2.
    P1,
    /// Idle work: slot 1, runs only while the author is idle.
    P2,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Sampling {
    pub temperature: f64,
    pub top_p: f64,
    pub min_p: f64,
    pub max_tokens: u32,
    #[serde(default)]
    pub stop: Vec<String>,
    /// A JSON schema as JSON text. The server turns it into a grammar.
    #[serde(default)]
    pub json_schema: Option<String>,
}

/// Sampling defaults per role (SPEC §6.5).
pub fn default_sampling(role: Role) -> Sampling {
    let (temperature, top_p, min_p, max_tokens, stop): (f64, f64, f64, u32, Vec<String>) =
        match role {
            Role::Ghost => (0.7, 0.9, 0.05, 16, vec!["\n".into()]),
            Role::Ideas => (0.9, 0.95, 0.05, 400, vec![]),
            Role::Ask => (0.3, 0.9, 0.05, 500, vec![]),
            Role::Phrasing => (0.9, 0.95, 0.05, 400, vec![]),
            Role::Extraction => (0.1, 1.0, 0.0, 3000, vec![]),
            Role::Summary => (0.2, 0.95, 0.02, 300, vec![]),
            Role::Test => (0.7, 0.9, 0.05, 64, vec![]),
        };
    Sampling {
        temperature,
        top_p,
        min_p,
        max_tokens,
        stop,
        json_schema: None,
    }
}

/// How the instruct template wraps a request when we send a raw prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Template {
    ChatMl,
    Gemma,
    Mistral,
}

/// How reasoning is turned off (SPEC §6.2: thinking is disabled for all roles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ThinkingSwitch {
    /// The model does not think.
    None,
    /// Raw prompts prefill an empty think block (ChatML / Qwen3 style).
    Prefill,
    /// Chat calls send `chat_template_kwargs: {enable_thinking: false}`.
    TemplateKwarg,
}

/// A prompt split into the labelled sections the context log stores (SPEC §7.4, §9.6).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    /// Role instructions and project context: stable, cached prefix.
    pub system: String,
    /// Scene context the model should read before continuing.
    pub user: String,
    /// The assistant turn, prefilled; the model continues from here.
    pub assistant_prefix: String,
    /// The same text split by label, for the Context Inspector.
    pub sections: Vec<PromptSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PromptSection {
    pub label: String,
    pub text: String,
    /// Estimated until the server tokenizes it.
    pub tokens: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    /// Request sent → first token received.
    pub ttft_ms: Option<f64>,
    pub total_ms: f64,
    pub prompt_tokens: u32,
    /// Prompt tokens served from the slot's cache.
    pub cached_tokens: u32,
    pub output_tokens: u32,
    /// Tokens per second as measured by the server, when it reports it.
    pub decode_tps: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    pub text: String,
    pub outcome: Outcome,
    pub error: Option<String>,
    pub timings: Timings,
}

/// How a ghost-text request is going (SPEC §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GhostState {
    /// Text so far; more may follow.
    Streaming,
    /// The finished suggestion.
    Done,
    /// The model had nothing worth showing (or AI is off for this document).
    Empty,
    Cancelled,
    Failed,
}

// --- Connection, health, capabilities ----------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionMode {
    Managed,
    External,
}

/// The status-bar states (SPEC §6.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AiStatus {
    Off,
    Starting,
    Ready,
    Thinking,
    /// Idle work is held back because the author is typing.
    PausedTyping,
    NotRunning,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub reachable: bool,
    /// llama-server native `/completion` works.
    pub native_completion: bool,
    /// `/v1/chat/completions` works (fallback).
    pub openai_chat: bool,
    pub tokenize: bool,
    pub slots_endpoint: bool,
    pub total_slots: u32,
    pub n_ctx: u32,
    pub model: String,
    pub build_info: String,
    pub has_chat_template: bool,
    /// The server's own rendering of a sample chat differs from ours.
    pub template_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SlotInfo {
    pub id: u32,
    pub busy: bool,
    pub n_ctx: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Vram {
    pub used_mb: u32,
    pub total_mb: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LastRequest {
    pub role: Role,
    pub outcome: Outcome,
    pub ttft_ms: Option<f64>,
    pub total_ms: f64,
}

/// Everything the Model panel shows (SPEC §6.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelPanel {
    pub status: AiStatus,
    pub detail: String,
    pub mode: ConnectionMode,
    pub endpoint: String,
    pub preset_id: String,
    pub restarts: u32,
    pub log_tail: Vec<String>,
    pub vram: Option<Vram>,
    pub slots: Vec<SlotInfo>,
    pub capabilities: Capabilities,
    pub last_request: Option<LastRequest>,
}

// --- Presets and configuration ------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub notes: String,
    pub template: Template,
    pub thinking: ThinkingSwitch,
    /// Stop sequences added to every request (the template's end-of-turn token).
    pub stop: Vec<String>,
    /// Total context across all slots (`-c`).
    pub context_size: u32,
    /// Slots (`--parallel`): 0 ghost text, 1 everything else.
    pub parallel: u32,
    /// Managed mode: the `llama-server` binary.
    pub binary: String,
    /// Managed mode: the model file.
    pub model: String,
    /// Managed mode: extra arguments, one per entry.
    pub args: Vec<String>,
    /// Per-role sampling overrides; missing roles use the defaults.
    #[serde(default)]
    pub sampling: Vec<RoleSampling>,
    /// Built-in presets can be edited but not deleted.
    #[serde(default)]
    pub builtin: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RoleSampling {
    pub role: Role,
    pub sampling: Sampling,
}

impl Preset {
    pub fn sampling_for(&self, role: Role) -> Sampling {
        self.sampling
            .iter()
            .find(|s| s.role == role)
            .map(|s| s.sampling.clone())
            .unwrap_or_else(|| default_sampling(role))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    pub schema_version: u32,
    /// The master switch (SPEC §2.10). Off = no requests, no server start.
    pub enabled: bool,
    pub mode: ConnectionMode,
    /// Managed mode port (SPEC §1.2: Margin's own server on 8080).
    pub port: u32,
    /// External mode endpoint. Must be a loopback address.
    pub external_url: String,
    pub preset_id: String,
    /// Start the managed server when the app opens.
    pub auto_start: bool,
    /// Seconds without typing before idle work (P2) may run (SPEC §6.4).
    pub idle_seconds: u32,
    /// Context-log ring size (SPEC §9.6).
    pub context_log_limit: u32,
    /// Presets as edited by the author, merged over the built-ins by id.
    pub presets: Vec<Preset>,
    /// Which revision of the built-in presets this file has seen (see `presets::upgrade`).
    #[serde(default)]
    pub preset_revision: u32,
}
