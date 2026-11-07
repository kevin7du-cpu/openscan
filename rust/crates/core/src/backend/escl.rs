//! eSCL/AirScan over bounded HTTP requests, with mDNS device discovery.
use super::*;
use std::{
    collections::BTreeMap,
    io::Read,
    time::{Duration, Instant},
};
use url::Url;
pub struct Escl;
fn client() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(90)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}
fn base(id: &str) -> Result<Url> {
    let text = id
        .strip_prefix("escl:")
        .ok_or_else(|| error("Invalid AirScan identifier."))?;
    let mut url = Url::parse(text).map_err(|e| error(e.to_string()))?;
    if !["http", "https"].contains(&url.scheme())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(error(
            "AirScan needs an HTTP(S) scanner URL without credentials, query or fragment.",
        ));
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}
fn xml(id: &str) -> Result<String> {
    let url = base(id)?
        .join("ScannerCapabilities")
        .map_err(|e| error(e.to_string()))?;
    let mut response = client()
        .get(url.as_str())
        .call()
        .map_err(|e| error(e.to_string()))?;
    if response.status() != 200 {
        return Err(error(format!(
            "AirScan capabilities returned HTTP {}",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(error("Scanner capability response exceeds 1 MB."));
    }
    String::from_utf8(bytes).map_err(|e| error(e.to_string()))
}
fn parse(text: &str) -> Result<roxmltree::Document<'_>> {
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 20_000,
            ..Default::default()
        },
    )
    .map_err(|e| error(e.to_string()))
}
fn source_tag(source: Source) -> &'static str {
    match source {
        Source::Flatbed => "PlatenInputCaps",
        Source::Feeder => "AdfSimplexInputCaps",
        Source::Duplex => "AdfDuplexInputCaps",
    }
}
fn caps(id: &str, text: &str) -> Result<Capabilities> {
    let doc = parse(text)?;
    let mut sources = Vec::new();
    for source in [Source::Flatbed, Source::Feeder, Source::Duplex] {
        if let Some(node) = doc
            .descendants()
            .find(|n| n.has_tag_name(source_tag(source)))
        {
            let mut dpi: Vec<u32> = node
                .descendants()
                .filter(|n| n.has_tag_name("XResolution"))
                .filter_map(|n| n.text()?.parse().ok())
                .filter(|v| (75..=600).contains(v))
                .collect();
            dpi.sort();
            dpi.dedup();
            if dpi.is_empty() {
                for range in node
                    .descendants()
                    .filter(|n| n.has_tag_name("XResolutionRange"))
                {
                    let number = |name| {
                        range
                            .children()
                            .find(|n| n.has_tag_name(name))
                            .and_then(|n| n.text())
                            .and_then(|v| v.parse::<u32>().ok())
                    };
                    if let (Some(min), Some(max)) = (number("Min"), number("Max")) {
                        let step = number("Step").unwrap_or(1).max(1);
                        dpi.extend(
                            (75..=600).filter(|v| *v >= min && *v <= max && (*v - min) % step == 0),
                        );
                    }
                }
                dpi.sort();
                dpi.dedup();
            }
            // Zero means the driver did not advertise a usable range, not 75–600 support.
            let min = dpi.first().copied().unwrap_or(0);
            let max = dpi.last().copied().unwrap_or(0);
            sources.push(SourceCaps {
                source,
                dpi,
                min_dpi: min,
                max_dpi: max,
            });
        }
    }
    if sources.is_empty() {
        return Err(error("The scanner advertises no supported input sources."));
    }
    Ok(Capabilities{device_id:id.into(),sources,note:"AirScan capabilities reported by the scanner. TLS certificates are validated for HTTPS devices.".into()})
}
impl Backend for Escl {
    fn devices(&self, cancel: &Cancellation) -> Result<Vec<Device>> {
        let mdns = mdns_sd::ServiceDaemon::new().map_err(|e| error(e.to_string()))?;
        let receivers = [
            mdns.browse("_uscan._tcp.local.")
                .map_err(|e| error(e.to_string()))?,
            mdns.browse("_uscans._tcp.local.")
                .map_err(|e| error(e.to_string()))?,
        ];
        let start = Instant::now();
        let mut devices = BTreeMap::new();
        while start.elapsed() < Duration::from_secs(3) && !cancel.cancelled() {
            for receiver in &receivers {
                if let Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) =
                    receiver.recv_timeout(Duration::from_millis(40))
                {
                    let scheme = if info.get_fullname().contains("._uscans._tcp.") {
                        "https"
                    } else {
                        "http"
                    };
                    let host = info.get_hostname().trim_end_matches('.');
                    let rs = info
                        .get_property_val_str("rs")
                        .unwrap_or("eSCL")
                        .trim_matches('/');
                    let id = format!("escl:{scheme}://{host}:{}/{rs}/", info.get_port());
                    if base(&id).is_ok() {
                        let name = info
                            .get_property_val_str("ty")
                            .unwrap_or(info.get_fullname())
                            .to_string();
                        devices.insert(
                            id.clone(),
                            Device {
                                id,
                                name,
                                driver: "AirScan".into(),
                            },
                        );
                    }
                }
            }
        }
        let _ = mdns.shutdown();
        cancel.check()?;
        Ok(devices.into_values().collect())
    }
    fn capabilities(&self, id: &str) -> Result<Capabilities> {
        caps(id, &xml(id)?)
    }
    fn acquire(
        &self,
        p: &Profile,
        directory: &Path,
        cancel: &Cancellation,
        on_page: &mut dyn FnMut(RawPage) -> Result<()>,
    ) -> Result<()> {
        p.validate()?;
        cancel.check()?;
        let id = p
            .device_id
            .as_deref()
            .ok_or_else(|| error("Select a scanner."))?;
        let text = xml(id)?;
        let capabilities = caps(id, &text)?;
        if capabilities
            .sources
            .iter()
            .any(|s| s.source == p.source && !s.dpi.is_empty() && !s.dpi.contains(&p.dpi))
        {
            return Err(error(
                "The AirScan device does not advertise the selected resolution.",
            ));
        }
        let doc = parse(&text)?;
        let input = doc
            .descendants()
            .find(|n| n.has_tag_name(source_tag(p.source)))
            .ok_or_else(|| error("The AirScan device does not support this input source."))?;
        let formats: Vec<_> = input
            .descendants()
            .filter(|n| n.has_tag_name("DocumentFormat") || n.has_tag_name("DocumentFormatExt"))
            .filter_map(|n| n.text())
            .collect();
        let (format, ext) = if formats.contains(&"image/png") {
            ("image/png", "png")
        } else if formats.contains(&"image/jpeg") {
            ("image/jpeg", "jpg")
        } else {
            return Err(error("AirScan device must support PNG or JPEG transfer."));
        };
        let color = if p.color == Color::Color {
            "RGB24"
        } else {
            "Grayscale8"
        };
        if !input
            .descendants()
            .any(|n| n.has_tag_name("ColorMode") && n.text() == Some(color))
        {
            return Err(error("AirScan device does not support this color mode."));
        }
        let (w, h) = p.paper.millimeters();
        let width = (w / 25.4 * 300.).round() as u32;
        let height = (h / 25.4 * 300.).round() as u32;
        for (name, requested) in [("MaxWidth", width), ("MaxHeight", height)] {
            if let Some(max) = input
                .children()
                .find(|n| n.has_tag_name(name))
                .and_then(|n| n.text())
                .and_then(|v| v.parse::<u32>().ok())
                && requested > max.saturating_add(2)
            {
                return Err(error(
                    "The paper size exceeds the AirScan source's scan area.",
                ));
            }
        }
        let input_name = if p.source == Source::Flatbed {
            "Platen"
        } else {
            "Feeder"
        };
        let duplex = p.source == Source::Duplex;
        let body = format!(
            r#"<?xml version="1.0"?><scan:ScanSettings xmlns:scan="http://schemas.hp.com/imaging/escl/2011/05/03" xmlns:pwg="http://www.pwg.org/schemas/2010/12/sm"><pwg:Version>2.0</pwg:Version><pwg:ScanRegions><pwg:ScanRegion><pwg:Height>{height}</pwg:Height><pwg:Width>{width}</pwg:Width><pwg:XOffset>0</pwg:XOffset><pwg:YOffset>0</pwg:YOffset></pwg:ScanRegion></pwg:ScanRegions><pwg:InputSource>{input_name}</pwg:InputSource><scan:ColorMode>{color}</scan:ColorMode><scan:XResolution>{dpi}</scan:XResolution><scan:YResolution>{dpi}</scan:YResolution><scan:DocumentFormatExt>{format}</scan:DocumentFormatExt><scan:Duplex>{duplex}</scan:Duplex></scan:ScanSettings>"#,
            dpi = p.dpi
        );
        let base = base(id)?;
        let endpoint = base.join("ScanJobs").map_err(|e| error(e.to_string()))?;
        let agent = client();
        let response = agent
            .post(endpoint.as_str())
            .header("Content-Type", "text/xml")
            .send(body.as_bytes())
            .map_err(|e| error(e.to_string()))?;
        if response.status() != 201 {
            return Err(error(format!(
                "AirScan could not start a scan (HTTP {}). Check the feeder and selected options.",
                response.status()
            )));
        }
        let location = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| error("Scanner omitted the scan job URL."))?;
        let job = endpoint.join(location).map_err(|e| error(e.to_string()))?;
        if job.origin() != base.origin() || !job.path().starts_with(base.path()) {
            return Err(error("Scanner returned a job URL outside its own service."));
        }
        let result = (|| {
            std::fs::create_dir_all(directory)?;
            let count = if p.source == Source::Flatbed {
                1
            } else {
                p.max_pages
            };
            let mut received = 0;
            let next = format!("{}/NextDocument", job.as_str().trim_end_matches('/'));
            for i in 0..count {
                cancel.check()?;
                let mut response = agent.get(&next).call().map_err(|e| error(e.to_string()))?;
                if response.status() == 404 || response.status() == 410 {
                    break;
                }
                if response.status() != 200 {
                    return Err(error(format!(
                        "AirScan transfer stopped (HTTP {}). Completed pages were kept.",
                        response.status()
                    )));
                }
                let mut bytes = Vec::new();
                response
                    .body_mut()
                    .as_reader()
                    .take(correction::MAX_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() as u64 > correction::MAX_BYTES {
                    return Err(error("Scanned image exceeds 256 MB."));
                }
                let path = directory.join(format!("raw-{i:04}.{ext}"));
                storage::atomic_write(&path, &bytes, false)?;
                on_page(RawPage { path, dpi: p.dpi })?;
                received += 1;
            }
            if received == 0 {
                Err(error("The scanner returned no pages. Check the feeder."))
            } else {
                Ok(())
            }
        })();
        // Releasing a job on all paths prevents a failed host from leaving the device busy.
        let _ = agent.delete(job.as_str()).call();
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_credentials_and_non_http_urls() {
        for id in [
            "escl:file:///etc/passwd",
            "escl:http://user:secret@scanner/eSCL",
            "escl:http://scanner/eSCL?token=x",
        ] {
            assert!(base(id).is_err());
        }
    }
    #[test]
    fn namespace_independent_capabilities() {
        let text = r#"<s:ScannerCapabilities xmlns:s="urn:test"><s:PlatenInputCaps><s:XResolution>300</s:XResolution><s:XResolution>600</s:XResolution></s:PlatenInputCaps></s:ScannerCapabilities>"#;
        let c = caps("escl:http://scanner/eSCL", text).expect("caps");
        assert_eq!(c.sources.len(), 1);
        assert_eq!(c.sources[0].dpi, vec![300, 600]);
    }
}
