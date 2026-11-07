//! Connector tools called through Parzi's MCP server: the same workspace
//! hooks, approvals and per-tool choices as Parzi's own tools.

use serde_json::{json, Value};

use super::{Refusal, ToolHost};
use crate::handler::RunEvent;
use crate::mcp_relay::{relay_result, text_of, Relay};
use crate::tools::ApprovalMode;

fn said(t: impl Into<String>) -> Vec<Value> {
    vec![json!({"type": "text", "text": t.into()})]
}

impl ToolHost {
    /// This run's connector tools: what each enabled connector offered when
    /// last listed. None when agents are read-only: they could call none.
    pub(crate) async fn relay(&self) -> &Relay {
        self.relay
            .get_or_init(|| async {
                if self.p.mode == ApprovalMode::Deny {
                    return Relay::default();
                }
                let tools = &self.p.tools;
                tools.mcp.note_folder(&tools.cwd);
                let mut found = tools.mcp.session_tools(crate::mcp::COLD_LIST_BUDGET).await;
                found.retain(|t| {
                    tools.connector_allowed(&t.server)
                        && tools.connector_mode(&t.server, &t.name) != Some(ApprovalMode::Deny)
                });
                Relay::build(found)
            })
            .await
    }

    /// Runs the connector tool an agent named `asked`; `None` when this run
    /// offers no tool of that name.
    pub(crate) async fn call_relayed(
        &self,
        asked: &str,
        args: &Value,
    ) -> Option<(bool, Vec<Value>)> {
        let hit = self.relay().await.find(asked)?;
        let (server, tool) = (hit.server.clone(), hit.tool.clone());
        let qualified = format!("{server}.{tool}");
        let id = uuid::Uuid::new_v4().to_string();
        let guarded = async {
            let (denied, warnings) =
                crate::hooks::pre_tool(&self.p.store, &self.p.session_id, &qualified, args).await;
            for w in warnings {
                self.p.sink.emit(RunEvent::Notice { text: w });
            }
            if let Some(reason) = denied {
                return (
                    false,
                    said(format!("blocked by a workspace hook: {reason}")),
                );
            }
            if let Err(why) = self
                .connector_approved(&id, &server, &tool, &qualified, args)
                .await
            {
                return (false, said(super::refusal_text(&format!("`{asked}`"), why)));
            }
            match self
                .p
                .tools
                .mcp
                .call_raw(&server, &tool, args.clone())
                .await
            {
                Ok(result) => relay_result(&result),
                Err(e) => (false, said(e.to_string())),
            }
        };
        let (ok, content) = tokio::select! {
            done = guarded => done,
            () = self.p.cancel.cancelled() => {
                return Some((false, said("stopped: the run was cancelled")));
            }
        };
        let text = text_of(&content);
        for n in crate::hooks::post_tool(
            &self.p.store,
            &self.p.session_id,
            &qualified,
            args,
            ok,
            &text,
        )
        .await
        {
            self.p.sink.emit(RunEvent::Notice { text: n });
        }
        Some((ok, content))
    }

    /// A connector tool runs when its own choice says so, else as the
    /// session's approval mode says; full access asks for nothing but still
    /// honours a tool switched to blocked.
    async fn connector_approved(
        &self,
        id: &str,
        server: &str,
        tool: &str,
        qualified: &str,
        args: &Value,
    ) -> std::result::Result<(), Refusal> {
        if self.p.mode == ApprovalMode::Deny {
            return Err(Refusal::ReadOnly);
        }
        if !self.p.tools.connector_allowed(server) {
            return Err(Refusal::NotEnabled);
        }
        let choice = self.p.tools.connector_mode(server, tool);
        if choice == Some(ApprovalMode::Deny) {
            return Err(Refusal::ConnectorOff);
        }
        if self.p.full {
            return Ok(());
        }
        match choice.unwrap_or(self.p.mode) {
            ApprovalMode::Auto => Ok(()),
            ApprovalMode::Ask => self.ask(id, qualified, args).await,
            ApprovalMode::Deny => Err(Refusal::ConnectorOff),
        }
    }
}
