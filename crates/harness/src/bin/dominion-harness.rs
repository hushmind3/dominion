use dominion_harness::openai::OpenAiProvider;
use dominion_harness::tools::{ListDir, ReadFile, Shell, Workspace, WriteFile};
use dominion_harness::{Decision, Event, Harness, Limits, Permission, Policy, ToolSpec};
use std::path::PathBuf;

/// CLI policy: reads always allowed; writes and shell only when the operator
/// passed the matching flag. Anything else is denied, never silently run.
struct CliPolicy {
    allow_write: bool,
    allow_shell: bool,
}

impl Policy for CliPolicy {
    fn decide(&self, spec: &ToolSpec) -> Decision {
        match spec.permission {
            Permission::Read => Decision::Allow,
            Permission::Write if self.allow_write => Decision::Allow,
            Permission::Execute if self.allow_shell => Decision::Allow,
            _ => Decision::Deny,
        }
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|item| item == name).and_then(|index| args.get(index + 1)).cloned()
}

fn enabled(args: &[String], name: &str) -> bool {
    args.iter().any(|item| item == name)
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let base_url = arg(&args, "--base-url")
        .or_else(|| std::env::var("DOMINION_HARNESS_BASE_URL").ok())
        .unwrap_or_default();
    if base_url.is_empty() {
        eprintln!(
            "usage: dominion-harness --base-url URL --model MODEL --task TEXT [--workdir DIR] \
             [--max-rounds N] [--allow-write] [--allow-shell]\n\
             api key: --api-key or DOMINION_HARNESS_KEY"
        );
        std::process::exit(2);
    }
    let model = arg(&args, "--model").unwrap_or_default();
    let task = arg(&args, "--task").unwrap_or_default();
    if model.is_empty() || task.is_empty() {
        eprintln!("--model과 --task가 필요합니다.");
        std::process::exit(2);
    }
    let key = arg(&args, "--api-key")
        .or_else(|| std::env::var("DOMINION_HARNESS_KEY").ok())
        .unwrap_or_default();
    let workdir = arg(&args, "--workdir").map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let max_rounds = arg(&args, "--max-rounds").and_then(|value| value.parse().ok()).unwrap_or(12);
    let system = arg(&args, "--system").unwrap_or_else(|| {
        "You are a coding agent. Use tools to inspect and edit the workspace. \
         Never claim a command succeeded without its tool result. Work only inside the workspace."
            .to_string()
    });

    let workspace = match Workspace::new(&workdir) {
        Ok(workspace) => workspace,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let policy = CliPolicy {
        allow_write: enabled(&args, "--allow-write"),
        allow_shell: enabled(&args, "--allow-shell"),
    };
    let mut harness = Harness::new(
        OpenAiProvider::new(base_url.clone(), key.clone(), model.clone()),
        Box::new(policy),
        Limits { max_rounds, ..Limits::default() },
    );
    harness.register(Box::new(ReadFile(Workspace::new(&workdir).unwrap())));
    harness.register(Box::new(ListDir(Workspace::new(&workdir).unwrap())));
    if enabled(&args, "--allow-write") {
        harness.register(Box::new(WriteFile(Workspace::new(&workdir).unwrap())));
    }
    if enabled(&args, "--allow-shell") {
        harness.register(Box::new(Shell { workspace, timeout_secs: 300 }));
    }
    // Declare to the provider only the tools that exist in this run, so the
    // model cannot request a capability the operator did not enable.
    let declared = harness.tool_specs();
    *harness.provider_mut() = OpenAiProvider::new(base_url, key, model).with_tools(declared);

    let report = harness.run(&system, &task);
    for event in &report.events {
        match event {
            Event::RoundStarted(round) => eprintln!("[round {round}]"),
            Event::AssistantText(text) => eprintln!("[assistant] {text}"),
            Event::ToolStarted { name, .. } => eprintln!("[tool] {name}"),
            Event::ToolFinished { name, ok, preview } => eprintln!("[tool] {name} ok={ok} {preview}"),
            Event::Denied { name, permission } => eprintln!("[denied] {name} ({permission:?})"),
            Event::ContextTrimmed { removed } => eprintln!("[context] trimmed {removed}"),
        }
    }
    if let Some(last) = report.messages.iter().rev().find(|message| message.role == dominion_harness::Role::Assistant) {
        println!("{}", last.content);
    }
    eprintln!(
        "stop={:?} rounds={} tool_calls={}",
        report.stop, report.rounds, report.tool_calls
    );
    if !report.completed() {
        std::process::exit(1);
    }
}
