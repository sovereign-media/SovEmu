//! UPnP's Internet Gateway Device (IGD v1 and v2): what most home routers speak, when "UPnP" is
//! on in their settings.
//!
//! 1. **Search** (SSDP): a multicast `M-SEARCH` to 239.255.255.250:1900; the router answers with
//!    the address of its description.
//! 2. **Describe**: that XML file lists the router's services. The one that forwards ports is
//!    `WANIPConnection` (or `WANPPPConnection` on a router that dials PPPoE itself), at its
//!    control URL.
//! 3. **Control** (SOAP over HTTP): `GetExternalIPAddress`, `AddPortMapping` and
//!    `DeletePortMapping`.
//!
//! Every URL must be at the address that answered the search, so a device on the network can't
//! send the server to fetch from anywhere else.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use reqwest::Url;
use tokio::net::UdpSocket;

use crate::{Error, Granted, Protocol, Transport};

/// SSDP's multicast address and port.
pub(crate) const SSDP: SocketAddr =
    SocketAddr::new(IpAddr::V4(std::net::Ipv4Addr::new(239, 255, 255, 250)), 1900);

/// What the search asks for: a gateway (IGD v1, which v2 routers also answer to, and v2), or
/// the forwarding service itself, which some routers answer to instead.
const SEARCH_TARGETS: &[&str] = &[
    "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
    "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
];

/// The services that forward ports.
const SERVICES: &[&str] = &[
    "urn:schemas-upnp-org:service:WANIPConnection:",
    "urn:schemas-upnp-org:service:WANPPPConnection:",
];

/// How long one HTTP request to the router may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);
/// The largest description or SOAP answer read.
const MAX_BODY: usize = 256 * 1024;

/// UPnP's error codes that matter here.
const CONFLICT_IN_MAPPING_ENTRY: u16 = 718;
const SAME_PORT_VALUES_REQUIRED: u16 = 724;
const ONLY_PERMANENT_LEASES_SUPPORTED: u16 = 725;

/// The `M-SEARCH` for `target`.
fn search_request(to: SocketAddr, target: &str) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {to}\r\nST: {target}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\n\r\n"
    )
}

/// A search answer's `LOCATION` (the description's URL).
fn parse_search_answer(text: &str) -> Option<&str> {
    let mut lines = text.split("\r\n");
    if !lines.next()?.to_ascii_uppercase().starts_with("HTTP/1.1 200") {
        return None;
    }
    lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case("location").then(|| value.trim())
    })
}

/// `url` parsed, if it is plain HTTP at `responder` (the address that answered the search).
fn at_responder(url: &str, base: Option<&Url>, responder: IpAddr) -> Option<Url> {
    let url = match base {
        Some(base) => base.join(url).ok()?,
        None => Url::parse(url).ok()?,
    };
    let host: IpAddr = match url.host()? {
        url::Host::Ipv4(ip) => ip.into(),
        url::Host::Ipv6(ip) => ip.into(),
        url::Host::Domain(_) => return None,
    };
    (url.scheme() == "http" && host == responder).then_some(url)
}

/// The forwarding services a description lists: each one's type and control URL.
fn parse_description(xml: &str, location: &Url, responder: IpAddr) -> Vec<(String, Url)> {
    let Ok(doc) = roxmltree::Document::parse(xml) else { return Vec::new() };
    let text = |node: roxmltree::Node<'_, '_>, name: &str| {
        node.children()
            .find(|c| c.tag_name().name() == name)
            .and_then(|c| c.text())
            .map(str::trim)
            .map(str::to_owned)
    };
    // IGD v1 may give a base for relative URLs; without one they're relative to the description.
    let base = doc
        .descendants()
        .find(|n| n.tag_name().name() == "URLBase")
        .and_then(|n| n.text())
        .and_then(|b| at_responder(b.trim(), None, responder))
        .unwrap_or_else(|| location.clone());
    doc.descendants()
        .filter(|n| n.tag_name().name() == "service")
        .filter_map(|service| {
            let kind = text(service, "serviceType")?;
            if !SERVICES.iter().any(|s| kind.starts_with(s)) {
                return None;
            }
            let control = at_responder(&text(service, "controlURL")?, Some(&base), responder)?;
            Some((kind, control))
        })
        .collect()
}

/// Text for XML: what the requests carry is numbers and SovEmu's own words, but escaped all
/// the same.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The SOAP envelope calling `action` on `service`, with `args` in order (some routers want
/// them in the order the specification lists them).
fn soap_body(service: &str, action: &str, args: &[(&str, String)]) -> String {
    let args: String =
        args.iter().map(|(name, value)| format!("<{name}>{}</{name}>", escape(value))).collect();
    format!(
        "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
         <u:{action} xmlns:u=\"{service}\">{args}</u:{action}></s:Body></s:Envelope>"
    )
}

/// The text of the first element named `name` in a SOAP answer.
fn soap_value(xml: &str, name: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    doc.descendants()
        .find(|n| n.tag_name().name() == name)
        .map(|n| n.text().unwrap_or_default().trim().to_owned())
}

/// A SOAP fault's UPnP error code and description.
fn soap_fault(xml: &str) -> Option<(u16, String)> {
    let code = soap_value(xml, "errorCode")?.parse().ok()?;
    Some((code, soap_value(xml, "errorDescription").unwrap_or_default()))
}

fn protocol_name(transport: Transport) -> &'static str {
    match transport {
        Transport::Udp => "UDP",
        Transport::Tcp => "TCP",
    }
}

/// A router that answers UPnP IGD.
#[derive(Debug)]
pub(crate) struct Upnp {
    client: reqwest::Client,
    router: IpAddr,
    local: IpAddr,
    service: String,
    control: Url,
}

impl Upnp {
    /// Searches at `ssdp` (the multicast group, or a test's own address) for `wait`, and returns
    /// the first router whose forwarding service answers.
    pub(crate) async fn find(ssdp: SocketAddr, wait: Duration) -> Result<Self, Error> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        let answers = search(ssdp, wait).await?;
        let mut last = Error::NotFound;
        for (responder, location) in answers {
            match Self::describe(&client, responder, &location).await {
                Ok(found) => return Ok(found),
                Err(e) => {
                    tracing::debug!(location = %location, error = %e, "not a usable UPnP gateway");
                    last = e;
                }
            }
        }
        Err(last)
    }

    /// The router at `location`, if it has a forwarding service that tells its address.
    async fn describe(
        client: &reqwest::Client,
        responder: IpAddr,
        location: &Url,
    ) -> Result<Self, Error> {
        let xml = fetch(client.get(location.clone())).await?.1;
        let services = parse_description(&xml, location, responder);
        if services.is_empty() {
            return Err(Error::BadResponse("no port forwarding service"));
        }
        let local = crate::local_ip_towards(responder)?;
        let mut last = Error::NotFound;
        // A router may list a PPP and an IP connection, of which only one is up: the one that
        // knows its external address is it.
        for (service, control) in services {
            let gateway =
                Self { client: client.clone(), router: responder, local, service, control };
            match crate::Gateway::external_ip(&gateway).await {
                Ok(ip) if !ip.is_unspecified() => return Ok(gateway),
                Ok(_) => last = Error::BadResponse("the router has no internet address"),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Calls `action`, returning the answer's XML, or the router's error.
    async fn call(&self, action: &str, args: &[(&str, String)]) -> Result<String, Error> {
        let request = self
            .client
            .post(self.control.clone())
            .header("Content-Type", "text/xml; charset=\"utf-8\"")
            .header("SOAPAction", format!("\"{}#{action}\"", self.service))
            .body(soap_body(&self.service, action, args));
        let (status, body) = fetch(request).await?;
        if status.is_success() {
            return Ok(body);
        }
        Err(match soap_fault(&body) {
            Some((code, reason)) => Error::Refused { code, reason },
            None => Error::Http(format!("{action}: HTTP {status}")),
        })
    }

    async fn add(
        &self,
        transport: Transport,
        internal: u16,
        external: u16,
        lease: u32,
    ) -> Result<(), Error> {
        self.call(
            "AddPortMapping",
            &[
                ("NewRemoteHost", String::new()),
                ("NewExternalPort", external.to_string()),
                ("NewProtocol", protocol_name(transport).to_owned()),
                ("NewInternalPort", internal.to_string()),
                ("NewInternalClient", self.local.to_string()),
                ("NewEnabled", "1".to_owned()),
                ("NewPortMappingDescription", format!("SovEmu {internal}/{}", transport.name())),
                ("NewLeaseDuration", lease.to_string()),
            ],
        )
        .await
        .map(drop)
    }
}

impl crate::Gateway for Upnp {
    fn protocol(&self) -> Protocol {
        Protocol::Upnp
    }

    fn router(&self) -> IpAddr {
        self.router
    }

    fn local_ip(&self) -> IpAddr {
        self.local
    }

    async fn external_ip(&self) -> Result<IpAddr, Error> {
        let xml = self.call("GetExternalIPAddress", &[]).await?;
        soap_value(&xml, "NewExternalIPAddress")
            .and_then(|ip| ip.parse().ok())
            .ok_or(Error::BadResponse("no external address in the answer"))
    }

    async fn map(
        &self,
        transport: Transport,
        internal: u16,
        external: u16,
        lifetime: u32,
    ) -> Result<Granted, Error> {
        match self.add(transport, internal, external, lifetime).await {
            Ok(()) => Ok(Granted { external, lifetime }),
            // Older routers keep mappings until they are deleted, or they restart.
            Err(Error::Refused { code: ONLY_PERMANENT_LEASES_SUPPORTED, .. }) if lifetime != 0 => {
                self.add(transport, internal, external, 0).await?;
                Ok(Granted { external, lifetime: 0 })
            }
            Err(Error::Refused {
                code: CONFLICT_IN_MAPPING_ENTRY | SAME_PORT_VALUES_REQUIRED,
                ..
            }) => Err(Error::Conflict),
            Err(e) => Err(e),
        }
    }

    async fn unmap(
        &self,
        transport: Transport,
        _internal: u16,
        external: u16,
    ) -> Result<(), Error> {
        self.call(
            "DeletePortMapping",
            &[
                ("NewRemoteHost", String::new()),
                ("NewExternalPort", external.to_string()),
                ("NewProtocol", protocol_name(transport).to_owned()),
            ],
        )
        .await
        .map(drop)
    }
}

/// Sends the search and gathers answers for `wait`: who answered, and their description's URL.
async fn search(ssdp: SocketAddr, wait: Duration) -> Result<Vec<(IpAddr, Url)>, Error> {
    let socket =
        UdpSocket::bind(SocketAddr::new(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 0)).await?;
    for target in SEARCH_TARGETS {
        socket.send_to(search_request(ssdp, target).as_bytes(), ssdp).await?;
    }
    let deadline = tokio::time::Instant::now() + wait;
    let mut found: Vec<(IpAddr, Url)> = Vec::new();
    let mut buf = vec![0u8; 2048];
    while let Ok(received) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await {
        let Ok((n, from)) = received else { continue };
        let Some(location) = std::str::from_utf8(&buf[..n]).ok().and_then(parse_search_answer)
        else {
            continue;
        };
        let Some(url) = at_responder(location, None, from.ip()) else {
            tracing::debug!(from = %from, location, "UPnP answer pointing elsewhere: ignored");
            continue;
        };
        // One router answers each target: try each description once.
        if !found.iter().any(|(_, u)| *u == url) {
            found.push((from.ip(), url));
        }
    }
    if found.is_empty() { Err(Error::NotFound) } else { Ok(found) }
}

/// Sends `request`, returning the status and at most [`MAX_BODY`] of the body.
async fn fetch(request: reqwest::RequestBuilder) -> Result<(reqwest::StatusCode, String), Error> {
    let mut response =
        request.send().await.map_err(|e| Error::Http(e.without_url().to_string()))?;
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| Error::Http(e.to_string()))? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BODY {
            return Err(Error::BadResponse("the router's answer is too large"));
        }
    }
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Gateway;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const DESCRIPTION: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:InternetGatewayDevice:1</deviceType>
    <deviceList><device>
      <deviceType>urn:schemas-upnp-org:device:WANDevice:1</deviceType>
      <serviceList><service>
        <serviceType>urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1</serviceType>
        <controlURL>/ctl/CmnIfCfg</controlURL>
      </service></serviceList>
      <deviceList><device>
        <deviceType>urn:schemas-upnp-org:device:WANConnectionDevice:1</deviceType>
        <serviceList><service>
          <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
          <controlURL>/ctl/IPConn</controlURL>
        </service></serviceList>
      </device></deviceList>
    </device></deviceList>
  </device>
</root>"#;

    #[test]
    fn the_search_asks_for_a_gateway() {
        let to: SocketAddr = "239.255.255.250:1900".parse().unwrap();
        let m = search_request(to, SEARCH_TARGETS[0]);
        assert!(m.starts_with("M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n"), "{m}");
        assert!(m.contains("\r\nMAN: \"ssdp:discover\"\r\n") && m.ends_with("\r\n\r\n"), "{m}");
    }

    #[test]
    fn a_search_answer_gives_its_location() {
        let answer = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\nST: urn:x\r\n\
                      Location: http://192.168.1.1:5000/rootDesc.xml\r\n\r\n";
        assert_eq!(parse_search_answer(answer), Some("http://192.168.1.1:5000/rootDesc.xml"));
        assert_eq!(parse_search_answer("NOTIFY * HTTP/1.1\r\nLOCATION: http://x/\r\n\r\n"), None);
    }

    #[test]
    fn urls_must_be_at_whoever_answered() {
        let router: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(at_responder("http://192.168.1.1:5000/d.xml", None, router).is_some());
        assert!(at_responder("http://192.168.1.9:5000/d.xml", None, router).is_none());
        assert!(at_responder("http://router.lan/d.xml", None, router).is_none(), "no names");
        assert!(at_responder("https://192.168.1.1/d.xml", None, router).is_none());
        let base = Url::parse("http://192.168.1.1:5000/rootDesc.xml").unwrap();
        assert_eq!(
            at_responder("/ctl/IPConn", Some(&base), router).unwrap().as_str(),
            "http://192.168.1.1:5000/ctl/IPConn"
        );
        assert!(at_responder("http://10.0.0.1/ctl", Some(&base), router).is_none());
    }

    #[test]
    fn the_description_gives_the_forwarding_service() {
        let router: IpAddr = "192.168.1.1".parse().unwrap();
        let location = Url::parse("http://192.168.1.1:5000/rootDesc.xml").unwrap();
        let services = parse_description(DESCRIPTION, &location, router);
        assert_eq!(services.len(), 1, "{services:?}");
        assert_eq!(services[0].0, "urn:schemas-upnp-org:service:WANIPConnection:1");
        assert_eq!(services[0].1.as_str(), "http://192.168.1.1:5000/ctl/IPConn");
        // A URLBase is what relative URLs are relative to.
        let based = DESCRIPTION.replace(
            "<device>\n    <deviceType>urn:schemas-upnp-org:device:InternetGatewayDevice:1",
            "<URLBase>http://192.168.1.1:49000</URLBase><device>\n    <deviceType>urn:schemas-upnp-org:device:InternetGatewayDevice:1",
        );
        let services = parse_description(&based, &location, router);
        assert_eq!(services[0].1.as_str(), "http://192.168.1.1:49000/ctl/IPConn");
        assert!(parse_description("<not xml", &location, router).is_empty());
    }

    #[test]
    fn soap_requests_and_answers() {
        let body = soap_body(
            "urn:schemas-upnp-org:service:WANIPConnection:1",
            "DeletePortMapping",
            &[("NewRemoteHost", String::new()), ("NewExternalPort", "50000".into())],
        );
        assert!(body.contains(
            "<u:DeletePortMapping xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">\
             <NewRemoteHost></NewRemoteHost><NewExternalPort>50000</NewExternalPort></u:DeletePortMapping>"
        ));
        assert!(roxmltree::Document::parse(&body).is_ok());
        let answer = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:GetExternalIPAddressResponse xmlns:u="urn:schemas-upnp-org:service:WANIPConnection:1"><NewExternalIPAddress>203.0.113.7</NewExternalIPAddress></u:GetExternalIPAddressResponse></s:Body></s:Envelope>"#;
        assert_eq!(soap_value(answer, "NewExternalIPAddress").as_deref(), Some("203.0.113.7"));
        assert_eq!(
            soap_fault(&fault(718, "ConflictInMappingEntry")),
            Some((718, "ConflictInMappingEntry".into()))
        );
        assert_eq!(soap_fault(answer), None);
    }

    pub(crate) fn fault(code: u16, description: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>{code}</errorCode><errorDescription>{description}</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"#
        )
    }

    /// What the fake router was asked: each SOAP action and its body.
    type Calls = Arc<Mutex<Vec<(String, String)>>>;

    /// A UPnP router on loopback: an SSDP responder and an HTTP server for its description and
    /// control URL. `answer` gives each SOAP action's status and body.
    async fn fake_router(
        answer: impl Fn(&str, &str) -> (u16, String) + Send + Sync + 'static,
    ) -> (SocketAddr, Calls) {
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_port = http.local_addr().unwrap().port();
        let calls: Calls = Arc::default();
        let recorded = calls.clone();
        let answer = Arc::new(answer);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = http.accept().await else { return };
                let (answer, recorded) = (answer.clone(), recorded.clone());
                tokio::spawn(async move {
                    let mut data = Vec::new();
                    let mut buf = [0u8; 4096];
                    let (head, body) = loop {
                        let n = stream.read(&mut buf).await.unwrap();
                        data.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&data).into_owned();
                        if let Some((head, body)) = text.split_once("\r\n\r\n") {
                            let length = head
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            if body.len() >= length || n == 0 {
                                break (head.to_owned(), body.to_owned());
                            }
                        }
                    };
                    let (status, reply) = if head.starts_with("GET /rootDesc.xml") {
                        (200, DESCRIPTION.to_owned())
                    } else {
                        let action = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase().starts_with("soapaction:").then(|| {
                                    l.rsplit('#').next().unwrap().trim_matches('"').to_owned()
                                })
                            })
                            .unwrap_or_default();
                        recorded.lock().unwrap().push((action.clone(), body.clone()));
                        answer(&action, &body)
                    };
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                        reply.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        let ssdp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let ssdp_addr = ssdp.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            loop {
                let Ok((_, from)) = ssdp.recv_from(&mut buf).await else { return };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
                     LOCATION: http://127.0.0.1:{http_port}/rootDesc.xml\r\n\r\n"
                );
                let _ = ssdp.send_to(reply.as_bytes(), from).await;
            }
        });
        (ssdp_addr, calls)
    }

    fn ok(action: &str, extra: &str) -> (u16, String) {
        (
            200,
            format!(
                r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:{action}Response xmlns:u="urn:schemas-upnp-org:service:WANIPConnection:1">{extra}</u:{action}Response></s:Body></s:Envelope>"#
            ),
        )
    }

    fn router_answers(action: &str, body: &str) -> (u16, String) {
        match action {
            "GetExternalIPAddress" => {
                ok(action, "<NewExternalIPAddress>203.0.113.7</NewExternalIPAddress>")
            }
            // Port 3478 belongs to someone else; this router keeps nothing for a time.
            "AddPortMapping" if body.contains("<NewExternalPort>3478<") => {
                (500, fault(718, "ConflictInMappingEntry"))
            }
            "AddPortMapping" if !body.contains("<NewLeaseDuration>0<") => {
                (500, fault(725, "OnlyPermanentLeasesSupported"))
            }
            _ => ok(action, ""),
        }
    }

    #[tokio::test]
    async fn a_router_is_found_and_forwards_ports() {
        let (ssdp, calls) = fake_router(router_answers).await;
        let gateway = Upnp::find(ssdp, Duration::from_millis(300)).await.unwrap();
        assert_eq!(gateway.external_ip().await.unwrap(), IpAddr::from([203, 0, 113, 7]));
        assert_eq!(gateway.router(), IpAddr::from([127, 0, 0, 1]));

        let granted = gateway.map(Transport::Udp, 50000, 50000, 3600).await.unwrap();
        assert_eq!(
            (granted.external, granted.lifetime),
            (50000, 0),
            "permanent, as the router allows"
        );
        let err = gateway.map(Transport::Tcp, 3478, 3478, 3600).await.unwrap_err();
        assert!(matches!(err, Error::Conflict), "{err}");
        gateway.unmap(Transport::Udp, 50000, 50000).await.unwrap();

        let calls = calls.lock().unwrap();
        let actions: Vec<&str> = calls.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(
            actions,
            [
                "GetExternalIPAddress",
                "GetExternalIPAddress",
                "AddPortMapping",
                "AddPortMapping",
                "AddPortMapping",
                "DeletePortMapping"
            ]
        );
        let add = &calls[3].1;
        for part in [
            "<NewExternalPort>50000</NewExternalPort><NewProtocol>UDP</NewProtocol>",
            "<NewInternalPort>50000</NewInternalPort><NewInternalClient>127.0.0.1</NewInternalClient>",
            "<NewLeaseDuration>0</NewLeaseDuration>",
        ] {
            assert!(add.contains(part), "{part} in {add}");
        }
        assert!(calls[4].1.contains("<NewProtocol>TCP</NewProtocol>"));
    }

    #[tokio::test]
    async fn no_answer_is_not_found() {
        let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let err =
            Upnp::find(silent.local_addr().unwrap(), Duration::from_millis(200)).await.unwrap_err();
        assert!(matches!(err, Error::NotFound), "{err}");
    }

    #[tokio::test]
    async fn a_router_without_an_address_is_not_used() {
        let (ssdp, _) =
            fake_router(|action, _| ok(action, "<NewExternalIPAddress></NewExternalIPAddress>"))
                .await;
        assert!(Upnp::find(ssdp, Duration::from_millis(300)).await.is_err());
    }
}
