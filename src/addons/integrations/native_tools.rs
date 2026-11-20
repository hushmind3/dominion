//! Optional, separately owned native tool connectors. Core admission,
//! approvals and effect receipts are always enforced before invoking this.
use super::*;
pub(super) async fn execute(ctx:&ToolContext<'_>,name:&str,args:&Value)->Result<Value,String>{
    match name {
        "browser_control" => {
            let action = required_arg(args, "action")?;
            let owner_id=format!("{}:{}",ctx.agent_id.unwrap_or("user"),ctx.session_id);
            crate::engine::browser::execute_for_owner(
                &action,
                args,
                &ctx.state.config.data_dir,
                ctx.workspace,
                &owner_id,
            )
            .await
        }
        "web_search" => {
            let query = required_arg(args, "query")?;
            web_search(&query).await
        }
        "http_fetch" => {
            let url = required_arg(args, "url")?;
            http_fetch(&url).await
        }
        "memory_store"|"memory_search"|"skill_view"|"skill_propose" =>
            crate::engine::extensions::knowledge_tools::execute(ctx,name,args),
        "workspace_git_status"=>crate::engine::delegation_worktree::inspect(ctx.workspace,false).await,
        "workspace_git_diff"=>crate::engine::delegation_worktree::inspect(ctx.workspace,true).await,
        "delegate_task" => delegate(ctx, args).await,
        "task_integrate" => crate::engine::delegation_worktree::integrate(ctx,args).await,
        "task_status" => task_payload(ctx, args, false),
        "task_result" => task_payload(ctx, args, true),
        "account_status" => Ok(json!({
            "providers": crate::engine::oauth::status(ctx.state),
        })),
        "github_api" => github_api(ctx, args).await,
        "github_repositories" => github_repositories::list(ctx,args).await,
        "github_file_read" => github_file_read(ctx,args).await,
        "github_file_update" => github_file_update(ctx,args).await,
        "github_file_replace" => github_replace::replace(ctx,args).await,
        other=>Err(format!("Unknown tool: {other}")),
    }
}
