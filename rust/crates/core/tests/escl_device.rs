//! Real HTTP protocol tests against an in-process synthetic scanner. No hardware is used.
use openscan_core::{
    backend::{Backend, escl::Escl},
    *,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
};

#[test]
fn feeder_acquires_pages_and_releases_the_job() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let address = listener.local_addr().expect("address");
    let log = Arc::new(Mutex::new(Vec::new()));
    let server_log = log.clone();
    listener.set_nonblocking(true).expect("nonblocking");
    let worker = std::thread::spawn(move || {
        let caps = r#"<ScannerCapabilities><AdfSimplexInputCaps><XResolution>300</XResolution><ColorMode>RGB24</ColorMode><DocumentFormat>image/png</DocumentFormat></AdfSimplexInputCaps></ScannerCapabilities>"#;
        for (status, headers, body) in [
            ("200 OK", "", caps.as_bytes()),
            (
                "201 Created",
                "Location: /eSCL/ScanJobs/1\r\n",
                b"".as_slice(),
            ),
            (
                "200 OK",
                "Content-Type: image/png\r\n",
                b"original scanner data".as_slice(),
            ),
            ("404 Not Found", "", b"".as_slice()),
            ("204 No Content", "", b"".as_slice()),
        ] {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let (mut socket, _) = loop {
                match listener.accept() {
                    Ok(pair) => break pair,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            };
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .expect("timeout");
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut chunk).expect("request");
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..n]);
            }
            let header_end = request
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .expect("headers")
                + 4;
            let length = String::from_utf8_lossy(&request[..header_end])
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                        .map(|(_, v)| v.trim().parse::<usize>().expect("length"))
                })
                .unwrap_or(0);
            while request.len() < header_end + length {
                let n = socket.read(&mut chunk).expect("body");
                assert!(n > 0, "truncated request body");
                request.extend_from_slice(&chunk[..n]);
            }
            let request = String::from_utf8_lossy(&request).into_owned();
            server_log.lock().expect("log").push(request);
            write!(
                socket,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
                body.len()
            )
            .expect("headers");
            socket.write_all(body).expect("body");
        }
    });
    let dir = tempfile::tempdir().expect("temp");
    let p = Profile {
        device_id: Some(format!("escl:http://{address}/eSCL/")),
        source: Source::Feeder,
        max_pages: 3,
        ..Default::default()
    };
    let mut pages = Vec::new();
    Escl.acquire(&p, dir.path(), &Cancellation::default(), &mut |page| {
        pages.push(page);
        Ok(())
    })
    .expect("acquire");
    worker.join().expect("server");
    assert_eq!(pages.len(), 1);
    assert_eq!(
        std::fs::read(&pages[0].path).expect("page"),
        b"original scanner data"
    );
    let log = log.lock().expect("log");
    assert!(log[1].starts_with("POST /eSCL/ScanJobs "));
    assert!(log[4].starts_with("DELETE /eSCL/ScanJobs/1 "));
}
