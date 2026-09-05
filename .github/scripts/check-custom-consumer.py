"""Check the custom-only normal graph and coexistence with exact zeroize 1.9."""
import json
import os
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[2]
consumer = root / "target" / "custom-consumer"
(consumer / "src").mkdir(parents=True, exist_ok=True)
(consumer / "Cargo.toml").write_text(
    '[package]\nname="sdk-custom-consumer"\nversion="0.0.0"\nedition="2021"\npublish=false\n'
    '[workspace]\n[dependencies]\n'
    'acosmi-sdk={path=' + json.dumps(str(root)) +
    ',default-features=false,features=["custom-transport","sanitize","desktop-loopback"]}\n'
    'zeroize="=1.9.0"\nasync-trait="0.1"\ntokio-util="0.7"\n'
)
(consumer / "src/main.rs").write_text("""
use std::sync::Arc;
use acosmi::{Client,Config,HttpTransport,HttpRequest,HttpResponse,TransportError,TokenSet};
struct PolicyTransport;
#[async_trait::async_trait]
impl HttpTransport for PolicyTransport {
    async fn execute(&self,_:HttpRequest,_:tokio_util::sync::CancellationToken)->Result<HttpResponse,TransportError> {
        Err(TransportError::Rejected)
    }
}
fn main()->acosmi::Result<()> {
    let client=Client::new_with_transport(Config {store:Some(Arc::new(acosmi::InMemoryTokenStore::new())),..Default::default()},Arc::new(PolicyTransport))?;
    let req=acosmi::ChatRequest::default();
    let _stream=client.chat_messages_stream_with_options("model",&req,None,Default::default());
    let mut token=TokenSet {access_token:"canary".into(),refresh_token:"canary".into(),expires_at:String::new(),scope:String::new(),client_id:String::new(),server_url:String::new()};
    zeroize::Zeroize::zeroize(&mut token);
    assert!(token.access_token.is_empty());
    assert_eq!(acosmi::VERSION,"4.0.0");
    Ok(())
}
""")
env = dict(os.environ, CARGO_TARGET_DIR=str(root / "target" / "custom-consumer-build"))
command = ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(consumer / "Cargo.toml")]
metadata = json.loads(subprocess.check_output(command, env=env))
nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
packages = {p["id"]: p for p in metadata["packages"]}
seen = set()
pending = [metadata["resolve"]["root"]]
while pending:
    key = pending.pop()
    if key in seen:
        continue
    seen.add(key)
    for dep in nodes[key]["deps"]:
        if any(k["kind"] is None for k in dep["dep_kinds"]):
            pending.append(dep["pkg"])
names = {packages[p]["name"] for p in seen}
forbidden = {"reqwest", "hyper", "hyper-util", "hyper-rustls", "rustls", "tokio-rustls",
             "tokio-tungstenite", "tungstenite", "native-tls", "openssl", "openssl-sys"}
assert names.isdisjoint(forbidden), sorted(names & forbidden)
zeroize = [packages[p]["version"] for p in seen if packages[p]["name"] == "zeroize"]
assert zeroize == ["1.9.0"], zeroize
sdk = next(p for p in seen if packages[p]["name"] == "acosmi-sdk")
assert "native-http" not in nodes[sdk]["features"]
assert "notifications-ws" not in nodes[sdk]["features"]
subprocess.run(["cargo", "run", "--quiet", "--manifest-path", str(consumer / "Cargo.toml")], env=env, check=True)
result = {"sdk_version": packages[sdk]["version"], "sdk_features": nodes[sdk]["features"],
          "zeroize_versions": zeroize, "normal_package_names": sorted(names),
          "forbidden_packages": sorted(forbidden), "consumer_exit_code": 0}
(consumer / "verified-graph.json").write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result, indent=2))
