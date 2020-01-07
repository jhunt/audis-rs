use audis;

use rand;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

use std::fs;
use std::process;
use std::thread::sleep;
use std::time::Duration;

struct RedisServer {
    process: process::Child,
    url: String,
    path: String,
}

impl RedisServer {
    fn new() -> RedisServer {
        let mut cmd = process::Command::new("redis-server");
        cmd.stdout(process::Stdio::null())
            .stderr(process::Stdio::null());

        let path = {
            let (a, b) = rand::random::<(u64, u64)>();
            let path = format!("/tmp/redis-rs-test-{}-{}.sock", a, b);
            cmd.arg("--port").arg("0").arg("--unixsocket").arg(&path);
            path
        };

        let url = format!("unix:{}", path);
        let process = cmd.spawn().unwrap();
        RedisServer { process, path, url }
    }

    fn stop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        fs::remove_file(&self.path).ok();
    }
}

impl Drop for RedisServer {
    fn drop(&mut self) {
        self.stop()
    }
}

fn server() -> (RedisServer, audis::Client) {
    let s = RedisServer::new();
    let c;

    let ms = Duration::from_millis(1);
    loop {
        match audis::Client::connect(&s.url) {
            Err(err) => {
                if err.is_connection_refusal() {
                    println!("trying to connect; failing.  sleeping for 1ms");
                    sleep(ms);
                } else {
                    panic!("Could not connect: {}", err);
                }
            }
            Ok(con) => {
                c = con;
                break;
            }
        };
    }

    (s, c)
}

fn id() -> String {
    thread_rng().sample_iter(&Alphanumeric).take(30).collect()
}

#[test]
fn it_indexes_across_multiple_subjects() {
    let (s, c) = server();

    let id1 = id();
    c.log(&audis::Event {
        id: id1.to_string(),
        data: "{id1 data}".to_string(),
        subjects: vec!["system".to_string(), "user:42".to_string()],
    })
    .unwrap();

    let log = c.retrieve("system").unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].id, id1);

    let log = c.retrieve("user:42").unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].id, id1);

    let log = c.retrieve("enoent").unwrap();
    assert_eq!(log.len(), 0);

    drop(s);
}

#[test]
fn it_inserts_audit_events_in_order() {
    let (s, c) = server();

    let ids = vec![id(), id(), id()];
    let subj = vec!["all".to_string()];

    let log = c.retrieve(&subj[0]).unwrap();
    assert_eq!(log.len(), 0);

    for id in &ids {
        c.log(&audis::Event {
            id: id.to_string(),
            data: format!("[{} data]", id),
            subjects: subj.clone(),
        }).unwrap();
    }

    let log = c.retrieve(&subj[0]).unwrap();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].id, ids[0]);
    assert_eq!(log[1].id, ids[1]);
    assert_eq!(log[2].id, ids[2]);

    drop(s);
}

#[test]
fn it_can_function_in_a_background_thread() {
    let (s, c) = server();

    let ids = vec![id(), id(), id()];
    let subj = vec!["all".to_string()];

    let log = c.retrieve(&subj[0]).unwrap();
    assert_eq!(log.len(), 0);

    let (tx, tid) = c.background(2).unwrap();

    for id in &ids {
        tx.send(audis::Event {
            id: id.to_string(),
            data: format!("[{} data]", id),
            subjects: subj.clone(),
        }).unwrap();
    }
    drop(tx);
    tid.join().unwrap();

    let log = c.retrieve(&subj[0]).unwrap();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].id, ids[0]);
    assert_eq!(log[1].id, ids[1]);
    assert_eq!(log[2].id, ids[2]);

    drop(s);
}

#[test]
fn it_truncates_log_indices() {
    let (s, c) = server();

    let ids = vec![id(), id(), id()];
    let subj = vec!["all".to_string()];

    let log = c.retrieve(&subj[0]).unwrap();
    assert_eq!(log.len(), 0);

    for id in &ids {
        c.log(&audis::Event {
            id: id.to_string(),
            data: format!("[{} data]", id),
            subjects: subj.clone(),
        }).unwrap();
    }

    let log = c.retrieve(&subj[0]).unwrap();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].id, ids[0]);
    assert_eq!(log[1].id, ids[1]);
    assert_eq!(log[2].id, ids[2]);

    c.truncate(&subj[0], 2).unwrap();
    let log = c.retrieve(&subj[0]).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].id, ids[1]);
    assert_eq!(log[1].id, ids[2]);

    drop(s);
}

#[test]
fn it_purges_logs() {
    let (s, c) = server();

    let ids = vec![id(), id(), id()];
    let subj = vec!["all".to_string()];

    let log = c.retrieve(&subj[0]).unwrap();
    assert_eq!(log.len(), 0);

    for id in &ids {
        c.log(&audis::Event {
            id: id.to_string(),
            data: format!("[{} data]", id),
            subjects: subj.clone(),
        }).unwrap();
    }

    let log = c.retrieve(&subj[0]).unwrap();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].id, ids[0]);
    assert_eq!(log[1].id, ids[1]);
    assert_eq!(log[2].id, ids[2]);

    c.purge(&subj[0], &ids[1]).unwrap();
    let log = c.retrieve(&subj[0]).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].id, ids[2]);

    drop(s);
}
