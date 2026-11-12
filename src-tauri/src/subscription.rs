use crate::config::{NetworkConfig, Subscription};
use base64::{alphabet, engine::{general_purpose::{GeneralPurpose, GeneralPurposeConfig}, DecodePaddingMode}, Engine};
use percent_encoding::percent_decode_str;
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::HashMap, time::Duration};
use url::Url;

pub struct Node { pub name: String, pub outbound: Value }
#[derive(Serialize)]
pub struct NodeSummary { pub index: usize, pub name: String, pub protocol: String }
#[derive(Serialize)]
pub struct ImportResult { pub nodes: Vec<NodeSummary>, pub notices: Vec<String> }

fn decode64(text: &str) -> Result<String, String> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let alphabet = if compact.contains(['-', '_']) { &alphabet::URL_SAFE } else { &alphabet::STANDARD };
    let engine = GeneralPurpose::new(alphabet, GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent));
    let bytes = engine.decode(compact).map_err(|_| "订阅的 Base64 编码无法读取".to_string())?;
    String::from_utf8(bytes).map_err(|_| "订阅内容不是 UTF-8 文本".into())
}
fn decoded(s: &str) -> String { percent_decode_str(s).decode_utf8_lossy().into_owned() }
fn field<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v[k].as_str().ok_or_else(|| format!("节点缺少 {k} 字段"))
}

fn supported_fields(v: &Value, fields: &[&str], description: &str) -> Result<(), String> {
    let object = v.as_object().ok_or_else(|| format!("{description}格式不正确"))?;
    if object.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err(format!("{description}含尚未支持的选项，已跳过该节点"));
    }
    Ok(())
}

pub fn import(input: &str, cfg: &Subscription) -> Result<(Vec<Node>, ImportResult), String> {
    let text = input.trim();
    let body = if text.starts_with("https://") || text.starts_with("http://") {
        let client = reqwest::blocking::Client::builder().no_proxy()
            .timeout(Duration::from_secs(cfg.timeout_seconds)).user_agent(&cfg.user_agent)
            .build().map_err(|_| "无法初始化订阅下载".to_string())?;
        let response = client.get(text).send().map_err(|_| "订阅下载失败，请检查链接和当前网络".to_string())?;
        if !response.status().is_success() { return Err(format!("订阅服务返回 HTTP {}", response.status().as_u16())); }
        response.text().map_err(|_| "订阅内容读取失败".to_string())?
    } else { text.to_owned() };
    let text = body.trim();
    if text.is_empty() { return Err("请输入订阅链接或节点链接".into()); }
    let mut notices = Vec::new();
    let mut nodes = Vec::new();
    if text.starts_with('{') {
        let doc: Value = serde_json::from_str(text).map_err(|_| "sing-box 订阅 JSON 格式不正确".to_string())?;
        let entries = doc["outbounds"].as_array().ok_or("订阅 JSON 缺少 outbounds")?;
        for entry in entries {
            let kind = field(entry, "type")?;
            if ["direct", "block", "dns", "selector", "urltest"].contains(&kind) { continue; }
            if !["vless", "trojan", "shadowsocks", "hysteria2", "vmess", "socks"].contains(&kind) {
                notices.push(format!("已跳过暂未接入的 {kind} 节点")); continue;
            }
            if entry.get("detour").is_some() { notices.push("已跳过需要链式拨号的节点".into()); continue; }
            if entry.get("domain_resolver").is_some() || entry["tls"].get("certificate_path").is_some() || entry["tls"].get("client_key_path").is_some() || entry["tls"].get("client_certificate_path").is_some() {
                notices.push("已跳过依赖外部 DNS 配置或本地证书文件的节点".into()); continue;
            }
            let name = field(entry, "tag")?.to_owned();
            field(entry, "server")?;
            let mut outbound = entry.clone(); outbound["tag"] = json!("proxy");
            nodes.push(Node { name, outbound });
        }
    } else if text.contains("proxies:") {
        let doc: Value = serde_yaml_ng::from_str(text).map_err(|_| "Clash 订阅 YAML 格式不正确".to_string())?;
        for entry in doc["proxies"].as_array().ok_or("Clash 订阅缺少 proxies")? {
            match clash_node(entry) {
                Ok(node) => nodes.push(node),
                Err(reason) => notices.push(reason),
            }
        }
    } else {
        let list = if text.contains("://") { text.to_owned() } else { decode64(text)? };
        for line in list.lines().map(str::trim).filter(|s| !s.is_empty()) {
            match uri_node(line) {
                Ok(node) => nodes.push(node),
                Err(reason) => notices.push(reason),
            }
        }
    }
    if nodes.is_empty() { return Err(format!("没有可用节点。{}", notices.join("；"))); }
    let summaries = nodes.iter().enumerate().map(|(index, n)| NodeSummary {
        index, name: n.name.clone(), protocol: n.outbound["type"].as_str().unwrap().to_owned(),
    }).collect();
    Ok((nodes, ImportResult { nodes: summaries, notices }))
}

fn transport(out: &mut Value, kind: &str, path: &str, host: &str, service: &str) -> Result<(), String> {
    match kind {
        "" | "tcp" | "raw" => (),
        "ws" => {
            let mut t = json!({"type":"ws","path":path});
            if !host.is_empty() { t["headers"] = json!({"Host":host}); }
            out["transport"] = t;
        },
        "grpc" => out["transport"] = json!({"type":"grpc","service_name":service}),
        "httpupgrade" => {
            let mut t = json!({"type":"httpupgrade","path":path});
            if !host.is_empty() { t["host"] = json!(host); }
            out["transport"] = t;
        },
        _ => return Err(format!("暂不支持 {kind} 传输，请换用 TCP、WebSocket、gRPC 或 HTTPUpgrade 节点")),
    }
    Ok(())
}

fn uri_node(line: &str) -> Result<Node, String> {
    if let Some(encoded) = line.strip_prefix("vmess://") {
        let v: Value = serde_json::from_str(&decode64(encoded)?).map_err(|_| "VMess 节点格式不正确".to_string())?;
        supported_fields(&v, &["v","ps","add","port","id","aid","scy","net","type","host","path","tls","sni","alpn","fp","allowInsecure"], "VMess 节点")?;
        if !["", "none"].contains(&v["type"].as_str().unwrap_or("")) { return Err("暂不支持 VMess 的 TCP 头部伪装".into()); }
        let port = match &v["port"] { Value::String(s) => s.parse::<u16>().ok(), Value::Number(n) => n.as_u64().and_then(|p| p.try_into().ok()), _ => None }.ok_or("VMess 端口无效")?;
        let aid = match &v["aid"] { Value::String(s) => s.parse::<u64>().map_err(|_| "VMess alterId 无效")?, Value::Number(n) => n.as_u64().ok_or("VMess alterId 无效")?, Value::Null => 0, _ => return Err("VMess alterId 无效".into()) };
        let mut out = json!({"type":"vmess","tag":"proxy","server":field(&v,"add")?,"server_port":port,"uuid":field(&v,"id")?,"security":v["scy"].as_str().unwrap_or("auto"),"alter_id":aid});
        if v["tls"] == "tls" {
            out["tls"] = json!({"enabled":true,"server_name":v["sni"].as_str().unwrap_or(field(&v,"add")?)});
            if let Some(alpn) = v["alpn"].as_str().filter(|s|!s.is_empty()) { out["tls"]["alpn"] = json!(alpn.split(',').collect::<Vec<_>>()); }
            if let Some(fp) = v["fp"].as_str().filter(|s|!s.is_empty()) { out["tls"]["utls"] = json!({"enabled":true,"fingerprint":fp}); }
            if let Some(insecure) = v.get("allowInsecure") { out["tls"]["insecure"] = json!(insecure == true || insecure == "1" || insecure == "true"); }
        } else if !["", "none"].contains(&v["tls"].as_str().unwrap_or("")) { return Err("VMess TLS 选项尚未支持".into()); }
        transport(&mut out, v["net"].as_str().unwrap_or("tcp"), v["path"].as_str().unwrap_or("/"), v["host"].as_str().unwrap_or(""), v["path"].as_str().unwrap_or(""))?;
        return Ok(Node { name: v["ps"].as_str().unwrap_or("VMess").into(), outbound: out });
    }
    // SIP002 also permits Base64(method:password@host:port).
    let expanded = if let Some(rest) = line.strip_prefix("ss://") {
        let (main, suffix) = rest.split_once('#').unwrap_or((rest, ""));
        if !main.contains('@') { format!("ss://{}#{}", decode64(main)?, suffix) } else { line.to_owned() }
    } else { line.to_owned() };
    let u = Url::parse(&expanded).map_err(|_| "节点链接格式不正确".to_string())?;
    let kind = match u.scheme() { "vless" => "vless", "trojan" => "trojan", "ss" => "shadowsocks", "hy2" | "hysteria2" => "hysteria2", "socks5" => "socks", _ => return Err("已跳过不支持的节点协议".into()) };
    let host = u.host_str().ok_or("节点缺少服务器地址")?.trim_matches(['[', ']']);
    let port = u.port().ok_or("节点链接需要明确的端口")?;
    // Client exports often carry empty options, which mean the same as absent ones.
    let q: HashMap<String,String> = u.query_pairs().into_owned().filter(|(_, value)| !value.is_empty()).collect();
    let common = ["security","sni","alpn","fp","allowInsecure","insecure","type","path","host","serviceName","headerType","mode"];
    let protocol_fields: &[&str] = match kind {
        // spx and pqv only steer the REALITY client side, and sing-box has neither.
        "vless" => &["flow","pbk","sid","encryption","packetEncoding","spx","pqv"],
        "hysteria2" => &["obfs","obfs-password"],
        "trojan" => &[],
        "shadowsocks" | "socks" => &[],
        _ => unreachable!(),
    };
    if let Some(key) = q.keys().find(|key| !protocol_fields.contains(&key.as_str()) && (!common.contains(&key.as_str()) || ["shadowsocks","socks"].contains(&kind))) {
        return Err(format!("节点链接包含尚未支持的选项 {key}，已跳过该节点"));
    }
    let value = |key: &str| q.get(key).map(String::as_str).unwrap_or("");
    if !["", "none"].contains(&value("encryption")) { return Err("此 VLESS 加密选项尚未接入".into()); }
    if !value("type").is_empty() && kind == "hysteria2" { return Err("Hysteria2 节点不使用 V2Ray 传输选项".into()); }
    if !["", "none"].contains(&value("headerType")) { return Err("暂不支持 TCP 头部伪装".into()); }
    if !["", "gun"].contains(&value("mode")) { return Err(format!("暂不支持 {} 传输模式", value("mode"))); }
    let mut out = json!({"type":kind,"tag":"proxy","server":host,"server_port":port});
    match kind {
        "socks" => { out["version"] = json!("5"); if !u.username().is_empty() { out["username"] = json!(decoded(u.username())); out["password"] = json!(decoded(u.password().ok_or("SOCKS 节点缺少密码")?)); } },
        "vless" => {
            out["uuid"] = json!(decoded(u.username()));
            if !value("flow").is_empty() { out["flow"] = json!(value("flow")); }
            match value("packetEncoding") {
                "" => (),
                "none" => out["packet_encoding"] = json!(""),
                "xudp" | "packetaddr" => out["packet_encoding"] = json!(value("packetEncoding")),
                other => return Err(format!("暂不支持 {other} 包编码")),
            }
        },
        "shadowsocks" => {
            let credentials = match u.password() { Some(p) => format!("{}:{}", decoded(u.username()), decoded(p)), None => decode64(&decoded(u.username()))? };
            let (method,password) = credentials.split_once(':').ok_or("Shadowsocks 凭据格式不正确")?;
            out["method"] = json!(method); out["password"] = json!(password);
        },
        _ => {
            let password = match u.password() { Some(p) => format!("{}:{}", decoded(u.username()), decoded(p)), None => decoded(u.username()) };
            out["password"] = json!(password);
        }
    }
    if ["tls", "reality"].contains(&value("security")) || ["trojan", "hysteria2"].contains(&kind) {
        let sni = if value("sni").is_empty() { host } else { value("sni") };
        let mut tls = json!({"enabled":true,"server_name":sni});
        if ["1","true"].contains(&value("insecure")) || ["1","true"].contains(&value("allowInsecure")) { tls["insecure"] = json!(true); }
        if !value("alpn").is_empty() { tls["alpn"] = json!(value("alpn").split(',').collect::<Vec<_>>()); }
        if !value("fp").is_empty() { tls["utls"] = json!({"enabled":true,"fingerprint":value("fp")}); }
        if value("security") == "reality" { tls["reality"] = json!({"enabled":true,"public_key":value("pbk"),"short_id":value("sid")}); }
        out["tls"] = tls;
    } else if !["", "none"].contains(&value("security")) { return Err("节点 TLS 类型尚未接入".into()); }
    if kind == "hysteria2" && !value("obfs").is_empty() { out["obfs"] = json!({"type":value("obfs"),"password":value("obfs-password")}); }
    transport(&mut out, value("type"), if value("path").is_empty() { "/" } else { value("path") }, value("host"), value("serviceName"))?;
    Ok(Node { name: u.fragment().map(decoded).filter(|s| !s.is_empty()).unwrap_or_else(|| kind.to_owned()), outbound: out })
}

fn clash_node(v: &Value) -> Result<Node, String> {
    supported_fields(v, &["name","type","server","port","uuid","password","cipher","flow","alterId","tls","servername","sni","skip-cert-verify","alpn","client-fingerprint","reality-opts","network","ws-opts","grpc-opts","obfs","obfs-password","udp","tfo","packet-encoding"], "Clash 节点")?;
    let source_type = field(v, "type")?;
    let kind = match source_type { "ss" => "shadowsocks", "vless" | "vmess" | "trojan" | "hysteria2" => source_type, _ => return Err(format!("已跳过暂未接入的 {source_type} 节点")) };
    let mut out = json!({"type":kind,"tag":"proxy","server":field(v,"server")?,"server_port":v["port"].as_u64().ok_or("Clash 节点端口无效")?});
    for (source,target) in [("uuid","uuid"),("password","password"),("flow","flow"),("alterId","alter_id")] {
        if let Some(x) = v.get(source) { out[target] = x.clone(); }
    }
    if let Some(cipher) = v.get("cipher") { out[if kind == "shadowsocks" {"method"} else {"security"}] = cipher.clone(); }
    if v["udp"] == false { out["network"] = json!("tcp"); }
    if let Some(tfo) = v.get("tfo") { out["tcp_fast_open"] = tfo.clone(); }
    if let Some(encoding) = v.get("packet-encoding") { out["packet_encoding"] = encoding.clone(); }
    if v["tls"] == true || ["trojan", "hysteria2"].contains(&kind) {
        let mut tls = json!({"enabled":true});
        for (source,target) in [("servername","server_name"),("sni","server_name"),("skip-cert-verify","insecure"),("alpn","alpn")] {
            if let Some(x) = v.get(source) { tls[target] = x.clone(); }
        }
        if let Some(fp) = v.get("client-fingerprint") { tls["utls"] = json!({"enabled":true,"fingerprint":fp}); }
        if let Some(reality) = v.get("reality-opts") {
            supported_fields(reality, &["public-key","short-id"], "Reality 配置")?;
            tls["reality"] = json!({"enabled":true,"public_key":field(reality,"public-key")?,"short_id":field(reality,"short-id")?});
        }
        out["tls"] = tls;
    }
    if let Some(obfs) = v.get("obfs") { out["obfs"] = json!({"type":obfs,"password":field(v,"obfs-password")?}); }
    transport(&mut out, v["network"].as_str().unwrap_or("tcp"), v["ws-opts"]["path"].as_str().unwrap_or("/"), v["ws-opts"]["headers"]["Host"].as_str().unwrap_or(""), v["grpc-opts"]["grpc-service-name"].as_str().unwrap_or(""))?;
    if let Some(ws) = v.get("ws-opts") {
        supported_fields(ws, &["path","headers","max-early-data","early-data-header-name"], "WebSocket 配置")?;
        if v["network"] != "ws" { return Err("WebSocket 参数与节点传输类型不一致".into()); }
        for (source,target) in [("headers","headers"),("max-early-data","max_early_data"),("early-data-header-name","early_data_header_name")] {
            if let Some(value) = ws.get(source) { out["transport"][target] = value.clone(); }
        }
    }
    if let Some(grpc) = v.get("grpc-opts") { supported_fields(grpc, &["grpc-service-name"], "gRPC 配置")?; }
    Ok(Node { name: field(v,"name")?.into(), outbound: out })
}

pub fn sing_box(node: &Node, cfg: &NetworkConfig) -> Value {
    json!({
        "log":{"level":"error"},
        "dns":{"servers":[{"type":"https","tag":"remote-dns","server":cfg.dns_server,"detour":"proxy"},{"type":"udp","tag":"bootstrap-dns","server":cfg.bootstrap_dns}],"final":"remote-dns"},
        "inbounds":[{"type":"tun","tag":"tun","interface_name":cfg.tun_interface,"address":cfg.tun_addresses,"auto_route":true,"auto_redirect":false,"strict_route":true,"stack":cfg.tun_stack},{"type":"mixed","tag":"local","listen":"127.0.0.1","listen_port":cfg.mixed_port}],
        "outbounds":[node.outbound],
        "route":{"rules":[{"port":53,"action":"hijack-dns"},{"action":"resolve","server":"remote-dns"},{"ip_cidr":cfg.guard.blocked_destination_cidrs,"action":"reject"}],"final":"proxy","auto_detect_interface":true,"default_domain_resolver":"bootstrap-dns"}
    })
}
