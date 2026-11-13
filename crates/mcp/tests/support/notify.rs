//! A client that hears `notifications/tools/list_changed`: the harness's
//! [`TestClient`](super::TestClient) ignores notifications, so the access
//! control tests connect through [`Fixture::connect_notified`] instead.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use coffer_core::mcp_client::ClientName;
use coffer_mcp::Server;
use coffer_mcp::registry::Registry;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{NotificationContext, RunningService};
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::Value;
use tokio::sync::Notify;

use super::Fixture;

/// How long a test waits for a notification before failing.
const WAIT: Duration = Duration::from_secs(10);

/// Counts `notifications/tools/list_changed`.
#[derive(Clone, Default)]
pub struct ListChanged {
    count: Arc<AtomicUsize>,
    notify: Arc<Notify>,
}

impl ClientHandler for ListChanged {
    async fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}

/// An MCP client that counts the tool-list changes it hears.
pub struct NotifiedClient {
    service: RunningService<RoleClient, ListChanged>,
    heard: ListChanged,
    server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    /// Starts a server for `client` with the tools in `registry`, checking
    /// its access every `poll` (`None`: only on calls), without touching
    /// `mcp_client`, and connects a client that counts list changes.
    pub async fn connect_notified(
        &self,
        client: &str,
        registry: Registry,
        poll: Option<Duration>,
    ) -> NotifiedClient {
        let server = Server::with_registry(self.open(), ClientName::new(client).unwrap(), registry)
            .unwrap()
            .with_poll_interval(poll);
        let (server_io, client_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let running = server.serve(server_io).await.unwrap();
            let _ = running.waiting().await;
        });
        let heard = ListChanged::default();
        let service = heard.clone().serve(client_io).await.unwrap();
        NotifiedClient {
            service,
            heard,
            server,
        }
    }
}

impl NotifiedClient {
    /// Calls `tool` with `args` (a JSON object), expecting a result (which
    /// may be a tool error).
    pub async fn call(&self, tool: &str, args: Value) -> CallToolResult {
        let Value::Object(args) = args else {
            panic!("arguments are a JSON object");
        };
        self.service
            .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(args))
            .await
            .unwrap()
    }

    /// The names of the tools this client sees.
    pub async fn tool_names(&self) -> Vec<String> {
        self.service
            .list_all_tools()
            .await
            .unwrap()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect()
    }

    /// How many list changes the client has heard.
    pub fn heard(&self) -> usize {
        self.heard.count.load(Ordering::SeqCst)
    }

    /// Waits until the client has heard `count` list changes; panics after
    /// a while.
    pub async fn wait_for_list_changes(&self, count: usize) {
        let wait = async {
            loop {
                let notified = self.heard.notify.notified();
                if self.heard() >= count {
                    return;
                }
                notified.await;
            }
        };
        tokio::time::timeout(WAIT, wait).await.unwrap_or_else(|_| {
            panic!(
                "heard {} tools/list_changed notifications, expected {count}",
                self.heard()
            )
        });
    }

    /// Disconnects and waits for the server to stop.
    pub async fn close(self) {
        self.service.cancel().await.unwrap();
        self.server.await.unwrap();
    }
}
