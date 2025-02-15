use crate::docker_compose::run_command;
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use std::process::{Child, Command, Stdio};

pub struct ShotoverProcess {
    /// Always Some while ShotoverProcess is owned
    pub child: Option<Child>,
}

impl Drop for ShotoverProcess {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            if let Err(err) =
                nix::sys::signal::kill(Pid::from_raw(child.id() as i32), Signal::SIGKILL)
            {
                println!("Failed to shutdown ShotoverProcess {err}");
            }

            if !std::thread::panicking() {
                panic!("Need to call either wait or shutdown_and_assert_success method on ShotoverProcess before dropping it ");
            }
        }
    }
}

impl ShotoverProcess {
    #[allow(dead_code)]
    pub fn new(topology_path: &str) -> ShotoverProcess {
        // First ensure shotover is fully built so that the potentially lengthy build time is not included in the wait_for_socket_to_open timeout
        // PROFILE is set in build.rs from PROFILE listed in https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-build-scripts
        let all_args = if env!("PROFILE") == "release" {
            vec!["build", "--all-features", "--release"]
        } else {
            vec!["build", "--all-features"]
        };
        run_command(env!("CARGO"), &all_args).unwrap();

        // Now actually run shotover and keep hold of the child process
        let all_args = if env!("PROFILE") == "release" {
            vec![
                "run",
                "--all-features",
                "--release",
                "--",
                "-t",
                topology_path,
            ]
        } else {
            vec!["run", "--all-features", "--", "-t", topology_path]
        };
        let child = Some(
            Command::new(env!("CARGO"))
                .args(&all_args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );

        // Wait for observability metrics port to open
        if let Err(err) = crate::try_wait_for_socket_to_open("127.0.0.1", 9001) {
            // Shutdown shotover and panic if shotover hit any kind of failure.
            // Panicking here is good because any errors here are more important then reporting the timeout
            ShotoverProcess { child }.shutdown_and_assert_success();

            // If shotover shutdown fine then just panic with a generic error. Good luck developer.
            panic!("Shotover succesfully started up and shutdown, but the metrics port was never opened: {}", err);
        }

        ShotoverProcess { child }
    }

    #[allow(dead_code)]
    fn pid(&self) -> Pid {
        Pid::from_raw(self.child.as_ref().unwrap().id() as i32)
    }

    #[allow(dead_code)]
    pub fn signal(&self, signal: Signal) {
        nix::sys::signal::kill(self.pid(), signal).unwrap();
    }

    #[allow(dead_code)]
    pub fn wait(mut self) -> WaitOutput {
        let output = self.child.take().unwrap().wait_with_output().unwrap();

        let stdout = String::from_utf8(output.stdout).expect("stdout was not valid utf8");
        let stderr = String::from_utf8(output.stderr).expect("stderr was not valid utf8");

        WaitOutput {
            exit_code: output
                .status
                .code()
                .expect("Couldnt get exit code, the process was killed by something like SIGKILL"),
            stdout,
            stderr,
        }
    }

    #[allow(dead_code)]
    pub fn shutdown_and_assert_success(self) {
        self.signal(nix::sys::signal::Signal::SIGTERM);
        let result = self.wait();

        if result.exit_code != 0 {
            panic!(
                "Shotover exited with {} but expected 0 exit code (Success).\nstdout:\n{}\nstderr:\n{}",
                result.exit_code, result.stdout, result.stderr
            );
        }
    }
}

pub struct WaitOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
