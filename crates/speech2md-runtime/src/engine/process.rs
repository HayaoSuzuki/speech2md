use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use send_ctrlc::{Interruptible as _, InterruptibleCommand as _};
use wait_timeout::ChildExt as WaitTimeoutChildExt;

use super::EngineError;

const DIAGNOSTIC_TAIL_BYTES: usize = 64 * 1_024;

pub(super) struct ProcessOutcome {
    pub(super) status: ExitStatus,
    pub(super) stdout: CapturedTail,
    pub(super) stderr: CapturedTail,
}

pub(super) struct CapturedTail {
    bytes: Vec<u8>,
    truncated: bool,
}

impl CapturedTail {
    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(super) const fn truncated(&self) -> bool {
        self.truncated
    }
}

pub(super) fn run(
    command: &mut Command,
    cancelled: &Arc<AtomicBool>,
    grace: Duration,
) -> Result<ProcessOutcome, EngineError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn_interruptible()
        .map_err(|error| EngineError::Spawn(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| EngineError::Spawn("stdout pipe was not created".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| EngineError::Spawn("stderr pipe was not created".into()))?;
    let stdout_thread = thread::spawn(move || capture_tail(stdout));
    let stderr_thread = thread::spawn(move || capture_tail(stderr));

    loop {
        if cancelled.load(Ordering::Acquire) {
            let _terminate_result = child.terminate();
            if !matches!(
                WaitTimeoutChildExt::wait_timeout(&mut *child, grace),
                Ok(Some(_))
            ) {
                let _kill_result = child.kill();
            }
            child
                .wait()
                .map_err(|error| EngineError::Spawn(error.to_string()))?;
            join_capture(stdout_thread)?;
            join_capture(stderr_thread)?;
            return Err(EngineError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(_)) => {
                let status = child
                    .wait()
                    .map_err(|error| EngineError::Spawn(error.to_string()))?;
                return Ok(ProcessOutcome {
                    status,
                    stdout: join_capture(stdout_thread)?,
                    stderr: join_capture(stderr_thread)?,
                });
            }
            Ok(None) => {}
            Err(error) => {
                let _kill_result = child.kill();
                let _wait_result = child.wait();
                let _stdout_result = join_capture(stdout_thread);
                let _stderr_result = join_capture(stderr_thread);
                return Err(EngineError::Spawn(error.to_string()));
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn capture_tail(mut reader: impl Read) -> Result<CapturedTail, std::io::Error> {
    let mut tail = Vec::with_capacity(DIAGNOSTIC_TAIL_BYTES);
    let mut truncated = false;
    let mut buffer = [0_u8; 8 * 1_024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if count >= DIAGNOSTIC_TAIL_BYTES {
            tail.clear();
            tail.extend_from_slice(&buffer[count - DIAGNOSTIC_TAIL_BYTES..count]);
            truncated = true;
            continue;
        }
        let overflow = tail
            .len()
            .saturating_add(count)
            .saturating_sub(DIAGNOSTIC_TAIL_BYTES);
        if overflow > 0 {
            tail.drain(..overflow);
            truncated = true;
        }
        tail.extend_from_slice(&buffer[..count]);
    }
    Ok(CapturedTail {
        bytes: tail,
        truncated,
    })
}

fn join_capture(
    handle: thread::JoinHandle<Result<CapturedTail, std::io::Error>>,
) -> Result<CapturedTail, EngineError> {
    handle
        .join()
        .map_err(|_| EngineError::Cleanup("diagnostic reader thread panicked".into()))?
        .map_err(|error| EngineError::Cleanup(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{DIAGNOSTIC_TAIL_BYTES, capture_tail};

    #[test]
    fn capture_retains_only_the_last_sixty_four_kibibytes() {
        let input = (0_u8..=250).cycle().take(70_000).collect::<Vec<_>>();
        let captured = capture_tail(input.as_slice()).expect("capture bytes");
        assert_eq!(captured.len(), DIAGNOSTIC_TAIL_BYTES);
        assert!(captured.truncated());
        assert_eq!(captured.bytes, input[input.len() - DIAGNOSTIC_TAIL_BYTES..]);
    }
}
