//! Une fausse commande `claude` pour les tests : elle parle comme la vraie (une ligne JSON par
//! événement, outils servis par le pont qu'elle lance), sans appeler aucun modèle.
//!
//! Ce qu'on lui dit décide de ce qu'elle fait : « OUTIL <nom> <arguments JSON> » lui fait appeler
//! cet outil et répéter sa réponse ; « INTRUS » et « HORS » lui font sortir de son rôle ; tout
//! autre texte est répété tel quel.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("0.0.0 (faux claude)");
        return;
    }
    let after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    // ce que le maître nous a passé : les tests le relisent
    if let Ok(log) = std::env::var("FAUX_CLAUDE_JOURNAL") {
        let names: Vec<String> = std::env::vars().map(|(name, _)| name).collect();
        let entry = json!({ "args": args, "env": names, "system": after("--system-prompt-file").and_then(|f| std::fs::read_to_string(f).ok()) });
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(log).unwrap();
        writeln!(file, "{entry}").unwrap();
    }
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    let asked: Value = serde_json::from_str(&line).unwrap();
    let text = asked["message"]["content"][0]["text"].as_str().unwrap_or("").to_string();
    let say = |text: &str| {
        println!("{}", json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "id": "m1", "content": [{ "type": "text", "text": text }], "usage": { "input_tokens": 7, "output_tokens": 3 } } }));
    };
    // comme la vraie, elle annonce ce qu'elle a en main ; « INTRUS » : un outil qui n'est pas à
    // l'agent ; « HORS » : elle s'en sert
    let tools = if text.contains("INTRUS") { json!(["Bash"]) } else { json!(["mcp__talixio__read_file"]) };
    println!("{}", json!({ "type": "system", "subtype": "init", "tools": tools, "mcp_servers": [{ "name": "talixio", "status": "connected" }] }));
    if text.contains("HORS") {
        println!("{}", json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "id": "m0", "content": [{ "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "id" } }], "usage": {} } }));
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
    let mode = if after("--resume").is_some() { "suite" } else { "début" };
    let reply = match text.lines().find_map(|l| l.strip_prefix("de l'utilisateur : OUTIL ").or(l.strip_prefix("OUTIL "))) {
        Some(order) => {
            let (name, arguments) = order.split_once(' ').unwrap();
            let config: Value = serde_json::from_str(&std::fs::read_to_string(after("--mcp-config").unwrap()).unwrap()).unwrap();
            let server = &config["mcpServers"]["talixio"];
            let mut bridge = Command::new(server["command"].as_str().unwrap())
                .args(server["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()))
                .envs(server["env"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut to = bridge.stdin.take().unwrap();
            let mut from = BufReader::new(bridge.stdout.take().unwrap());
            let mut call = |id: u64, method: &str, params: Value| -> Value {
                writeln!(to, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).unwrap();
                let mut answer = String::new();
                from.read_line(&mut answer).unwrap();
                serde_json::from_str(&answer).unwrap_or(Value::Null)
            };
            call(1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "faux", "version": "0" } }));
            let tools = call(2, "tools/list", json!({}));
            let known = tools["result"]["tools"].as_array().map(|t| t.len()).unwrap_or(0);
            say("J'appelle l'outil.");
            let result = call(3, "tools/call", json!({ "name": name, "arguments": serde_json::from_str::<Value>(arguments).unwrap() }));
            let _ = bridge.kill();
            let _ = bridge.wait();
            format!("outils={known} erreur={} réponse={}", result["result"]["isError"], result["result"]["content"][0]["text"].as_str().unwrap_or("?"))
        }
        None => format!("écho ({mode}, {}) : {text}", after("--model").unwrap_or_default()),
    };
    println!("{}", json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "id": "m2", "content": [{ "type": "text", "text": reply }], "usage": { "input_tokens": 9, "output_tokens": 4 } } }));
    println!("{}", json!({ "type": "result", "subtype": "success", "is_error": false, "result": reply }));
}
