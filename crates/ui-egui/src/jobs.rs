//! 后台任务：一帧只轮询一次，绝不在帧内等待（`docs/DEVELOPMENT.md` §6）。

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use metis_core::{MetisError, Result};

/// 取消令牌。UI 点"取消"时置位，任务自己检查。
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 请求取消。
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// 是否已取消。
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 一个任务的结局。
#[derive(Debug)]
pub enum Finished<T> {
    /// 成功。
    Done(u64, T),
    /// 失败。
    Failed(u64, MetisError),
    /// 被取消。
    Cancelled(u64),
}

/// 后台任务表。
#[derive(Debug)]
pub struct Jobs<T> {
    sender: Sender<(u64, std::result::Result<T, MetisError>)>,
    receiver: Receiver<(u64, std::result::Result<T, MetisError>)>,
    running: Vec<(u64, String, CancelToken)>,
    next_id: u64,
}

impl<T: Send + 'static> Default for Jobs<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + 'static> Jobs<T> {
    /// 新建。
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self { sender, receiver, running: Vec::new(), next_id: 1 }
    }

    /// 起一个任务。返回 id。
    ///
    /// **panic 必须变成失败**：否则任务表里那条记录永远不会消失，界面会永远显示"进行中"，
    /// 而且 CPU 会一直按忙碌节奏重绘（`PITFALLS.md` D2 的同类问题）。
    pub fn spawn(&mut self, label: impl Into<String>, work: impl FnOnce(CancelToken) -> Result<T> + Send + 'static) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let token = CancelToken::new();
        let sender = self.sender.clone();
        let task_token = token.clone();
        std::thread::spawn(move || {
            let result = if task_token.is_cancelled() {
                Err(MetisError::new(metis_core::ErrorKind::Internal, "已取消"))
            } else {
                // 后台线程里的 panic 不该拖垮进程，也不该让界面永远等下去。
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(task_token))) {
                    Ok(result) => result,
                    Err(_) => {
                        Err(MetisError::new(metis_core::ErrorKind::Internal, "后台任务意外中断").with_remediation("重试一次；如果反复出现，请报告这个问题"))
                    }
                }
            };
            let _ = sender.send((id, result));
        });
        self.running.push((id, label.into(), token));
        id
    }

    /// 每帧调一次。
    pub fn poll(&mut self) -> Vec<Finished<T>> {
        let mut finished = Vec::new();
        while let Ok((id, result)) = self.receiver.try_recv() {
            let cancelled = self.running.iter().find(|(running_id, _, _)| *running_id == id).is_some_and(|(_, _, token)| token.is_cancelled());
            self.running.retain(|(running_id, _, _)| *running_id != id);
            match result {
                Ok(value) => finished.push(Finished::Done(id, value)),
                Err(_) if cancelled => finished.push(Finished::Cancelled(id)),
                Err(error) => finished.push(Finished::Failed(id, error)),
            }
        }
        finished
    }

    /// 请求取消一个任务。
    pub fn cancel(&self, id: u64) {
        if let Some((_, _, token)) = self.running.iter().find(|(running_id, _, _)| *running_id == id) {
            token.cancel();
        }
    }

    /// 正在跑的任务数量。
    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    /// 正在跑的任务标签。
    pub fn running_labels(&self) -> Vec<&str> {
        self.running.iter().map(|(_, label, _)| label.as_str()).collect()
    }

    /// 有任务在跑吗（用来决定重绘节奏）。
    pub fn is_busy(&self) -> bool {
        !self.running.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// 等到**至少 `expect` 个**任务结束，然后返回这次收集到的结果。
    ///
    /// 旧版只要收到一个非空结果就返回，于是"起了两个任务、调两次"的测试
    /// 第二次会等满超时（30 秒）才通过 —— 看起来是超时值问题，其实是
    /// 等待条件写错了。显式要求个数就不会有这种歧义。
    fn wait_for_n<T: Send + 'static>(jobs: &mut Jobs<T>, expect: usize, timeout: Duration) -> Vec<Finished<T>> {
        let start = Instant::now();
        let mut collected = Vec::new();
        while collected.len() < expect && start.elapsed() < timeout {
            collected.extend(jobs.poll());
            if collected.len() >= expect {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        collected.extend(jobs.poll());
        collected
    }

    /// 等**一个**任务结束（最常用）。
    fn wait_for<T: Send + 'static>(jobs: &mut Jobs<T>, timeout: Duration) -> Vec<Finished<T>> {
        wait_for_n(jobs, 1, timeout)
    }

    #[test]
    fn job_runs_and_reports_done() {
        let mut jobs: Jobs<u32> = Jobs::new();
        let id = jobs.spawn("计算", |_| Ok(41 + 1));
        assert_eq!(jobs.running_count(), 1);
        let finished = wait_for(&mut jobs, Duration::from_secs(10));
        match finished.as_slice() {
            [Finished::Done(done_id, value)] => {
                assert_eq!(*done_id, id);
                assert_eq!(*value, 42);
            }
            other => panic!("expected Done, got {other:?}"),
        }
        assert_eq!(jobs.running_count(), 0, "跑完必须从表里移除");
        assert!(!jobs.is_busy());
    }

    #[test]
    fn job_failure_is_reported_not_lost() {
        let mut jobs: Jobs<u32> = Jobs::new();
        jobs.spawn("失败", |_| Err(MetisError::new(metis_core::ErrorKind::Io, "boom").with_remediation("重试")));
        let finished = wait_for(&mut jobs, Duration::from_secs(10));
        match finished.as_slice() {
            [Finished::Failed(_, error)] => assert_eq!(error.kind(), metis_core::ErrorKind::Io),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn cancelling_before_start_is_respected() {
        let mut jobs: Jobs<u32> = Jobs::new();
        let id = jobs.spawn("可取消", |token| {
            std::thread::sleep(Duration::from_millis(50));
            if token.is_cancelled() { Ok(0) } else { Ok(1) }
        });
        jobs.cancel(id);
        let finished = wait_for(&mut jobs, Duration::from_secs(10));
        assert_eq!(finished.len(), 1, "必须有且只有一个结局");
    }

    #[test]
    fn poll_without_jobs_is_empty() {
        let mut jobs: Jobs<u32> = Jobs::new();
        assert!(jobs.poll().is_empty());
        assert!(!jobs.is_busy());
        assert_eq!(jobs.running_count(), 0);
    }

    #[test]
    fn ids_are_unique_and_increasing() {
        let mut jobs: Jobs<()> = Jobs::new();
        let a = jobs.spawn("a", |_| Ok(()));
        let b = jobs.spawn("b", |_| Ok(()));
        assert!(b > a);
        // 两个任务，一次等齐（旧写法分两次等，第二次必然等满超时）。
        let finished = wait_for_n(&mut jobs, 2, Duration::from_secs(10));
        assert_eq!(finished.len(), 2, "两个任务都应该结束");
    }

    #[test]
    fn running_labels_are_visible() {
        let mut jobs: Jobs<()> = Jobs::new();
        jobs.spawn("探测本机执行器", |_| {
            std::thread::sleep(Duration::from_millis(30));
            Ok(())
        });
        assert!(jobs.running_labels().contains(&"探测本机执行器"));
        let _ = wait_for(&mut jobs, Duration::from_secs(10));
    }

    #[test]
    fn cancel_token_is_shared() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        token.cancel();
        assert!(clone.is_cancelled(), "克隆必须共享同一个标志");
    }
}
