use std::io::{self, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};

pub struct ProcessMetrics {
    pub duration: Duration,
    pub user_cpu: Option<Duration>,
    pub system_cpu: Option<Duration>,
    pub peak_memory_kib: Option<u64>,
}

pub struct ProcessOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub metrics: ProcessMetrics,
}

pub fn run_command_with_metrics(command: &mut Command) -> Result<ProcessOutput> {
    let started = Instant::now();
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn remote backup process")?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Failed to capture remote backup process stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("Failed to capture remote backup process stderr"))?;

    let stdout_handle = thread::spawn(move || read_all(stdout));
    let stderr_handle = thread::spawn(move || read_all(stderr));

    let pid = child.id() as libc::pid_t;
    let mut status = 0;
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    let waited_pid = unsafe { libc::wait4(pid, &mut status, 0, usage.as_mut_ptr()) };
    let duration = started.elapsed();

    if waited_pid < 0 {
        let err = io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        bail!("Failed to wait for remote backup process: {err}");
    }

    let usage = unsafe { usage.assume_init() };
    let stdout = stdout_handle
        .join()
        .map_err(|_| anyhow!("Remote backup stdout reader thread panicked"))??;
    let stderr = stderr_handle
        .join()
        .map_err(|_| anyhow!("Remote backup stderr reader thread panicked"))??;

    Ok(ProcessOutput {
        status: ExitStatus::from_raw(status),
        stdout,
        stderr,
        metrics: ProcessMetrics {
            duration,
            user_cpu: timeval_to_duration(usage.ru_utime),
            system_cpu: timeval_to_duration(usage.ru_stime),
            peak_memory_kib: (usage.ru_maxrss > 0).then_some(usage.ru_maxrss as u64),
        },
    })
}

fn read_all(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    reader.read_to_end(&mut output)?;
    Ok(output)
}

pub fn replay_output(output: &ProcessOutput) -> Result<()> {
    io::stdout()
        .write_all(&output.stdout)
        .context("Failed to replay remote backup stdout")?;
    io::stderr()
        .write_all(&output.stderr)
        .context("Failed to replay remote backup stderr")?;
    Ok(())
}

fn timeval_to_duration(time: libc::timeval) -> Option<Duration> {
    if time.tv_sec < 0 || time.tv_usec < 0 {
        return None;
    }

    Some(Duration::new(
        time.tv_sec as u64,
        (time.tv_usec as u32).saturating_mul(1000),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_metrics_for_successful_process() -> Result<()> {
        let mut command = Command::new("true");
        let output = run_command_with_metrics(&mut command)?;

        assert!(output.status.success());
        assert!(output.metrics.user_cpu.is_some());
        assert!(output.metrics.system_cpu.is_some());
        Ok(())
    }
}
