use mcp_bridge::server::FuryBuilderServer;
use rmcp::transport::stdio;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() {
    // stdout is the JSON-RPC channel; diagnostics must go to stderr only.
    eprintln!("FuryBuilder MCP bridge listening on stdio");

    let running = match FuryBuilderServer::new().serve(stdio()).await {
        Ok(running) => running,
        Err(error) => {
            eprintln!("FuryBuilder MCP bridge failed to start: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = running.waiting().await {
        eprintln!("FuryBuilder MCP bridge stopped: {error}");
    }
}
