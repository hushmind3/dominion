//! Native Rust SDK executable entry point. TypeScript is not required.
use dominion_harness::openai::OpenAiProvider;
use dominion_harness::sdk::{AgentSdk, CommandVerifier, FileJournal, RunStatus, Session};
use dominion_harness::tools::{ListDir, PatchFile, ReadFile, Shell, Workspace, WriteFile};
use dominion_harness::{Decision, Limits, Permission, Policy, Tool, ToolSpec};
use std::path::PathBuf;

struct CliPolicy { allow_write: bool, allow_shell: bool }
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
    args.iter().position(|item| item == name)
        .and_then(|index| args.get(index + 1)).cloned()
}
fn enabled(args: &[String], name: &str) -> bool { args.iter().any(|item| item == name) }

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if enabled(&args, "--help") {
        eprintln!("dominion-harness --base-url URL --model MODEL --task TEXT [--workdir DIR]
  [--allow-write] [--allow-shell] [--verify-command COMMAND]
  [--checkpoint FILE] [--resume] [--approve-call ID] [--max-rounds N]
  The provider key is read from DOMINION_HARNESS_KEY (or --api-key).
  --resume requires the original --checkpoint FILE; no tool is replayed after an uncertain crash.");
        return;
    }
    let base_url = arg(&args, "--base-url")
        .or_else(|| std::env::var("DOMINION_HARNESS_BASE_URL").ok())
        .unwrap_or_default();
    let model = arg(&args, "--model").unwrap_or_default();
    let task = arg(&args, "--task").unwrap_or_default();
    let resume = enabled(&args, "--resume");
    if base_url.is_empty() || model.is_empty() || (!resume && task.trim().is_empty()) {
        eprintln!("--base-url, --model and --task are required for a new session.");
        std::process::exit(2);
    }
    let workdir = arg(&args, "--workdir").map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let workspace = match Workspace::new(&workdir) {
        Ok(workspace) => workspace,
        Err(error) => { eprintln!("{error}"); std::process::exit(2); }
    };
    let allow_write = enabled(&args, "--allow-write");
    let allow_shell = enabled(&args, "--allow-shell");
    let verify_command = arg(&args, "--verify-command");
    if verify_command.is_some() && !allow_shell {
        eprintln!("--verify-command requires --allow-shell");
        std::process::exit(2);
    }
    let mut tools: Vec<Box<dyn Tool + Send>> = vec![
        Box::new(ReadFile(Workspace::new(&workdir).unwrap())),
        Box::new(ListDir(Workspace::new(&workdir).unwrap())),
    ];
    if allow_write {
        tools.push(Box::new(WriteFile(Workspace::new(&workdir).unwrap())));
        tools.push(Box::new(PatchFile(Workspace::new(&workdir).unwrap())));
    }
    if allow_shell { tools.push(Box::new(Shell { workspace, timeout_secs: 300 })); }
    let specs = tools.iter().map(|tool| tool.spec()).collect();
    let key = arg(&args, "--api-key")
        .or_else(|| std::env::var("DOMINION_HARNESS_KEY").ok())
        .unwrap_or_default();
    let mut sdk = AgentSdk::new(
        Box::new(OpenAiProvider::new(base_url, key, model).with_tools(specs)),
        Box::new(CliPolicy { allow_write, allow_shell }),
        Limits {
            max_rounds: arg(&args, "--max-rounds").and_then(|v| v.parse().ok()).unwrap_or(24),
            max_tool_calls: arg(&args, "--max-tool-calls").and_then(|v| v.parse().ok()).unwrap_or(80),
            ..Limits::default()
        },
    );
    for tool in tools { sdk.register(tool); }
    if let Some(command) = verify_command {
        sdk.set_verifier(Box::new(CommandVerifier {
            shell: Shell { workspace: Workspace::new(&workdir).unwrap(), timeout_secs: 300 },
            command,
        }));
    }
    let checkpoint = arg(&args, "--checkpoint").map(FileJournal::new);
    if resume && checkpoint.is_none() {
        eprintln!("--resume requires --checkpoint FILE");
        std::process::exit(2);
    }
    let system = arg(&args, "--system").unwrap_or_else(|| {
        "You are the DOMINION coding agent. Respect the user's exact goal and settings. \
         Inspect files before editing; use real tool results as evidence. \
         When verification fails, repair and verify again. Never report unverified work as complete."
            .to_string()
    });
    let mut session = if resume {
        match checkpoint.as_ref().unwrap().load() {
            Ok(Some(session)) => session,
            Ok(None) => { eprintln!("checkpoint not found"); std::process::exit(2); }
            Err(error) => { eprintln!("{error}"); std::process::exit(2); }
        }
    } else {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        Session::new(format!("{}-{stamp}", std::process::id()), system, task)
    };
    if let Some(journal) = checkpoint { sdk.set_journal(journal); }
    if let Some(call_id) = arg(&args, "--approve-call") { sdk.approve_once(call_id); }
    if let Err(error) = sdk.run(&mut session) {
        eprintln!("Rust SDK execution failure: {error}");
        std::process::exit(1);
    }
    for event in &session.events {
        eprintln!("[{}] {}", event.kind, event.detail);
    }
    if let Some(content) = session.final_text() { println!("{content}"); }
    eprintln!("status={:?} rounds={} tool_calls={} session={}",
        session.status, session.rounds, session.tool_calls, session.id);
    if session.status != RunStatus::Completed { std::process::exit(1); }
}
