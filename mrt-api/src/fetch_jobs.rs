//! Finite live HTTP fetches whose completion must not block the runtime (B185).
use std::collections::HashMap;
use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}, mpsc};
use std::time::{Duration, Instant};
use serde_json::{Value, json};
use crate::{fetch, store::Stores};

const LIMIT: usize = 16;
type Owner = (String, String, bool, String);
fn owner(req: &Value) -> Owner {
    let text = |k: &str| req[k].as_str().unwrap_or("").to_owned();
    (text("site"), text("top"), req["ancestor"].as_bool().unwrap_or(false), text("origin"))
}
struct Job {
    owner: Owner,
    created: Instant,
    answer: mpsc::Receiver<Result<Value, String>>,
}
struct Running(Arc<AtomicUsize>);
impl Drop for Running {
    fn drop(&mut self) { self.0.fetch_sub(1, Ordering::SeqCst); }
}
#[derive(Default)]
pub struct Jobs {
    jobs: HashMap<String, Job>,
    running: Arc<AtomicUsize>,
}
impl Jobs {
    pub fn handle(&mut self, req: &Value, stores: &Arc<Mutex<Stores>>) -> Result<Value, String> {
        match req["call"].as_str().unwrap_or("") {
            "start" => {
                // Abandoned delivery handles expire after the bounded worker deadline.
                self.jobs.retain(|_, j| j.created.elapsed() < Duration::from_secs(125));
                if self.jobs.len() >= LIMIT || self.running.load(Ordering::SeqCst) >= LIMIT {
                    return Err("mrt-api: live fetch capacity exceeded".into());
                }
                // Do not reuse an old request's id after the API process restarts.
                // Strings also avoid JSON/JavaScript's 53-bit integer precision limit.
                let id = loop {
                    let id = format!("{:032x}", rand::random::<u128>());
                    if !self.jobs.contains_key(&id) { break id; }
                };
                let (send, answer) = mpsc::channel();
                let request = req.clone();
                let mut shared = stores.clone();
                let count = self.running.clone();
                self.running.fetch_add(1, Ordering::SeqCst);
                let worker = std::thread::Builder::new().name(format!("fetch-{id}")).spawn(move || {
                    let _running = Running(count);
                    let mut net = fetch::Net::from_env();
                    let result = fetch::live(&request, &mut net, &mut shared, Duration::from_secs(120)).map(|mut value| {
                        // Real time passed while the page was running, not virtual replay time.
                        value["took"] = json!(0);
                        value
                    });
                    let _ = send.send(result);
                });
                if let Err(e) = worker {
                    self.running.fetch_sub(1, Ordering::SeqCst);
                    return Err(format!("mrt-api: fetch worker: {e}"));
                }
                self.jobs.insert(id.clone(), Job { owner: owner(req), created: Instant::now(), answer });
                Ok(json!({"job": id, "pending": true}))
            }
            "poll" | "cancel" => {
                let id = req["id"].as_str().ok_or("mrt-api: missing fetch id")?;
                let job = self.jobs.get(id).ok_or("mrt-api: unknown fetch job")?;
                if job.owner != owner(req) { return Err("mrt-api: wrong fetch owner".into()); }
                if req["call"] == "cancel" {
                    self.jobs.remove(id);
                    return Ok(Value::Null);
                }
                match job.answer.try_recv() {
                    Ok(result) => { self.jobs.remove(id); result }
                    Err(mpsc::TryRecvError::Empty) => Ok(json!({"job": id, "pending": true})),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.jobs.remove(id);
                        Err("mrt-api: fetch worker ended without a response".into())
                    }
                }
            }
            _ => Err("mrt-api: unknown fetch job operation".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(call: &str) -> Value {
        json!({"call":call,"id":"test","site":"http://localhost","top":"http://localhost","origin":"http://localhost"})
    }
    fn pending(jobs: &mut Jobs) -> mpsc::Sender<Result<Value,String>> {
        let (send, answer) = mpsc::channel();
        jobs.jobs.insert("test".into(), Job { owner: owner(&request("poll")), created: Instant::now(), answer });
        send
    }
    #[test]
    fn completion_is_owned_and_consumed_once() {
        let stores = Arc::new(Mutex::new(Stores::default()));
        let mut jobs = Jobs::default();
        let send = pending(&mut jobs);
        assert_eq!(jobs.handle(&request("poll"), &stores).unwrap()["pending"], true);
        for field in ["site", "top", "origin", "ancestor"] {
            let mut wrong = request("poll");
            wrong[field] = if field == "ancestor" { json!(true) } else { json!("http://other") };
            assert!(jobs.handle(&wrong, &stores).is_err());
        }
        send.send(Ok(json!({"status":200,"body":"late bytes"}))).unwrap();
        assert_eq!(jobs.handle(&request("poll"), &stores).unwrap()["body"], "late bytes");
        assert!(jobs.handle(&request("poll"), &stores).is_err());
    }
    #[test]
    fn cancellation_and_dead_worker_settle() {
        let stores = Arc::new(Mutex::new(Stores::default()));
        let mut jobs = Jobs::default();
        let send = pending(&mut jobs);
        jobs.handle(&request("cancel"), &stores).unwrap();
        assert!(send.send(Ok(Value::Null)).is_err());
        assert!(jobs.handle(&request("poll"), &stores).is_err());
        drop(pending(&mut jobs));
        assert!(jobs.handle(&request("poll"), &stores).unwrap_err().contains("worker ended"));
    }
    #[test]
    fn running_workers_stay_bounded_after_cancel() {
        let stores = Arc::new(Mutex::new(Stores::default()));
        let mut jobs = Jobs::default();
        let _send = pending(&mut jobs);
        jobs.running.store(LIMIT, Ordering::SeqCst);
        jobs.handle(&request("cancel"), &stores).unwrap();
        assert!(jobs.handle(&request("start"), &stores).unwrap_err().contains("capacity"));
    }
    #[test]
    fn real_job_delivers_bytes_and_errors() {
        let stores = Arc::new(Mutex::new(Stores::default()));
        let mut jobs = Jobs::default();
        for url in ["data:text/plain;base64,/wBB", "unsupported://host/"] {
            let mut start = request("start");
            start["url"] = json!(url); start["bytes"] = json!(true);
            let id = jobs.handle(&start, &stores).unwrap()["job"].clone();
            let mut poll = request("poll"); poll["id"] = id;
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let result = jobs.handle(&poll, &stores);
                if result.as_ref().is_ok_and(|v| v["pending"] == true) {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1)); continue;
                }
                if url.starts_with("data:") {
                    let r = result.unwrap(); assert_eq!(r["bodyBase64"], "/wBB"); assert_eq!(r["took"], 0);
                } else { assert!(result.is_err()); }
                break;
            }
        }
    }
    #[test]
    fn decoded_response_is_bounded() {
        use std::net::TcpListener;
        use std::io::{Read, Write};
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&vec![0;16 * 1024 * 1024 + 1]).unwrap();
        let compressed = encoder.finish().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0;2048]; let _ = socket.read(&mut bytes);
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n", compressed.len()).unwrap();
            socket.write_all(&compressed).unwrap();
        });
        let mut req = request("start");
        req["url"] = json!(url); req["origin"] = json!(url.trim_end_matches('/'));
        req["credentials"] = json!("omit");
        let error = fetch::live(&req, &mut fetch::Net::from_env(), &mut Stores::default(), Duration::from_secs(2)).unwrap_err();
        assert!(error.contains("body limit"), "{error}");
        server.join().unwrap();
    }
    #[test]
    fn replaced_process_cannot_deliver_another_jobs_response() {
        let stores = Arc::new(Mutex::new(Stores::default()));
        let mut old = Jobs::default(); let mut replacement = Jobs::default();
        let mut start = request("start"); start["url"] = json!("data:text/plain,response");
        let previous = old.handle(&start, &stores).unwrap()["job"].clone();
        let current = replacement.handle(&start, &stores).unwrap()["job"].clone();
        assert_ne!(previous, current);
        let mut stale = request("poll"); stale["id"] = previous;
        assert!(replacement.handle(&stale, &stores).is_err());
    }
    #[test]
    fn finite_live_deadline_is_an_error() {
        use std::net::TcpListener;
        use std::io::{Read, Write};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0;2048]; let _ = socket.read(&mut bytes);
            let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n");
            std::thread::sleep(Duration::from_millis(200));
            let _ = socket.write_all(b"late");
        });
        let mut req = request("start");
        req["url"] = json!(format!("http://127.0.0.1:{port}/"));
        req["mode"] = json!("no-cors"); req["credentials"] = json!("omit");
        let error = fetch::live(&req, &mut fetch::Net::from_env(), &mut Stores::default(), Duration::from_millis(50)).unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        server.join().unwrap();
    }
}
