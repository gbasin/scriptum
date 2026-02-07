// mDNS LAN peer discovery for Scriptum sync optimization.
//
// Discovers `_scriptum-sync._tcp.local` services on the local network and
// resolves them into direct TCP endpoints scoped to a workspace.

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use uuid::Uuid;

const DNS_HEADER_LEN: usize = 12;
const DNS_CLASS_IN: u16 = 1;
const DNS_TYPE_A: u16 = 1;
const DNS_TYPE_PTR: u16 = 12;
const DNS_TYPE_TXT: u16 = 16;
const DNS_TYPE_AAAA: u16 = 28;
const DNS_TYPE_SRV: u16 = 33;
const MDNS_MULTICAST_V4: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;
const MAX_MDNS_PACKET_BYTES: usize = 9_000;
const DISCOVERY_POLL_STEP: Duration = Duration::from_millis(100);

/// DNS-SD service type used by Scriptum peers.
pub const SCRIPTUM_SYNC_SERVICE_TYPE: &str = "_scriptum-sync._tcp.local.";

/// A LAN peer discovered through mDNS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanPeerEndpoint {
    /// Service instance name, e.g. `peer-a._scriptum-sync._tcp.local.`.
    pub instance_name: String,
    /// Optional logical peer id from TXT records (`peer_id=` or `peer=`).
    pub peer_id: Option<String>,
    /// Resolved direct TCP endpoint.
    pub addr: SocketAddr,
}

#[derive(Debug, Clone)]
struct SrvRecord {
    target: String,
    port: u16,
}

#[derive(Debug, Default)]
struct MdnsRecords {
    // owner -> instance
    ptr: Vec<(String, String)>,
    // instance -> service location
    srv: HashMap<String, SrvRecord>,
    // instance -> key/value metadata
    txt: HashMap<String, HashMap<String, String>>,
    // hostname -> ip addresses
    addresses: HashMap<String, Vec<IpAddr>>,
}

/// Discover LAN peers for the given workspace via mDNS.
pub fn discover_lan_peers(
    workspace_id: Uuid,
    timeout: Duration,
) -> Result<Vec<LanPeerEndpoint>, String> {
    let socket = UdpSocket::bind(("0.0.0.0", 0))
        .map_err(|error| format!("failed to bind mDNS socket: {error}"))?;
    socket
        .set_multicast_loop_v4(false)
        .map_err(|error| format!("failed to configure mDNS socket loopback: {error}"))?;
    socket
        .set_read_timeout(Some(DISCOVERY_POLL_STEP))
        .map_err(|error| format!("failed to configure mDNS read timeout: {error}"))?;

    discover_lan_peers_with_socket(&socket, workspace_id, timeout)
}

fn discover_lan_peers_with_socket(
    socket: &UdpSocket,
    workspace_id: Uuid,
    timeout: Duration,
) -> Result<Vec<LanPeerEndpoint>, String> {
    if timeout.is_zero() {
        return Ok(Vec::new());
    }

    let query = build_ptr_query(SCRIPTUM_SYNC_SERVICE_TYPE)?;
    if let Err(error) =
        socket.send_to(&query, SocketAddr::new(IpAddr::V4(MDNS_MULTICAST_V4), MDNS_PORT))
    {
        // Discovery is a latency optimization; in constrained environments
        // (CI sandboxes, loopback-only sockets), multicast may be unavailable.
        if is_non_fatal_mdns_send_error(&error) {
            return Ok(Vec::new());
        }
        return Err(format!("failed to send mDNS discovery query: {error}"));
    }

    let deadline = Instant::now() + timeout;
    let mut records = MdnsRecords::default();
    let mut packet_buf = [0u8; MAX_MDNS_PACKET_BYTES];
    while Instant::now() < deadline {
        match socket.recv_from(&mut packet_buf) {
            Ok((size, _)) => {
                let _ = merge_records_from_packet(&mut records, &packet_buf[..size]);
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(error) => {
                return Err(format!("failed to receive mDNS response: {error}"));
            }
        }
    }

    Ok(resolve_records(&records, workspace_id))
}

fn is_non_fatal_mdns_send_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::AddrNotAvailable
            | std::io::ErrorKind::NetworkUnreachable
            | std::io::ErrorKind::PermissionDenied
    )
}

fn build_ptr_query(service_type: &str) -> Result<Vec<u8>, String> {
    let mut query = Vec::new();
    // id, flags, qdcount, ancount, nscount, arcount
    query.extend_from_slice(&[0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    encode_dns_name(service_type, &mut query)?;
    query.extend_from_slice(&DNS_TYPE_PTR.to_be_bytes());
    query.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
    Ok(query)
}

fn merge_records_from_packet(records: &mut MdnsRecords, packet: &[u8]) -> Result<(), String> {
    if packet.len() < DNS_HEADER_LEN {
        return Err("mDNS packet is smaller than DNS header".to_string());
    }

    let question_count = read_u16(packet, 4)? as usize;
    let answer_count = read_u16(packet, 6)? as usize;
    let authority_count = read_u16(packet, 8)? as usize;
    let additional_count = read_u16(packet, 10)? as usize;

    let mut offset = DNS_HEADER_LEN;

    for _ in 0..question_count {
        let (_name, next) = decode_dns_name(packet, offset)?;
        offset = next;
        if offset + 4 > packet.len() {
            return Err("mDNS question section is truncated".to_string());
        }
        offset += 4;
    }

    let total_rr = answer_count + authority_count + additional_count;
    for _ in 0..total_rr {
        let (owner_name, next) = decode_dns_name(packet, offset)?;
        offset = next;
        if offset + 10 > packet.len() {
            return Err("mDNS resource record header is truncated".to_string());
        }

        let rr_type = read_u16(packet, offset)?;
        // Class is currently unused (flush bit can be set in mDNS).
        let _rr_class = read_u16(packet, offset + 2)?;
        let _ttl = read_u32(packet, offset + 4)?;
        let rdlen = read_u16(packet, offset + 8)? as usize;
        offset += 10;

        if offset + rdlen > packet.len() {
            return Err("mDNS resource record payload is truncated".to_string());
        }
        let rdata_start = offset;
        let rdata_end = offset + rdlen;
        let owner_name = normalize_dns_name(&owner_name);

        match rr_type {
            DNS_TYPE_PTR => {
                let (target_name, _) = decode_dns_name(packet, rdata_start)?;
                records.ptr.push((owner_name, normalize_dns_name(&target_name)));
            }
            DNS_TYPE_SRV => {
                if rdlen >= 6 {
                    let port = read_u16(packet, rdata_start + 4)?;
                    let (target_name, _) = decode_dns_name(packet, rdata_start + 6)?;
                    records.srv.insert(
                        owner_name,
                        SrvRecord { target: normalize_dns_name(&target_name), port },
                    );
                }
            }
            DNS_TYPE_TXT => {
                let txt_map = parse_txt_kv_pairs(&packet[rdata_start..rdata_end]);
                if !txt_map.is_empty() {
                    records.txt.insert(owner_name, txt_map);
                }
            }
            DNS_TYPE_A => {
                if rdlen == 4 {
                    let address = IpAddr::V4(Ipv4Addr::new(
                        packet[rdata_start],
                        packet[rdata_start + 1],
                        packet[rdata_start + 2],
                        packet[rdata_start + 3],
                    ));
                    records.addresses.entry(owner_name).or_default().push(address);
                }
            }
            DNS_TYPE_AAAA => {
                if rdlen == 16 {
                    let mut octets = [0u8; 16];
                    octets.copy_from_slice(&packet[rdata_start..rdata_end]);
                    records.addresses.entry(owner_name).or_default().push(IpAddr::from(octets));
                }
            }
            _ => {}
        }

        offset = rdata_end;
    }

    Ok(())
}

fn resolve_records(records: &MdnsRecords, workspace_id: Uuid) -> Vec<LanPeerEndpoint> {
    let service_type = normalize_dns_name(SCRIPTUM_SYNC_SERVICE_TYPE);
    let workspace_key = workspace_id.to_string();
    let mut peers = BTreeMap::<(String, SocketAddr), LanPeerEndpoint>::new();

    for (owner, instance) in &records.ptr {
        if owner != &service_type {
            continue;
        }

        let txt = records.txt.get(instance);
        if let Some(scope) = txt
            .and_then(|map| map.get("workspace_id").or_else(|| map.get("workspace")))
            .map(|value| value.trim())
        {
            if scope != workspace_key {
                continue;
            }
        }

        let Some(srv) = records.srv.get(instance) else {
            continue;
        };

        // Primary mapping is SRV target hostname -> A/AAAA.
        // Fallback to instance owner (some implementations place address there).
        let mut addresses = records.addresses.get(&srv.target).cloned().unwrap_or_default();
        if addresses.is_empty() {
            addresses.extend(records.addresses.get(instance).cloned().unwrap_or_default());
        }
        if addresses.is_empty() {
            continue;
        }

        let peer_id = txt.and_then(|map| map.get("peer_id").or_else(|| map.get("peer"))).cloned();
        for address in addresses {
            let addr = SocketAddr::new(address, srv.port);
            peers.entry((instance.clone(), addr)).or_insert_with(|| LanPeerEndpoint {
                instance_name: instance.clone(),
                peer_id: peer_id.clone(),
                addr,
            });
        }
    }

    peers.into_values().collect()
}

fn encode_dns_name(name: &str, output: &mut Vec<u8>) -> Result<(), String> {
    let trimmed = name.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return Err("DNS name must not be empty".to_string());
    }

    for label in trimmed.split('.') {
        if label.is_empty() {
            return Err(format!("DNS name `{name}` contains an empty label"));
        }
        if label.len() > 63 {
            return Err(format!("DNS label `{label}` exceeds 63 bytes"));
        }
        output.push(label.len() as u8);
        output.extend_from_slice(label.as_bytes());
    }
    output.push(0);
    Ok(())
}

fn decode_dns_name(packet: &[u8], offset: usize) -> Result<(String, usize), String> {
    if offset >= packet.len() {
        return Err("DNS name offset points past end of packet".to_string());
    }

    let mut cursor = offset;
    let mut next_offset = offset;
    let mut jumped = false;
    let mut jump_budget = packet.len();
    let mut labels = Vec::new();

    loop {
        if cursor >= packet.len() {
            return Err("DNS name parsing ran past end of packet".to_string());
        }
        if jump_budget == 0 {
            return Err("DNS name compression pointer loop detected".to_string());
        }
        jump_budget -= 1;

        let len = packet[cursor];
        if len == 0 {
            if !jumped {
                next_offset = cursor + 1;
            }
            break;
        }

        if (len & 0xC0) == 0xC0 {
            if cursor + 1 >= packet.len() {
                return Err("truncated DNS compression pointer".to_string());
            }
            let pointer = (((len & 0x3F) as usize) << 8) | packet[cursor + 1] as usize;
            if pointer >= packet.len() {
                return Err("DNS compression pointer points past packet".to_string());
            }
            if !jumped {
                next_offset = cursor + 2;
                jumped = true;
            }
            cursor = pointer;
            continue;
        }

        if (len & 0xC0) != 0 {
            return Err("unsupported DNS label encoding".to_string());
        }

        cursor += 1;
        let label_len = len as usize;
        if cursor + label_len > packet.len() {
            return Err("truncated DNS label".to_string());
        }
        let label = std::str::from_utf8(&packet[cursor..cursor + label_len])
            .map_err(|error| format!("invalid UTF-8 in DNS label: {error}"))?;
        labels.push(label.to_ascii_lowercase());
        cursor += label_len;
        if !jumped {
            next_offset = cursor;
        }
    }

    let name = if labels.is_empty() { ".".to_string() } else { format!("{}.", labels.join(".")) };
    Ok((name, next_offset))
}

fn parse_txt_kv_pairs(payload: &[u8]) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let mut cursor = 0usize;
    while cursor < payload.len() {
        let len = payload[cursor] as usize;
        cursor += 1;
        if cursor + len > payload.len() {
            break;
        }
        if len == 0 {
            continue;
        }
        let item = &payload[cursor..cursor + len];
        if let Ok(text) = std::str::from_utf8(item) {
            if let Some((key, value)) = text.split_once('=') {
                values.insert(key.to_ascii_lowercase(), value.to_string());
            } else {
                values.insert(text.to_ascii_lowercase(), String::new());
            }
        }
        cursor += len;
    }
    values
}

fn normalize_dns_name(name: &str) -> String {
    let trimmed = name.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        ".".to_string()
    } else {
        format!("{}.", trimmed.to_ascii_lowercase())
    }
}

fn read_u16(packet: &[u8], offset: usize) -> Result<u16, String> {
    if offset + 2 > packet.len() {
        return Err("failed to read u16: out of bounds".to_string());
    }
    Ok(u16::from_be_bytes([packet[offset], packet[offset + 1]]))
}

fn read_u32(packet: &[u8], offset: usize) -> Result<u32, String> {
    if offset + 4 > packet.len() {
        return Err("failed to read u32: out of bounds".to_string());
    }
    Ok(u32::from_be_bytes([
        packet[offset],
        packet[offset + 1],
        packet[offset + 2],
        packet[offset + 3],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv6Addr, UdpSocket};

    #[test]
    fn build_ptr_query_targets_scriptum_service() {
        let query = build_ptr_query(SCRIPTUM_SYNC_SERVICE_TYPE).expect("query should build");
        assert!(query.len() > DNS_HEADER_LEN);
        assert_eq!(u16::from_be_bytes([query[4], query[5]]), 1, "one question expected");
    }

    #[test]
    fn merge_and_resolve_workspace_scoped_records() {
        let workspace_id = Uuid::new_v4();
        let service_instance = "peer-a._scriptum-sync._tcp.local.";
        let host = "peer-a.local.";
        let packet = build_response_packet(
            &[
                ptr_record(SCRIPTUM_SYNC_SERVICE_TYPE, service_instance),
                srv_record(service_instance, 39092, host),
                txt_record(
                    service_instance,
                    &[format!("workspace_id={workspace_id}"), "peer_id=peer-a".to_string()],
                ),
                a_record(host, Ipv4Addr::new(192, 168, 1, 20)),
            ],
            0,
        );

        let mut records = MdnsRecords::default();
        merge_records_from_packet(&mut records, &packet).expect("packet should parse");

        let peers = resolve_records(&records, workspace_id);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].instance_name, normalize_dns_name(service_instance));
        assert_eq!(peers[0].peer_id.as_deref(), Some("peer-a"));
        assert_eq!(
            peers[0].addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)), 39092)
        );
    }

    #[test]
    fn resolve_filters_different_workspace_ids() {
        let workspace_id = Uuid::new_v4();
        let other_workspace = Uuid::new_v4();
        let service_instance = "peer-b._scriptum-sync._tcp.local.";
        let host = "peer-b.local.";
        let packet = build_response_packet(
            &[
                ptr_record(SCRIPTUM_SYNC_SERVICE_TYPE, service_instance),
                srv_record(service_instance, 39093, host),
                txt_record(service_instance, &[format!("workspace_id={other_workspace}")]),
                a_record(host, Ipv4Addr::new(192, 168, 1, 21)),
            ],
            0,
        );

        let mut records = MdnsRecords::default();
        merge_records_from_packet(&mut records, &packet).expect("packet should parse");
        let peers = resolve_records(&records, workspace_id);
        assert!(peers.is_empty(), "different workspace should be ignored");
    }

    #[test]
    fn resolve_supports_aaaa_records() {
        let workspace_id = Uuid::new_v4();
        let service_instance = "peer-v6._scriptum-sync._tcp.local.";
        let host = "peer-v6.local.";
        let packet = build_response_packet(
            &[
                ptr_record(SCRIPTUM_SYNC_SERVICE_TYPE, service_instance),
                srv_record(service_instance, 39094, host),
                txt_record(service_instance, &[format!("workspace={workspace_id}")]),
                aaaa_record(host, Ipv6Addr::LOCALHOST),
            ],
            0,
        );

        let mut records = MdnsRecords::default();
        merge_records_from_packet(&mut records, &packet).expect("packet should parse");
        let peers = resolve_records(&records, workspace_id);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].addr, SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 39094));
    }

    #[test]
    fn decode_dns_name_handles_compressed_owner_name() {
        let workspace_id = Uuid::new_v4();
        let service_instance = "peer-c._scriptum-sync._tcp.local.";
        let host = "peer-c.local.";

        // Build one question so answer owner can point to it via compression pointer.
        let mut packet = Vec::new();
        packet.extend_from_slice(&[
            0, 0, // id
            0, 0, // flags
            0, 1, // questions
            0, 4, // answers
            0, 0, // authority
            0, 0, // additional
        ]);
        let question_offset = packet.len();
        encode_dns_name(SCRIPTUM_SYNC_SERVICE_TYPE, &mut packet)
            .expect("question name should encode");
        packet.extend_from_slice(&DNS_TYPE_PTR.to_be_bytes());
        packet.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());

        // PTR with compressed owner name pointer.
        packet.extend_from_slice(&[0xC0, question_offset as u8]);
        packet.extend_from_slice(&DNS_TYPE_PTR.to_be_bytes());
        packet.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
        packet.extend_from_slice(&120u32.to_be_bytes());
        let mut ptr_rdata = Vec::new();
        encode_dns_name(service_instance, &mut ptr_rdata).expect("ptr target should encode");
        packet.extend_from_slice(&(ptr_rdata.len() as u16).to_be_bytes());
        packet.extend_from_slice(&ptr_rdata);

        // SRV
        push_record(&mut packet, service_instance, DNS_TYPE_SRV, {
            let mut data = Vec::new();
            data.extend_from_slice(&0u16.to_be_bytes());
            data.extend_from_slice(&0u16.to_be_bytes());
            data.extend_from_slice(&39095u16.to_be_bytes());
            encode_dns_name(host, &mut data).expect("srv host should encode");
            data
        });
        // TXT
        push_record(
            &mut packet,
            service_instance,
            DNS_TYPE_TXT,
            encode_txt_entries(&[format!("workspace_id={workspace_id}")]),
        );
        // A
        push_record(&mut packet, host, DNS_TYPE_A, vec![192, 168, 1, 25]);

        let mut records = MdnsRecords::default();
        merge_records_from_packet(&mut records, &packet).expect("packet should parse");
        let peers = resolve_records(&records, workspace_id);
        assert_eq!(peers.len(), 1);
        assert_eq!(
            peers[0].addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 25)), 39095)
        );
    }

    #[test]
    fn discover_returns_empty_when_timeout_elapsed_immediately() {
        let socket =
            UdpSocket::bind(("127.0.0.1", 0)).expect("loopback socket should bind for test");
        socket
            .set_read_timeout(Some(Duration::from_millis(1)))
            .expect("test read timeout should set");
        let peers = discover_lan_peers_with_socket(&socket, Uuid::new_v4(), Duration::ZERO)
            .expect("zero-timeout discovery should succeed");
        assert!(peers.is_empty());
    }

    #[derive(Debug, Clone)]
    struct ResourceRecord {
        owner: String,
        rr_type: u16,
        rdata: Vec<u8>,
    }

    fn ptr_record(owner: &str, target: &str) -> ResourceRecord {
        let mut rdata = Vec::new();
        encode_dns_name(target, &mut rdata).expect("ptr target should encode");
        ResourceRecord { owner: owner.to_string(), rr_type: DNS_TYPE_PTR, rdata }
    }

    fn srv_record(owner: &str, port: u16, target: &str) -> ResourceRecord {
        let mut rdata = Vec::new();
        rdata.extend_from_slice(&0u16.to_be_bytes()); // priority
        rdata.extend_from_slice(&0u16.to_be_bytes()); // weight
        rdata.extend_from_slice(&port.to_be_bytes());
        encode_dns_name(target, &mut rdata).expect("srv target should encode");
        ResourceRecord { owner: owner.to_string(), rr_type: DNS_TYPE_SRV, rdata }
    }

    fn txt_record(owner: &str, entries: &[String]) -> ResourceRecord {
        ResourceRecord {
            owner: owner.to_string(),
            rr_type: DNS_TYPE_TXT,
            rdata: encode_txt_entries(entries),
        }
    }

    fn a_record(owner: &str, address: Ipv4Addr) -> ResourceRecord {
        ResourceRecord {
            owner: owner.to_string(),
            rr_type: DNS_TYPE_A,
            rdata: address.octets().to_vec(),
        }
    }

    fn aaaa_record(owner: &str, address: Ipv6Addr) -> ResourceRecord {
        ResourceRecord {
            owner: owner.to_string(),
            rr_type: DNS_TYPE_AAAA,
            rdata: address.octets().to_vec(),
        }
    }

    fn encode_txt_entries(entries: &[String]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for entry in entries {
            bytes.push(entry.len() as u8);
            bytes.extend_from_slice(entry.as_bytes());
        }
        bytes
    }

    fn build_response_packet(records: &[ResourceRecord], question_count: u16) -> Vec<u8> {
        let mut packet = Vec::new();
        packet.extend_from_slice(&[
            0,
            0, // id
            0,
            0, // flags
            (question_count >> 8) as u8,
            (question_count & 0xFF) as u8,
            ((records.len() as u16) >> 8) as u8,
            ((records.len() as u16) & 0xFF) as u8,
            0,
            0, // authority
            0,
            0, // additional
        ]);
        for record in records {
            push_record(&mut packet, &record.owner, record.rr_type, record.rdata.clone());
        }
        packet
    }

    fn push_record(packet: &mut Vec<u8>, owner: &str, rr_type: u16, rdata: Vec<u8>) {
        encode_dns_name(owner, packet).expect("record owner should encode");
        packet.extend_from_slice(&rr_type.to_be_bytes());
        packet.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
        packet.extend_from_slice(&120u32.to_be_bytes());
        packet.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        packet.extend_from_slice(&rdata);
    }
}
