//! Finite live HTTP fetches whose completion must not block the runtime (B185).
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};
use serde_json::{Value, json};
use crate::{fetch, store::Stores};

// Workers at once, in all and per host (Firefox's max-persistent-connections-per-server).
// Starts beyond them wait in a queue, as Firefox's connection manager queues them (B206).
const LIMIT: usize = 16;
const HOST_LIMIT: usize = 6;
// Handles a page may hold before a start is refused; only a page that never polls gets here.
const PENDING_LIMIT: usize = 4096;
type Owner = (String, String, bool, String);
fn owner(req: &Value) -> Owner {
    let text = |k: &str| req[k].as_str().unwrap_or("").to_owned();
    (text("site"), text("top"), req["ancestor"].as_bool().unwrap_or(false), text("origin"))
}
fn host(req: &Value) -> String {
    let url = url::Url::parse(req["url"].as_str().unwrap_or(""));
    url.ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default()
}
type Answer = Result<Value, String>;
struct Job {
    owner: Owner,
    // Set when a worker takes the request; a queued job does not expire.
    started: Arc<OnceLock<Instant>>,
    answer: mpsc::Receiver<Answer>,
}
struct Request {
    id: String,
    host: String,
    request: Value,
    started: Arc<OnceLock<Instant>>,
    send: mpsc::Sender<Answer>,
}
#[derive(Default)]
struct Queue {
    waiting: VecDeque<Request>,
    running: usize,
    by_host: HashMap<String, usize>,
}
impl Queue {
    fn has_room(&self, host: &str) -> bool {
        self.running < LIMIT && self.by_host.get(host).copied().unwrap_or(0) < HOST_LIMIT
    }
    fn take(&mut self, request: &Request) {
        self.running += 1;
        *self.by_host.entry(request.host.clone()).or_default() += 1;
    }
    fn release(&mut self, host: &str) {
        self.running -= 1;
        if let Some(n) = self.by_host.get_mut(host) {
            *n -= 1;
            if *n == 0 { self.by_host.remove(host); }
        }
    }
    /// The oldest waiting request whose host has room, counted as running.
    fn next(&mut self) -> Option<Request> {
        let i = self.waiting.iter().position(|r| self.has_room(&r.host))?;
        let request = self.waiting.remove(i)?;
        self.take(&request);
        Some(request)
    }
}
#[derive(Default)]
pub struct Jobs {
    jobs: HashMap<String, Job>,
    queue: Arc<Mutex<Queue>>,
}
/// Runs one worker: fetches `first`, then the queued requests it frees room for, until none can start.
///
/// The caller has already counted `first` as running.
fn work(queue: Arc<Mutex<Queue>>, first: Request, stores: Arc<Mutex<Stores>>) {
    let mut shared = stores;
    let mut net = fetch::Net::from_env();
    let mut current = first;
    loop {
        let _ = current.started.set(Instant::now());
        let result = fetch::live(&current.request, &mut net, &mut shared, Duration::from_secs(120)).map(|mut value| {
            // Real time passed while the page was running, not virtual replay time.
            value["took"] = json!(0);
            value
        });
        let _ = current.send.send(result);
        let mut queue = queue.lock().unwrap();
        queue.release(&current.host);
        match queue.next() { Some(next) => current = next, None => return }
    }
}
impl Jobs {
    pub fn handle(&mut self, req: &Value, stores: &Arc<Mutex<Stores>>) -> Result<Value, String> {
        match req["call"].as_str().unwrap_or("") {
            "start" => {
                // Abandoned delivery handles expire after the bounded worker deadline.
                self.jobs.retain(|_, j| j.started.get().is_none_or(|t| t.elapsed() < Duration::from_secs(125)));
                if self.jobs.len() >= PENDING_LIMIT {
                    return Err("mrt-api: live fetch capacity exceeded".into());
                }
                // Do not reuse an old request's id after the API process restarts.
                // Strings also avoid JSON/JavaScript's 53-bit integer precision limit.
                let id = loop {
                    let id = format!("{:032x}", rand::random::<u128>());
                    if !self.jobs.contains_key(&id) { break id; }
                };
                let (send, answer) = mpsc::channel();
                let started = Arc::new(OnceLock::new());
                let request = Request { id: id.clone(), host: host(req), request: req.clone(), started: started.clone(), send };
                let mut queue = self.queue.lock().unwrap();
                if queue.has_room(&request.host) {
                    queue.take(&request);
                    let host = request.host.clone();
                    let (shared, stores) = (self.queue.clone(), stores.clone());
                    let worker = std::thread::Builder::new().name(format!("fetch-{id}"))
                        .spawn(move || work(shared, request, stores));
                    if let Err(e) = worker {
                        queue.release(&host);
                        return Err(format!("mrt-api: fetch worker: {e}"));
                    }
                } else {
                    queue.waiting.push_back(request);
                }
                drop(queue);
                self.jobs.insert(id.clone(), Job { owner: owner(req), started, answer });
                Ok(json!({"job": id, "pending": true}))
            }
            "poll" | "cancel" => {
                let id = req["id"].as_str().ok_or("mrt-api: missing fetch id")?;
                let job = self.jobs.get(id).ok_or("mrt-api: unknown fetch job")?;
                if job.owner != owner(req) { return Err("mrt-api: wrong fetch owner".into()); }
                if req["call"] == "cancel" {
                    self.jobs.remove(id);
                    self.queue.lock().unwrap().waiting.retain(|r| r.id != id);
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
        jobs.jobs.insert("test".into(), Job { owner: owner(&request("poll")), started: Arc::new(OnceLock::new()), answer });
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
    fn poll_until_done(jobs: &mut Jobs, stores: &Arc<Mutex<Stores>>, id: &Value) -> Result<Value, String> {
        let mut poll = request("poll"); poll["id"] = id.clone();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let result = jobs.handle(&poll, stores);
            if !result.as_ref().is_ok_and(|v| v["pending"] == true) { return result; }
            assert!(Instant::now() < deadline, "job {id} never finished");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    // A server that answers each connection only once `release` lets it.
    fn held_server(release: mpsc::Receiver<()>) -> String {
        use std::net::TcpListener;
        use std::io::{Read, Write};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/", listener.local_addr().unwrap().port());
        let release = Arc::new(Mutex::new(release));
        std::thread::spawn(move || for socket in listener.incoming() {
            let (mut socket, release) = (socket.unwrap(), release.clone());
            std::thread::spawn(move || {
                let mut bytes = [0;2048]; let _ = socket.read(&mut bytes);
                let _ = release.lock().unwrap().recv();
                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
            });
        });
        url
    }
    fn live_start(url: &str) -> Value {
        let mut start = request("start");
        start["url"] = json!(url); start["mode"] = json!("no-cors"); start["credentials"] = json!("omit");
        start
    }
    #[test]
    fn burst_beyond_the_workers_is_queued_not_refused() {
        let stores = Arc::new(Mutex::new(Stores::default()));
        let mut jobs = Jobs::default();
        let (release, held) = mpsc::channel();
        let url = held_server(held);
        let ids: Vec<Value> = (0..40).map(|_| jobs.handle(&live_start(&url), &stores).unwrap()["job"].clone()).collect();
        {
            let queue = jobs.queue.lock().unwrap();
            assert_eq!((queue.running, queue.waiting.len()), (HOST_LIMIT, 40 - HOST_LIMIT));
        }
        for _ in 0..40 { release.send(()).unwrap(); }
        // no-cors from another origin: each answer is an opaque response, not an error
        for id in &ids { assert_eq!(poll_until_done(&mut jobs, &stores, id).unwrap()["type"], "opaque"); }
        assert_eq!(jobs.queue.lock().unwrap().running, 0);
    }
    #[test]
    fn a_busy_host_does_not_hold_another_hosts_request() {
        let stores = Arc::new(Mutex::new(Stores::default()));
        let mut jobs = Jobs::default();
        let (release, held) = mpsc::channel();
        let busy = held_server(held);
        for _ in 0..30 { jobs.handle(&live_start(&busy), &stores).unwrap(); }
        let other = jobs.handle(&live_start("data:text/plain,other"), &stores).unwrap()["job"].clone();
        assert_eq!(poll_until_done(&mut jobs, &stores, &other).unwrap()["body"], "other");
        drop(release);
    }
    #[test]
    fn cancelled_queued_job_never_starts() {
        let stores = Arc::new(Mutex::new(Stores::default()));
        let mut jobs = Jobs::default();
        jobs.queue.lock().unwrap().running = LIMIT;
        let mut start = request("start"); start["url"] = json!("data:text/plain,x");
        let id = jobs.handle(&start, &stores).unwrap()["job"].clone();
        assert_eq!(jobs.queue.lock().unwrap().waiting.len(), 1);
        let mut cancel = request("cancel"); cancel["id"] = id;
        jobs.handle(&cancel, &stores).unwrap();
        assert!(jobs.queue.lock().unwrap().waiting.is_empty());
    }
    #[test]
    fn pending_handles_stay_bounded() {
        let stores = Arc::new(Mutex::new(Stores::default()));
        let mut jobs = Jobs::default();
        jobs.queue.lock().unwrap().running = LIMIT;
        let mut start = request("start"); start["url"] = json!("data:text/plain,x");
        for _ in 0..PENDING_LIMIT { jobs.handle(&start, &stores).unwrap(); }
        assert!(jobs.handle(&start, &stores).unwrap_err().contains("capacity"));
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
