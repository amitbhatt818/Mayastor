use std::{
    fs,
    io,
    io::Write,
    panic,
    process::{Command, Stdio},
    time::Duration,
};

use nix::{
    sys::wait::{waitpid, WaitPidFlag},
    unistd::{gettid, Pid},
};

use mayastor::core::Mthread;

// there is a CARGO_EXEC_$BIN variable in recent Rust which does
// not seem to work yet with our compiler version
fn get_path(bin: &str) -> String {
    if std::path::Path::new("./target/debug/bin").exists() {
        format!("./target/debug/{}", bin)
    } else {
        format!("../target/debug/{}", bin)
    }
}

fn rpc_sock_path() -> String {
    format!("/var/tmp/mayastor-test-{}", gettid())
}

fn hugetlbfs_path() -> String {
    format!("/tmp/mayastor-test-{}", gettid())
}

/// start mayastor as a separate process and run the closure. By wrapping the
/// test closure, we can catch errors but still kill mayastor to avoid dangling
/// process.
pub fn run_test<T>(args: Box<[String]>, test: T)
where
    T: FnOnce(&MayastorProcess) + panic::UnwindSafe,
{
    let ms = MayastorProcess::new(args).unwrap();
    let ret = panic::catch_unwind(|| test(&ms));
    drop(ms);
    assert!(ret.is_ok());
}

#[derive(Debug)]
/// this structure is used to fork mayastor(s) and to test them using
/// (g)rpc calls.
///
/// Note that depending on the configuration that is passed, one or more
/// instances might fail to start as the instances might overlap ports.
pub struct MayastorProcess {
    /// the PID we are tracked under
    child: u32,
    /// the json-rpc socket we listen on
    pub rpc_path: String,
    /// the hugepage directory we are using
    pub hugetlbfs: String,
}

impl MayastorProcess {
    /// start mayastor and open the unix socket, if we are able to connect
    /// we know we are up and running and ready for business.
    pub fn new(args: Box<[String]>) -> Result<Self, ()> {
        let mayastor = get_path("mayastor");

        let (tx, rx) = std::sync::mpsc::channel::<MayastorProcess>();
        Mthread::spawn_unaffinitized(move || {
            if let Err(e) = fs::create_dir(hugetlbfs_path()) {
                panic!("failed to create hugetlbfs mount path {}", e);
            }

            let output = Command::new("mount")
                .args(&[
                    "-t",
                    "hugetlbfs",
                    "nodev",
                    &hugetlbfs_path(),
                    "-o",
                    "pagesize=2048k",
                ])
                .output()
                .expect("could not exec mount");

            if !output.status.success() {
                io::stderr().write_all(&output.stderr).unwrap();
                panic!("failed to mount hugetlbfs");
            }

            let mut child = Command::new(mayastor)
                .args(&["-r", &rpc_sock_path()])
                .args(&["--huge-dir", &hugetlbfs_path()])
                .args(args.into_vec())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();

            while !MayastorProcess::ping(&rpc_sock_path()) {
                match child.try_wait() {
                    Ok(Some(_status)) => tx
                        .send(MayastorProcess {
                            child: child.id(),
                            rpc_path: rpc_sock_path(),
                            hugetlbfs: hugetlbfs_path(),
                        })
                        .unwrap(),
                    Err(_e) => tx
                        .send(MayastorProcess {
                            child: 0,
                            rpc_path: rpc_sock_path(),
                            hugetlbfs: hugetlbfs_path(),
                        })
                        .unwrap(),
                    _ => (),
                };

                std::thread::sleep(Duration::from_millis(200));
            }

            let m = MayastorProcess {
                child: child.id(),
                rpc_path: rpc_sock_path(),
                hugetlbfs: hugetlbfs_path(),
            };

            let _ = tx.send(m);
        });

        let m = rx.recv().unwrap();
        if m.child == 0 {
            panic!("Mayastor not started within deadline");
        } else {
            Ok(m)
        }
    }

    /// check to see if rpc is up
    pub fn ping(path: &str) -> bool {
        use std::os::unix::net::UnixStream;
        let _stream = match UnixStream::connect(path) {
            Ok(stream) => stream,
            Err(_) => return false,
        };
        true
    }

    /// call json-rpc method using the binary
    pub fn rpc_call(
        &self,
        method: &str,
        arg: serde_json::Value,
    ) -> Result<serde_json::Value, ()> {
        let jsonrpc = get_path("jsonrpc");

        let output = Command::new(jsonrpc)
            .args(&["-s", &self.rpc_path, "raw", method])
            .arg(serde_json::to_string(&arg).unwrap())
            .output()
            .expect("could not exec jsonrpc");

        if !output.status.success() {
            panic!(
                "RPC to socket {} with method {} failed arguments {:?}",
                self.rpc_path, method, arg
            );
        }

        let output_string = String::from_utf8_lossy(&output.stdout);
        Ok(serde_json::from_str(&output_string).unwrap())
    }

    fn sig_x(&mut self, sig_str: &str, options: Option<WaitPidFlag>) {
        if self.child == 0 {
            return;
        }
        let child = self.child;
        if sig_str == "TERM" {
            self.child = 0;
        }
        Command::new("kill")
            .args(&["-s", sig_str, &format!("{}", child)])
            .spawn()
            .unwrap();

        // blocks until child changes state, signals are racy by themselves
        // however
        waitpid(Pid::from_raw(child as i32), options).unwrap();
    }

    /// terminate the mayastor process and wait for it to die
    pub fn sig_term(&mut self) {
        self.sig_x("TERM", None);
    }

    /// stop the mayastor process and wait for it to stop
    pub fn sig_stop(&mut self) {
        self.sig_x("STOP", Some(WaitPidFlag::WUNTRACED));
    }

    /// continue the mayastor process and wait for it to continue
    pub fn sig_cont(&mut self) {
        self.sig_x("CONT", Some(WaitPidFlag::WCONTINUED));
    }
}

/// ensure we umount the huge pages during shutdown
impl Drop for MayastorProcess {
    fn drop(&mut self) {
        self.sig_term();
        let _ = Command::new("umount")
            .args(&[&self.hugetlbfs])
            .output()
            .unwrap();
        let _ = fs::remove_dir(&self.hugetlbfs);
        let _ = Command::new("rm").args(&[&self.rpc_path]).output().unwrap();
    }
}
